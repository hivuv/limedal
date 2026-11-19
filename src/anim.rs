//! Küçük animasyon yardımcıları: yumuşatma eğrileri ve renk geçişleri.

#![allow(dead_code)]

use eframe::egui;

/// Sert başlayıp yavaşlayan giriş (0..1).
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

pub fn ease_out_quint(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    1.0 - inv * inv * inv * inv * inv
}

/// Hafif geri taşarak yerine oturan giriş; splash ve kartlar için.
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let p = t - 1.0;
    1.0 + C3 * p * p * p + C1 * p * p
}

pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let p = -2.0 * t + 2.0;
        1.0 - p * p * p / 2.0
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        let value = x as f32 + (y as f32 - x as f32) * t;
        value.round().clamp(0.0, 255.0) as u8
    };
    egui::Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Alfa çarpanı uygular (saydamlık animasyonları için).
pub fn with_alpha(color: egui::Color32, factor: f32) -> egui::Color32 {
    let alpha = (color.a() as f32 * factor.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Kare hızından bağımsız yumuşatma: `speed` büyüdükçe hedefe daha hızlı yaklaşır.
pub fn approach(current: f32, target: f32, dt: f32, speed: f32) -> f32 {
    let factor = 1.0 - (-speed.max(0.001) * dt.max(0.0)).exp();
    current + (target - current) * factor.clamp(0.0, 1.0)
}

/// `elapsed` / `duration` oranı, 0..1.
pub fn progress(elapsed: f32, duration: f32) -> f32 {
    if duration <= 0.0 {
        return 1.0;
    }
    (elapsed / duration).clamp(0.0, 1.0)
}

/// Sıralı giriş: her öğe `spread` kadar gecikmeyle başlar.
pub fn stagger(index: usize, overall: f32, spread: f32) -> f32 {
    let delay = (index as f32 * spread).min(0.45);
    let span = (1.0 - delay).max(0.05);
    ((overall - delay) / span).clamp(0.0, 1.0)
}

/// Nabız (kayıt göstergesi gibi): 0..1 arası yumuşak salınım.
pub fn pulse(time: f64, period: f64) -> f32 {
    if period <= 0.0 {
        return 1.0;
    }
    let phase = (time % period) / period;
    let wave = (phase * std::f64::consts::TAU).sin() as f32;
    (wave * 0.5 + 0.5).clamp(0.0, 1.0)
}
