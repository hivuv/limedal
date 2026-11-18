//! Ayarlar: ~/.config/limedal/config.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::util::{config_path, expand};

pub const QUALITIES: [&str; 4] = ["low", "medium", "high", "ultra"];
pub const ENCODERS: [&str; 5] = ["auto", "h264_nvenc", "h264_vaapi", "h264_qsv", "libx264"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub game_source: String,
    pub mic_source: String,
    pub game_volume: f32,
    pub mic_volume: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            game_source: "auto".to_string(),
            mic_source: String::new(),
            game_volume: 1.0,
            mic_volume: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: u32,
    pub display: String,
    pub quality: String,
    pub encoder: String,
    pub buffer_seconds: u32,
    pub segment_seconds: u32,
    pub draw_cursor: bool,
    pub audio: AudioConfig,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        CaptureConfig {
            width: None,
            height: None,
            fps: 60,
            display: String::new(),
            quality: "high".to_string(),
            encoder: "auto".to_string(),
            buffer_seconds: 60,
            segment_seconds: 2,
            draw_cursor: true,
            audio: AudioConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub enabled: bool,
    pub save_clip: String,
    pub toggle_record: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        HotkeyConfig {
            enabled: true,
            save_clip: "F9".to_string(),
            toggle_record: "F10".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub library_dir: String,
    pub auto_start_capture: bool,
    pub game_detection: bool,
    pub notifications: bool,
    pub max_storage_gb: u32,
    pub clip_seconds_default: u32,
    pub share_port: u16,
    pub share_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            library_dir: "~/Videos/LiMedal".to_string(),
            auto_start_capture: true,
            game_detection: true,
            notifications: true,
            max_storage_gb: 0,
            clip_seconds_default: 30,
            share_port: 8771,
            share_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub hotkeys: HotkeyConfig,
    pub app: AppConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            capture: CaptureConfig::default(),
            hotkeys: HotkeyConfig::default(),
            app: AppConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        Config::load_from(&config_path())
    }

    pub fn load_from(path: &PathBuf) -> Config {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => return Config::default(),
        };
        let mut config: Config = serde_json::from_str(&text).unwrap_or_default();
        config.sanitize();
        config
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match serde_json::to_string_pretty(self) {
            Ok(text) => text,
            Err(_) => return,
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    pub fn sanitize(&mut self) {
        let cap = &mut self.capture;
        cap.fps = cap.fps.clamp(15, 240);
        cap.buffer_seconds = cap.buffer_seconds.clamp(5, 1800);
        cap.segment_seconds = cap.segment_seconds.clamp(1, 10);
        if !QUALITIES.contains(&cap.quality.as_str()) {
            cap.quality = "high".to_string();
        }
        if !ENCODERS.contains(&cap.encoder.as_str()) {
            cap.encoder = "auto".to_string();
        }
        cap.width = cap.width.map(|v| v.clamp(320, 7680).max(320));
        cap.height = cap.height.map(|v| v.clamp(240, 4320).max(240));
        cap.audio.game_volume = clamp_f32(cap.audio.game_volume, 0.0, 3.0);
        cap.audio.mic_volume = clamp_f32(cap.audio.mic_volume, 0.0, 3.0);
        if cap.audio.game_source.trim().is_empty() {
            cap.audio.game_source = "auto".to_string();
        }

        let hot = &mut self.hotkeys;
        hot.save_clip = hot.save_clip.trim().to_string();
        hot.toggle_record = hot.toggle_record.trim().to_string();
        if hot.save_clip.is_empty() {
            hot.save_clip = "F9".to_string();
        }

        let app = &mut self.app;
        app.clip_seconds_default = app.clip_seconds_default.clamp(5, 1800);
        if app.share_port < 1024 {
            app.share_port = 8771;
        }
        if app.library_dir.trim().is_empty() {
            app.library_dir = "~/Videos/LiMedal".to_string();
        }
    }

    pub fn library_dir(&self) -> PathBuf {
        let path = expand(&self.app.library_dir);
        if std::fs::create_dir_all(&path).is_err() {
            let fallback = expand("~/Videos/LiMedal");
            let _ = std::fs::create_dir_all(&fallback);
            return fallback;
        }
        path
    }

    /// Etkin X görüntüsü: ortamda DISPLAY varsa o, yoksa ayardaki değer.
    pub fn display(&self) -> String {
        let ambient = std::env::var("DISPLAY").unwrap_or_default();
        let raw = if ambient.trim().is_empty() {
            self.capture.display.clone()
        } else {
            ambient
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return ":0.0".to_string();
        }
        match trimmed.split('+').next() {
            Some(head) if !head.is_empty() => head.to_string(),
            _ => ":0.0".to_string(),
        }
    }
}

fn clamp_f32(value: f32, low: f32, high: f32) -> f32 {
    if !value.is_finite() {
        return low;
    }
    value.max(low).min(high)
}
