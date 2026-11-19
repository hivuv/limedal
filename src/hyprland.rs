//! Hyprland uyumluluğu: bileşen algılama, monitör çözünürlüğü ve önerilen ayarlar.
//!
//! Hyprland saf bir Wayland bileşenidir; LiMedal penceresi wgpu/winit üzerinden
//! Wayland'de yerel olarak açılır. Ekran yakalama tarafında XWayland pencereleri
//! X11 üzerinden yakalanır, bu yüzden XWayland'in gerçek kök boyutunu bilmek
//! kritiktir — `xrandr` Wayland'de çoğu zaman yoktur.

#![allow(dead_code)]

use std::process::{Command, Stdio};

use serde_json::Value;

/// Hyprland oturumunda mıyız?
pub fn detected() -> bool {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return true;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop.to_lowercase().contains("hyprland")
}

/// `hyprctl` çalışıyor mu (Hyprland ayakta mı)?
pub fn running() -> bool {
    if !detected() {
        return false;
    }
    hyprctl(&["version"]).is_some()
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let output = Command::new("hyprctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn hyprctl_json(args: &[&str]) -> Option<Value> {
    let text = hyprctl(args)?;
    serde_json::from_str(text.trim()).ok()
}

pub fn version() -> Option<String> {
    let text = hyprctl(&["version"])?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Hyprland ") {
            return Some(rest.trim().to_string());
        }
        if line.to_lowercase().starts_with("tag ") {
            return Some(line.trim().to_string());
        }
    }
    text.lines().next().map(|line| line.trim().to_string())
}

fn even(value: f64) -> u32 {
    let rounded = value.round().max(0.0) as u32;
    rounded - (rounded % 2)
}

/// Odaklı monitörün (yoksa ilkinin) piksel çözünürlüğü.
///
/// `scale` 1'den büyükse XWayland kök penceresi mantıksal boyuttadır; bu yüzden
/// ölçek bölünür. Sonuç her zaman çift sayıya yuvarlanır (H.264 kısıtı).
pub fn monitor_resolution() -> Option<(u32, u32)> {
    let monitors = hyprctl_json(&["monitors"])?;
    let list = monitors.as_array()?;
    let mut fallback: Option<(u32, u32)> = None;
    for monitor in list {
        let width = monitor.get("width").and_then(|value| value.as_f64());
        let height = monitor.get("height").and_then(|value| value.as_f64());
        let scale = monitor
            .get("scale")
            .and_then(|value| value.as_f64())
            .filter(|value| *value > 0.1)
            .unwrap_or(1.0);
        let (width, height) = match (width, height) {
            (Some(width), Some(height)) => (width, height),
            _ => continue,
        };
        let logical_width = even(width / scale);
        let logical_height = even(height / scale);
        if logical_width < 320 || logical_height < 240 {
            continue;
        }
        let focused = monitor
            .get("focused")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let entry = (logical_width, logical_height);
        if focused {
            return Some(entry);
        }
        if fallback.is_none() {
            fallback = Some(entry);
        }
    }
    fallback
}

/// Odaklı pencere: (sınıf, başlık, xwayland_mi)
pub fn active_window() -> Option<(String, String, bool)> {
    let value = hyprctl_json(&["activewindow"])?;
    let class = value
        .get("class")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let title = value
        .get("title")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let xwayland = value
        .get("xwayland")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    if class.is_empty() && title.is_empty() {
        return None;
    }
    Some((class, title, xwayland))
}

/// `limedal hyprland` çıktısı: kopyalanabilir yapılandırma önerisi.
pub fn config_snippet() -> String {
    let mut text = String::new();
    text.push_str("# ~/.config/hypr/hyprland.conf  (LiMedal önerilen ayarlar)\n");
    text.push_str("#\n");
    text.push_str("# LiMedal penceresini sabit boyutta, gölgesiz ve animasyonsuz aç.\n");
    text.push_str("windowrulev2 = float, class:^(limedal)$\n");
    text.push_str("windowrulev2 = center, class:^(limedal)$\n");
    text.push_str("windowrulev2 = size 1180 780, class:^(limedal)$\n");
    text.push_str("windowrulev2 = noblur, class:^(limedal)$\n");
    text.push_str("windowrulev2 = noshadow, class:^(limedal)$\n");
    text.push_str("\n");
    text.push_str("# Global kısayollar: /dev/input dinleyicisine gerek kalmaz.\n");
    text.push_str("bind = , F9, exec, limedal clip\n");
    text.push_str("bind = , F10, exec, limedal record toggle\n");
    text.push_str("bind = SUPER SHIFT, L, exec, limedal open\n");
    text.push_str("\n");
    text.push_str("# Arka planda klip motoru (istersen systemd yerine):\n");
    text.push_str("# exec-once = limedal run\n");
    text.push_str("\n");
    text.push_str("# Not: Saf Wayland oyunları (XWayland kullanmayan) X11 ekran\n");
    text.push_str("# yakalama ile alınamaz. Proton/Steam oyunları XWayland kullanır.\n");
    text
}

/// Tanı satırı için kısa özet.
pub fn summary() -> String {
    if !detected() {
        return "yok".to_string();
    }
    if !running() {
        return "algılandı ama hyprctl yanıt vermiyor".to_string();
    }
    let version = version().unwrap_or_else(|| "sürüm bilinmiyor".to_string());
    match monitor_resolution() {
        Some((width, height)) => format!("{} · {}x{}", version, width, height),
        None => version,
    }
}
