mod detect;
mod export;
mod permission;

use base64::Engine;
use detect::{Detector, DetectorConfig, Fingerprint, Verdict};
use image::{codecs::jpeg::JpegEncoder, imageops, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- źródła obrazu

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    Monitor { id: u32 },
    Window { id: u32 },
}

#[derive(Serialize)]
struct SourceInfo {
    source: Source,
    label: String,
    width: u32,
    height: u32,
}

#[tauri::command]
fn list_sources() -> Res<Vec<SourceInfo>> {
    let mut out = Vec::new();
    for (i, m) in xcap::Monitor::all().map_err(err)?.into_iter().enumerate() {
        let name = m.friendly_name().or_else(|_| m.name()).unwrap_or_default();
        let primary = if m.is_primary().unwrap_or(false) {
            ", główny"
        } else {
            ""
        };
        let (w, h) = (m.width().unwrap_or(0), m.height().unwrap_or(0));
        out.push(SourceInfo {
            source: Source::Monitor {
                id: m.id().map_err(err)?,
            },
            label: format!("Monitor {}: {} ({}×{}{})", i + 1, name, w, h, primary),
            width: w,
            height: h,
        });
    }
    if let Ok(windows) = xcap::Window::all() {
        for w in windows {
            if w.is_minimized().unwrap_or(false) {
                continue;
            }
            let (ww, wh) = (w.width().unwrap_or(0), w.height().unwrap_or(0));
            if ww < 300 || wh < 200 {
                continue;
            }
            let app = w.app_name().unwrap_or_default();
            let title = w.title().unwrap_or_default();
            if app.is_empty() && title.is_empty() {
                continue;
            }
            out.push(SourceInfo {
                source: Source::Window {
                    id: w.id().map_err(err)?,
                },
                label: format!("Okno: {} — {}", app, title),
                width: ww,
                height: wh,
            });
        }
    }
    Ok(out)
}

/// Uchwyt do źródła trzymany w wątku przechwytywania (obiekty xcap nie zawsze są `Send`).
enum Grabber {
    Monitor(xcap::Monitor),
    Window(xcap::Window),
}

impl Grabber {
    fn open(source: &Source) -> Res<Self> {
        match source {
            Source::Monitor { id } => xcap::Monitor::all()
                .map_err(err)?
                .into_iter()
                .find(|m| m.id().ok() == Some(*id))
                .map(Grabber::Monitor)
                .ok_or_else(|| "Nie znaleziono monitora (odłączony?)".into()),
            Source::Window { id } => xcap::Window::all()
                .map_err(err)?
                .into_iter()
                .find(|w| w.id().ok() == Some(*id))
                .map(Grabber::Window)
                .ok_or_else(|| "Nie znaleziono okna (zamknięte?)".into()),
        }
    }

    fn grab(&self) -> Res<RgbaImage> {
        match self {
            Grabber::Monitor(m) => m.capture_image().map_err(err),
            Grabber::Window(w) => w.capture_image().map_err(err),
        }
    }
}

/// Obszar w ułamkach (0-1) obrazu źródła, np. sam udostępniony slajd bez kafelków z kamerami.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Region {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn crop(img: RgbaImage, region: Option<Region>) -> RgbaImage {
    let Some(r) = region else { return img };
    let (iw, ih) = img.dimensions();
    let x = ((r.x.clamp(0.0, 1.0)) * iw as f32) as u32;
    let y = ((r.y.clamp(0.0, 1.0)) * ih as f32) as u32;
    let w = ((r.w * iw as f32) as u32).clamp(1, iw - x.min(iw - 1));
    let h = ((r.h * ih as f32) as u32).clamp(1, ih - y.min(ih - 1));
    imageops::crop_imm(&img, x, y, w, h).to_image()
}

fn jpeg_base64(img: &RgbaImage, max_w: u32) -> Res<String> {
    let (w, h) = img.dimensions();
    let small = if w > max_w {
        imageops::resize(img, max_w, h * max_w / w, imageops::FilterType::Triangle)
    } else {
        img.clone()
    };
    let rgb = image::DynamicImage::ImageRgba8(small).to_rgb8();
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, 80)
        .encode_image(&rgb)
        .map_err(err)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf)
    ))
}

#[tauri::command]
async fn preview(source: Source) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let img = Grabber::open(&source)?.grab()?;
        jpeg_base64(&img, 1280)
    })
    .await
    .map_err(err)?
}

// ---------------------------------------------------------------- sesja

#[derive(Serialize, Clone)]
struct Slide {
    id: u64,
    path: PathBuf,
    thumb: String,
}

#[derive(Default)]
struct Session {
    dir: Option<PathBuf>,
    slides: Vec<Slide>,
    next_id: u64,
}

impl Session {
    fn ensure_dir(&mut self, base: &PathBuf) -> Res<PathBuf> {
        if let Some(d) = &self.dir {
            return Ok(d.clone());
        }
        let d = base.join(chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string());
        std::fs::create_dir_all(&d).map_err(err)?;
        self.dir = Some(d.clone());
        Ok(d)
    }

    fn save(&mut self, base: &PathBuf, img: &RgbaImage) -> Res<Slide> {
        let dir = self.ensure_dir(base)?;
        self.next_id += 1;
        let path = dir.join(format!("slajd_{:03}.png", self.next_id));
        img.save(&path).map_err(err)?;
        let slide = Slide {
            id: self.next_id,
            path,
            thumb: jpeg_base64(img, 360)?,
        };
        self.slides.push(slide.clone());
        Ok(slide)
    }
}

struct AppState {
    session: Arc<Mutex<Session>>,
    running: Arc<AtomicBool>,
    /// prośba o ręczny zrzut, obsługiwana przez wątek przechwytywania
    snap_now: Arc<AtomicBool>,
    base_dir: Mutex<PathBuf>,
}

#[derive(Deserialize, Clone)]
struct StartOptions {
    source: Source,
    region: Option<Region>,
    /// próg zmiany w procentach pikseli
    threshold_pct: f32,
    settle_ms: u64,
    interval_ms: u64,
    skip_repeats: bool,
}

#[derive(Serialize, Clone)]
struct Status {
    state: &'static str,
    message: String,
}

fn status(app: &AppHandle, state: &'static str, message: impl Into<String>) {
    let _ = app.emit(
        "status",
        Status {
            state,
            message: message.into(),
        },
    );
}

#[tauri::command]
fn start(app: AppHandle, st: State<AppState>, opts: StartOptions) -> Res<()> {
    if st.running.swap(true, Ordering::SeqCst) {
        return Err("Przechwytywanie już działa".into());
    }
    let running = st.running.clone();
    let snap_now = st.snap_now.clone();
    let session = st.session.clone();
    let base = st.base_dir.lock().unwrap().clone();

    std::thread::spawn(move || {
        let result = capture_loop(&app, &opts, &running, &snap_now, &session, &base);
        running.store(false, Ordering::SeqCst);
        match result {
            Ok(()) => status(&app, "stopped", "Zatrzymano"),
            Err(e) => status(&app, "error", e),
        }
    });
    Ok(())
}

fn capture_loop(
    app: &AppHandle,
    opts: &StartOptions,
    running: &AtomicBool,
    snap_now: &AtomicBool,
    session: &Mutex<Session>,
    base: &PathBuf,
) -> Res<()> {
    let mut grabber = Grabber::open(&opts.source)?;
    let mut detector = Detector::new(DetectorConfig {
        threshold: opts.threshold_pct / 100.0,
        settle: Duration::from_millis(opts.settle_ms),
        skip_repeats: opts.skip_repeats,
    });
    let interval = Duration::from_millis(opts.interval_ms.max(150));
    let mut failures = 0;
    status(app, "watching", "Obserwuję ekran…");

    while running.load(Ordering::SeqCst) {
        let tick = std::time::Instant::now();
        let img = match grabber.grab() {
            Ok(img) => {
                failures = 0;
                crop(img, opts.region)
            }
            Err(e) => {
                failures += 1;
                if failures > 5 {
                    return Err(format!("Nie mogę przechwycić obrazu: {e}"));
                }
                // okno mogło zmienić identyfikator lub monitor się przełączyć
                if let Ok(g) = Grabber::open(&opts.source) {
                    grabber = g;
                }
                std::thread::sleep(interval);
                continue;
            }
        };
        let fp = Fingerprint::of(&img);

        if snap_now.swap(false, Ordering::SeqCst) {
            detector.mark_saved(fp.clone());
            save_and_emit(app, session, base, &img)?;
        } else {
            match detector.feed(fp) {
                Verdict::Capture => save_and_emit(app, session, base, &img)?,
                Verdict::Moving => status(
                    app,
                    "moving",
                    "Obraz się zmienia, czekam aż się ustabilizuje…",
                ),
                Verdict::Repeat => status(app, "watching", "Ten slajd już był, pomijam"),
                Verdict::Idle => {}
            }
        }
        if let Some(rest) = interval.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}

fn save_and_emit(
    app: &AppHandle,
    session: &Mutex<Session>,
    base: &PathBuf,
    img: &RgbaImage,
) -> Res<()> {
    let slide = session.lock().unwrap().save(base, img)?;
    let count = session.lock().unwrap().slides.len();
    let _ = app.emit("slide", slide);
    status(app, "watching", format!("Zapisano slajd ({count})"));
    Ok(())
}

#[tauri::command]
fn stop(st: State<AppState>) {
    st.running.store(false, Ordering::SeqCst);
}

/// Ręczny zrzut: gdy przechwytywanie działa, robi go wątek przechwytywania (z tym samym regionem);
/// w przeciwnym razie zrzut jest robiony od razu.
#[tauri::command]
async fn snap(
    app: AppHandle,
    st: State<'_, AppState>,
    source: Source,
    region: Option<Region>,
) -> Res<()> {
    if st.running.load(Ordering::SeqCst) {
        st.snap_now.store(true, Ordering::SeqCst);
        return Ok(());
    }
    let session = st.session.clone();
    let base = st.base_dir.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let img = crop(Grabber::open(&source)?.grab()?, region);
        save_and_emit(&app, &session, &base, &img)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn slides(st: State<AppState>) -> Vec<Slide> {
    st.session.lock().unwrap().slides.clone()
}

#[tauri::command]
fn delete_slide(st: State<AppState>, id: u64) -> Res<()> {
    let mut s = st.session.lock().unwrap();
    if let Some(pos) = s.slides.iter().position(|x| x.id == id) {
        let slide = s.slides.remove(pos);
        let _ = std::fs::remove_file(slide.path);
    }
    Ok(())
}

#[tauri::command]
fn move_slide(st: State<AppState>, id: u64, delta: i32) -> Res<()> {
    let mut s = st.session.lock().unwrap();
    if let Some(pos) = s.slides.iter().position(|x| x.id == id) {
        let to = (pos as i32 + delta).clamp(0, s.slides.len() as i32 - 1) as usize;
        let slide = s.slides.remove(pos);
        s.slides.insert(to, slide);
    }
    Ok(())
}

/// Zaczyna nową sesję (nowy folder). Pliki poprzedniej zostają na dysku.
#[tauri::command]
fn new_session(st: State<AppState>) -> Res<()> {
    if st.running.load(Ordering::SeqCst) {
        return Err("Najpierw zatrzymaj przechwytywanie".into());
    }
    *st.session.lock().unwrap() = Session::default();
    Ok(())
}

#[tauri::command]
fn get_output_dir(st: State<AppState>) -> String {
    st.base_dir.lock().unwrap().display().to_string()
}

#[tauri::command]
fn set_output_dir(st: State<AppState>, dir: String) -> Res<()> {
    let p = PathBuf::from(dir);
    std::fs::create_dir_all(&p).map_err(err)?;
    *st.base_dir.lock().unwrap() = p;
    st.session.lock().unwrap().dir = None;
    Ok(())
}

#[tauri::command]
async fn export(st: State<'_, AppState>, format: String) -> Res<String> {
    let (paths, dir) = {
        let mut s = st.session.lock().unwrap();
        let base = st.base_dir.lock().unwrap().clone();
        let dir = s.ensure_dir(&base)?;
        (
            s.slides.iter().map(|x| x.path.clone()).collect::<Vec<_>>(),
            dir,
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        let out = dir.join(format!("prezentacja.{format}"));
        match format.as_str() {
            "pdf" => export::to_pdf(&paths, &out)?,
            "pptx" => export::to_pptx(&paths, &out)?,
            _ => return Err("Nieznany format".into()),
        }
        let _ = tauri_plugin_opener::reveal_item_in_dir(&out);
        Ok(out.display().to_string())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn open_folder(st: State<AppState>) -> Res<()> {
    let dir = st
        .session
        .lock()
        .unwrap()
        .dir
        .clone()
        .unwrap_or_else(|| st.base_dir.lock().unwrap().clone());
    std::fs::create_dir_all(&dir).map_err(err)?;
    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(err)
}

#[tauri::command]
fn screen_permission() -> &'static str {
    permission::status()
}

#[tauri::command]
fn request_screen_permission() -> &'static str {
    permission::request()
}

fn default_base_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("SlideCatcher")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            session: Arc::new(Mutex::new(Session::default())),
            running: Arc::new(AtomicBool::new(false)),
            snap_now: Arc::new(AtomicBool::new(false)),
            base_dir: Mutex::new(default_base_dir()),
        })
        .invoke_handler(tauri::generate_handler![
            list_sources,
            preview,
            start,
            stop,
            snap,
            slides,
            delete_slide,
            move_slide,
            new_session,
            get_output_dir,
            set_output_dir,
            export,
            open_folder,
            screen_permission,
            request_screen_permission,
        ])
        .run(tauri::generate_context!())
        .expect("błąd uruchamiania aplikacji");
}
