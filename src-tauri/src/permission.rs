//! Uprawnienie do nagrywania ekranu. Wymagane tylko na macOS (Prywatność i ochrona → Nagrywanie ekranu).
//! Bez niego macOS zwraca zrzuty z samą tapetą zamiast zawartości okien.

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
pub fn status() -> &'static str {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        "granted"
    } else {
        "denied"
    }
}

/// Pokazuje systemowe okno z prośbą o zgodę (tylko za pierwszym razem) i otwiera właściwy panel Ustawień.
#[cfg(target_os = "macos")]
pub fn request() -> &'static str {
    if unsafe { CGRequestScreenCaptureAccess() } {
        return "granted";
    }
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn();
    "denied"
}

/// Usuwa zapamiętaną (np. nieaktualną po aktualizacji aplikacji) decyzję o nagrywaniu ekranu
/// i od razu prosi o nią ponownie. Pomaga, gdy w Ustawieniach przełącznik jest włączony,
/// a macOS i tak odmawia, bo wpis dotyczy starszej kopii aplikacji.
#[cfg(target_os = "macos")]
pub fn reset(bundle_id: &str) -> &'static str {
    let _ = std::process::Command::new("tccutil")
        .args(["reset", "ScreenCapture", bundle_id])
        .status();
    request()
}

#[cfg(not(target_os = "macos"))]
pub fn reset(_bundle_id: &str) -> &'static str {
    "not-needed"
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> &'static str {
    "not-needed"
}

#[cfg(not(target_os = "macos"))]
pub fn request() -> &'static str {
    "not-needed"
}
