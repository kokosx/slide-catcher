# SlideCatcher

Patrzy na wybrany monitor (albo okno), np. ten z Teams, i sam robi zrzut za każdym razem, gdy zmieni się slajd. Na koniec jednym kliknięciem składa wszystko w **PDF** albo **PPTX**.

Działa na macOS (priorytet), Windows i Linuksie. Tauri 2 + Rust, bez żadnych zewnętrznych programów do eksportu.

## Jak to działa

1. Wybierasz monitor lub okno. Na podglądzie przeciągasz myszą prostokąt wokół samego slajdu, żeby kafelki z kamerami i czat Teams nie wywoływały fałszywych zrzutów.
2. **Start.** Co pół sekundy (ustawialne) program robi klatkę, zmniejsza ją do 160×90 w skali szarości i porównuje z poprzednią.
3. Jeśli zmienił się więcej niż ustawiony procent pikseli (domyślnie 1%), uznaje, że coś się dzieje, i **czeka, aż obraz przestanie się ruszać** (domyślnie 1,2 s). Dzięki temu animacje i przejścia nie dają kilku zrzutów jednego slajdu, a kursor myszy w ogóle się nie liczy.
4. Stabilny obraz, który różni się od ostatnio zapisanego slajdu, ląduje jako PNG w pełnej rozdzielczości w `~/Pictures/SlideCatcher/<data_godzina>/`.
5. Opcja „Pomijaj slajdy, które już były” odrzuca slajd identyczny z którymkolwiek wcześniejszym (prowadzący cofnął się o slajd).
6. Na dole widzisz miniatury: możesz usunąć zbędne i zmienić kolejność. **Eksport PDF / PPTX** zapisuje `prezentacja.pdf` / `prezentacja.pptx` w folderze sesji i pokazuje plik w Finderze.

Przycisk **Zrzut teraz** robi zrzut ręcznie, gdy coś umknęło.

## Pobranie gotowej aplikacji

Gotowe pliki są w zakładce **Releases**: `.dmg` na macOS (Apple Silicon i Intel) i `.exe` na Windows.

Aplikacja nie jest podpisana certyfikatem Apple, więc macOS przy pierwszym otwarciu ją zablokuje. Po przeciągnięciu do Aplikacji uruchom raz w Terminalu:

```bash
xattr -cr /Applications/SlideCatcher.app
```

Na Windows SmartScreen może pokazać ostrzeżenie: kliknij „Więcej informacji” → „Uruchom mimo to”.

## Budowanie ze źródeł

Potrzebne: [Rust](https://rustup.rs) i Node.js 18+.

```bash
npm install
npm run dev      # uruchomienie w trybie deweloperskim
npm run build    # gotowa aplikacja: src-tauri/target/release/bundle/
```

Na macOS `npm run build` tworzy `SlideCatcher.app` i `.dmg` w `src-tauri/target/release/bundle/macos` i `.../dmg`.

### macOS: uprawnienie „Nagrywanie ekranu”

Bez niego macOS zwraca zrzuty z samą tapetą. Przy pierwszym uruchomieniu aplikacja pokaże żółty pasek z przyciskiem **Nadaj uprawnienie**, który otwiera *Ustawienia systemowe → Prywatność i ochrona → Nagrywanie ekranu i dźwięku*. Włącz tam SlideCatcher (w trybie `npm run dev` będzie to raczej Terminal lub Twój edytor) i **uruchom aplikację ponownie**.

Jeśli mimo włączonego przełącznika aplikacja dalej pokazuje brak uprawnienia: zamknij SlideCatcher, w *Nagrywanie ekranu i dźwięku* usuń wpis SlideCatcher przyciskiem **−**, uruchom aplikację, nadaj uprawnienie od nowa i uruchom ją ponownie. Każda nowa wersja aplikacji (podpis ad-hoc) wymaga takiego ponownego nadania.

Wymaga macOS 12.3 lub nowszego.

### Linux

Pakiety do kompilacji (Ubuntu/Debian):

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libssl-dev \
  libxcb1-dev libxrandr-dev libdbus-1-dev libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev clang
```

Na Wayland przechwytywanie idzie przez systemowy portal, więc system może zapytać o zgodę.

### Windows

Wystarczy Rust (MSVC) i Node.js; WebView2 jest w Windows 10/11.

## Wskazówki

- Zaznacz region wokół samego slajdu. To najważniejsza rzecz dla jakości wykrywania.
- Gdy łapie za dużo (np. przewijany arkusz), podnieś próg albo wydłuż „Odczekaj po zmianie”. Gdy gubi slajdy różniące się jednym punktem, obniż próg.
- Okno SlideCatchera trzymaj na innym monitorze niż obserwowany, bo inaczej trafi na zrzuty.
- Obserwacja **okna** zamiast monitora działa nawet gdy okno Teams jest częściowo zasłonięte (macOS, Windows).

## Struktura

```
src/                  interfejs (HTML/CSS/JS, bez bundlera)
src-tauri/src/
  lib.rs              komendy, wątek przechwytywania, sesja
  detect.rs           wykrywanie zmiany slajdu (+ testy)
  export.rs           zapis PDF i PPTX
  permission.rs       uprawnienie Nagrywanie ekranu na macOS
```

Testy wykrywania i eksportu: `cd src-tauri && cargo test`.
