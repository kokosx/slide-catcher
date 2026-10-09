//! Wykrywanie zmiany slajdu.
//!
//! Każda klatka jest zmniejszana do małej miniatury w skali szarości ("odcisk").
//! Dwie klatki różnią się, gdy odpowiednio duży ułamek pikseli odcisku zmienił
//! jasność o więcej niż `PIXEL_DELTA`. Dzięki temu kursor myszy czy szum kompresji
//! wideo z Teams nie wyzwalają zrzutu, a zmiana slajdu tak.

use image::{imageops, GrayImage, RgbaImage};
use std::time::{Duration, Instant};

const FP_WIDTH: u32 = 160;
const FP_HEIGHT: u32 = 90;
/// Minimalna zmiana jasności piksela odcisku (0-255), żeby uznać go za zmieniony.
const PIXEL_DELTA: u8 = 28;

#[derive(Clone)]
pub struct Fingerprint(GrayImage);

impl Fingerprint {
    pub fn of(img: &RgbaImage) -> Self {
        let small = imageops::thumbnail(img, FP_WIDTH, FP_HEIGHT);
        let gray = imageops::grayscale(&small);
        // lekkie rozmycie tłumi artefakty kompresji i antyaliasing tekstu
        Fingerprint(imageops::blur(&gray, 0.6))
    }

    /// Ułamek pikseli (0.0-1.0), które różnią się między odciskami.
    pub fn diff(&self, other: &Fingerprint) -> f32 {
        let (a, b) = (&self.0, &other.0);
        if a.dimensions() != b.dimensions() {
            return 1.0;
        }
        let changed = a
            .as_raw()
            .iter()
            .zip(b.as_raw())
            .filter(|(x, y)| x.abs_diff(**y) > PIXEL_DELTA)
            .count();
        changed as f32 / a.as_raw().len() as f32
    }
}

#[derive(Clone, Copy)]
pub struct DetectorConfig {
    /// Próg zmiany (ułamek pikseli), powyżej którego obraz uznajemy za inny.
    pub threshold: f32,
    /// Ile czasu obraz musi stać nieruchomo, zanim zrobimy zrzut (animacje, przejścia).
    pub settle: Duration,
    /// Pomijaj slajdy identyczne z którymkolwiek wcześniej zapisanym (np. powrót do slajdu).
    pub skip_repeats: bool,
}

pub enum Verdict {
    /// Nic się nie dzieje.
    Idle,
    /// Obraz się zmienia, czekamy aż się ustabilizuje.
    Moving,
    /// Nowy, stabilny slajd: zapisz go.
    Capture,
    /// Stabilny obraz, ale taki slajd już mamy.
    Repeat,
}

pub struct Detector {
    cfg: DetectorConfig,
    prev: Option<Fingerprint>,
    stable_since: Instant,
    last_saved: Option<Fingerprint>,
    saved: Vec<Fingerprint>,
}

impl Detector {
    pub fn new(cfg: DetectorConfig) -> Self {
        Detector {
            cfg,
            prev: None,
            stable_since: Instant::now(),
            last_saved: None,
            saved: Vec::new(),
        }
    }

    pub fn feed(&mut self, fp: Fingerprint) -> Verdict {
        let now = Instant::now();
        let moving = match &self.prev {
            Some(prev) => prev.diff(&fp) > self.cfg.threshold,
            None => true,
        };
        self.prev = Some(fp.clone());
        if moving {
            self.stable_since = now;
            return Verdict::Moving;
        }
        if now.duration_since(self.stable_since) < self.cfg.settle {
            return Verdict::Moving;
        }
        let is_new = match &self.last_saved {
            Some(last) => last.diff(&fp) > self.cfg.threshold,
            None => true,
        };
        if !is_new {
            return Verdict::Idle;
        }
        // zapamiętujemy jako "ostatni", żeby nie zgłaszać tego samego obrazu w kółko
        self.last_saved = Some(fp.clone());
        if self.cfg.skip_repeats && self.saved.iter().any(|s| s.diff(&fp) <= self.cfg.threshold) {
            return Verdict::Repeat;
        }
        self.saved.push(fp);
        Verdict::Capture
    }

    /// Ręczny zrzut: dopisz odcisk, żeby detektor nie złapał tego samego slajdu ponownie.
    pub fn mark_saved(&mut self, fp: Fingerprint) {
        self.last_saved = Some(fp.clone());
        self.saved.push(fp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn slide(color: u8, box_x: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(1920, 1080, Rgba([color, color, color, 255]));
        for y in 200..600 {
            for x in box_x..box_x + 600 {
                img.put_pixel(x, y, Rgba([20, 40, 200, 255]));
            }
        }
        img
    }

    fn cfg() -> DetectorConfig {
        DetectorConfig {
            threshold: 0.01,
            settle: Duration::ZERO,
            skip_repeats: true,
        }
    }

    #[test]
    fn cursor_does_not_count_as_change() {
        let a = slide(240, 100);
        let mut b = a.clone();
        for y in 500..520 {
            for x in 900..912 {
                b.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        assert!(Fingerprint::of(&a).diff(&Fingerprint::of(&b)) < 0.01);
    }

    #[test]
    fn new_slide_counts_as_change() {
        let a = Fingerprint::of(&slide(240, 100));
        let b = Fingerprint::of(&slide(240, 1100));
        assert!(a.diff(&b) > 0.05);
    }

    #[test]
    fn captures_once_per_slide_and_skips_repeats() {
        let mut d = Detector::new(cfg());
        let s1 = Fingerprint::of(&slide(240, 100));
        let s2 = Fingerprint::of(&slide(240, 1100));
        assert!(matches!(d.feed(s1.clone()), Verdict::Moving)); // pierwsza klatka
        assert!(matches!(d.feed(s1.clone()), Verdict::Capture));
        assert!(matches!(d.feed(s1.clone()), Verdict::Idle));
        assert!(matches!(d.feed(s2.clone()), Verdict::Moving));
        assert!(matches!(d.feed(s2.clone()), Verdict::Capture));
        assert!(matches!(d.feed(s1.clone()), Verdict::Moving));
        assert!(matches!(d.feed(s1.clone()), Verdict::Repeat));
        assert!(matches!(d.feed(s1), Verdict::Idle));
    }
}
