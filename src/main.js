const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const SETTINGS_KEY = "slidecatcher.settings";

let sources = [];
let region = null; // {x,y,w,h} w ułamkach obrazu źródła
let running = false;
let startedAt = 0; // chroni przed podwójnym kliknięciem: Start i Stop są w tym samym miejscu

// ------------------------------------------------------------ ustawienia

function loadSettings() {
  try {
    const s = JSON.parse(localStorage.getItem(SETTINGS_KEY) || "{}");
    for (const id of ["threshold", "settle", "interval"]) if (s[id] != null) $(id).value = s[id];
    if (s.skipRepeats != null) $("skip-repeats").checked = s.skipRepeats;
    return s;
  } catch { return {}; }
}

function saveSettings() {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({
      threshold: $("threshold").value,
      settle: $("settle").value,
      interval: $("interval").value,
      skipRepeats: $("skip-repeats").checked,
      sourceKey: JSON.stringify(currentSource()),
      region,
    }));
  } catch {}
}

function renderLabels() {
  $("threshold-val").textContent = `${Number($("threshold").value).toFixed(1)}% pikseli`;
  $("settle-val").textContent = `${Number($("settle").value).toFixed(1)} s`;
  $("interval-val").textContent = `${$("interval").value} ms`;
}

// ------------------------------------------------------------ źródła i podgląd

function currentSource() {
  const s = sources[Number($("source").value)];
  return s ? s.source : null;
}

async function loadSources(preferred) {
  try {
    sources = await invoke("list_sources");
  } catch (e) {
    setStatus("error", `Nie mogę pobrać listy ekranów: ${e}`);
    return;
  }
  const sel = $("source");
  sel.innerHTML = "";
  sources.forEach((s, i) => sel.add(new Option(s.label, String(i))));
  // domyślnie: zapamiętany wybór, a jeśli go nie ma, pierwszy monitor, który nie jest główny (np. ten z Teams)
  let idx = sources.findIndex((s) => JSON.stringify(s.source) === preferred);
  if (idx < 0) idx = sources.findIndex((s) => s.source.kind === "monitor" && !s.label.includes("główny"));
  if (idx < 0) idx = 0;
  sel.value = String(idx);
  await loadPreview();
}

async function loadPreview() {
  const src = currentSource();
  if (!src) return;
  $("preview-empty").hidden = true;
  try {
    $("preview").src = await invoke("preview", { source: src });
  } catch (e) {
    $("preview").removeAttribute("src");
    $("preview-empty").hidden = false;
    $("preview-empty").textContent = `Brak podglądu: ${e}`;
  }
}

// ------------------------------------------------------------ zaznaczanie regionu

function imgRect() {
  const img = $("preview").getBoundingClientRect();
  const wrap = $("preview-wrap").getBoundingClientRect();
  return { left: img.left - wrap.left, top: img.top - wrap.top, width: img.width, height: img.height };
}

function drawRegion() {
  const box = $("region");
  $("clear-region").hidden = !region;
  if (!region || !$("preview").naturalWidth) { box.hidden = true; return; }
  const r = imgRect();
  box.hidden = false;
  box.style.left = `${r.left + region.x * r.width}px`;
  box.style.top = `${r.top + region.y * r.height}px`;
  box.style.width = `${region.w * r.width}px`;
  box.style.height = `${region.h * r.height}px`;
}

function setupRegionSelection() {
  const wrap = $("preview-wrap");
  let start = null;
  const toFrac = (e) => {
    const img = $("preview").getBoundingClientRect();
    return {
      x: Math.min(1, Math.max(0, (e.clientX - img.left) / img.width)),
      y: Math.min(1, Math.max(0, (e.clientY - img.top) / img.height)),
    };
  };
  wrap.addEventListener("mousedown", (e) => {
    if (running || !$("preview").naturalWidth) return;
    start = toFrac(e);
  });
  window.addEventListener("mousemove", (e) => {
    if (!start) return;
    const p = toFrac(e);
    region = {
      x: Math.min(start.x, p.x), y: Math.min(start.y, p.y),
      w: Math.abs(p.x - start.x), h: Math.abs(p.y - start.y),
    };
    drawRegion();
  });
  window.addEventListener("mouseup", () => {
    if (!start) return;
    start = null;
    if (region && (region.w < 0.03 || region.h < 0.03)) region = null; // przypadkowe kliknięcie
    drawRegion();
    saveSettings();
  });
  $("preview").addEventListener("load", drawRegion);
  window.addEventListener("resize", drawRegion);
}

// ------------------------------------------------------------ slajdy

function addSlide(slide) {
  const li = document.createElement("li");
  li.dataset.id = slide.id;
  li.innerHTML = `
    <img src="${slide.thumb}" alt="" />
    <span class="num"></span>
    <div class="tools">
      <button data-act="left" title="Przesuń w lewo">◀</button>
      <button data-act="right" title="Przesuń w prawo">▶</button>
      <button data-act="del" title="Usuń">✕</button>
    </div>`;
  $("slide-list").appendChild(li);
  li.scrollIntoView({ behavior: "smooth", inline: "end" });
  renumber();
}

function renumber() {
  const items = [...$("slide-list").children];
  items.forEach((li, i) => (li.querySelector(".num").textContent = i + 1));
  $("count").textContent = items.length;
}

$("slide-list").addEventListener("click", async (e) => {
  const btn = e.target.closest("button");
  if (!btn) return;
  const li = btn.closest("li");
  const id = Number(li.dataset.id);
  if (btn.dataset.act === "del") {
    await invoke("delete_slide", { id });
    li.remove();
  } else if (btn.dataset.act === "left" && li.previousElementSibling) {
    await invoke("move_slide", { id, delta: -1 });
    li.parentNode.insertBefore(li, li.previousElementSibling);
  } else if (btn.dataset.act === "right" && li.nextElementSibling) {
    await invoke("move_slide", { id, delta: 1 });
    li.parentNode.insertBefore(li.nextElementSibling, li);
  }
  renumber();
});

// ------------------------------------------------------------ start / stop

function setStatus(state, text) {
  $("status").className = `status ${state}`;
  $("status").textContent = text;
}

function setRunning(on) {
  running = on;
  $("start").hidden = on;
  $("stop").hidden = !on;
  for (const id of ["source", "refresh", "threshold", "settle", "interval", "skip-repeats", "new-session", "outdir-set"]) {
    $(id).disabled = on;
  }
}

$("start").addEventListener("click", async () => {
  const source = currentSource();
  if (!source) return;
  saveSettings();
  try {
    await invoke("start", {
      opts: {
        source,
        region,
        threshold_pct: Number($("threshold").value),
        settle_ms: Math.round(Number($("settle").value) * 1000),
        interval_ms: Number($("interval").value),
        skip_repeats: $("skip-repeats").checked,
      },
    });
    startedAt = Date.now();
    setRunning(true);
  } catch (e) {
    setStatus("error", String(e));
  }
});

$("stop").addEventListener("click", () => {
  if (Date.now() - startedAt < 800) return;
  invoke("stop");
});

$("snap").addEventListener("click", async () => {
  try {
    await invoke("snap", { source: currentSource(), region });
  } catch (e) {
    setStatus("error", String(e));
  }
});

async function doExport(format) {
  setStatus("", `Eksportuję ${format.toUpperCase()}…`);
  try {
    const path = await invoke("export", { format });
    setStatus("watching", `Zapisano: ${path}`);
  } catch (e) {
    setStatus("error", `Eksport nie wyszedł: ${e}`);
  }
}
$("export-pdf").addEventListener("click", () => doExport("pdf"));
$("export-pptx").addEventListener("click", () => doExport("pptx"));

$("new-session").addEventListener("click", async () => {
  try {
    await invoke("new_session");
    $("slide-list").innerHTML = "";
    renumber();
    setStatus("", "Nowa sesja, slajdy trafią do nowego folderu");
  } catch (e) {
    setStatus("error", String(e));
  }
});

$("open-folder").addEventListener("click", () => invoke("open_folder").catch((e) => setStatus("error", String(e))));
$("outdir-set").addEventListener("click", async () => {
  try {
    await invoke("set_output_dir", { dir: $("outdir").value.trim() });
    setStatus("", "Folder zapisu ustawiony");
  } catch (e) {
    setStatus("error", String(e));
  }
});

$("source").addEventListener("change", () => { region = null; drawRegion(); saveSettings(); loadPreview(); });
$("refresh").addEventListener("click", () => loadSources(JSON.stringify(currentSource())));
$("reload-preview").addEventListener("click", loadPreview);
$("clear-region").addEventListener("click", () => { region = null; drawRegion(); saveSettings(); });
for (const id of ["threshold", "settle", "interval"]) $(id).addEventListener("input", () => { renderLabels(); saveSettings(); });
$("skip-repeats").addEventListener("change", saveSettings);

$("perm-btn").addEventListener("click", async () => {
  const s = await invoke("request_screen_permission");
  if (s === "granted") $("perm-banner").hidden = true;
});

// ------------------------------------------------------------ zdarzenia z backendu

listen("slide", (e) => addSlide(e.payload));
listen("status", (e) => {
  const { state, message } = e.payload;
  setStatus(state, message);
  if (state === "stopped" || state === "error") setRunning(false);
});

// ------------------------------------------------------------ start aplikacji

(async () => {
  const saved = loadSettings();
  renderLabels();
  setupRegionSelection();
  $("outdir").value = await invoke("get_output_dir");
  const perm = await invoke("screen_permission");
  $("perm-banner").hidden = perm !== "denied";
  await loadSources(saved.sourceKey);
  if (saved.region && saved.sourceKey === JSON.stringify(currentSource())) {
    region = saved.region;
    drawRegion();
  }
  for (const s of await invoke("slides")) addSlide(s);
})();
