//! Uygulama çekirdeği: bileşenleri bir araya getirir.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::audio::AudioSources;
use crate::capture::{CaptureStatus, Engine};
use crate::config::Config;
use crate::ffmpeg::{encoder_label, Ffmpeg};
use crate::gamedetect::{Detector, Game};
use crate::hotkeys::{self, HotkeyState};
use crate::ipc;
use crate::library::{Clip, Library, Stats};
use crate::share::{self, ShareHandle};
use crate::util::{cache_dir, now, which, Logger};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ToastKind {
    Info,
    Ok,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub at: f64,
}

pub struct Core {
    pub cfg: Arc<Mutex<Config>>,
    pub log: Arc<Logger>,
    pub ff: Arc<Ffmpeg>,
    pub audio: Arc<AudioSources>,
    pub library: Arc<Library>,
    pub engine: Arc<Engine>,
    pub detector: Arc<Detector>,
    quit: AtomicBool,
    ctx: Mutex<Option<egui::Context>>,
    hotkeys: Mutex<HotkeyState>,
    toasts: Mutex<Vec<Toast>>,
    share: Mutex<Option<ShareHandle>>,
    last_clip: Mutex<Option<Clip>>,
    focus_request: AtomicBool,
    clip_gate: Mutex<f64>,
    record_gate: Mutex<f64>,
}

impl Core {
    pub fn new() -> Arc<Core> {
        let config = Config::load();
        let cfg = Arc::new(Mutex::new(config));
        let log = Arc::new(Logger::new(cache_dir().join("limedal.log")));
        let ff = Arc::new(Ffmpeg::new());
        let audio = Arc::new(AudioSources::new());
        let library = Arc::new(Library::new(cfg.clone(), log.clone(), ff.clone()));
        let engine = Arc::new(Engine::new(
            cfg.clone(),
            log.clone(),
            library.clone(),
            audio.clone(),
            ff.clone(),
        ));
        let detector = Arc::new(Detector::new());

        Arc::new(Core {
            cfg,
            log,
            ff,
            audio,
            library,
            engine,
            detector,
            quit: AtomicBool::new(false),
            ctx: Mutex::new(None),
            hotkeys: Mutex::new(HotkeyState {
                available: false,
                reason: "başlatılıyor".to_string(),
                devices: 0,
                combos: Vec::new(),
            }),
            toasts: Mutex::new(Vec::new()),
            share: Mutex::new(None),
            last_clip: Mutex::new(None),
            focus_request: AtomicBool::new(false),
            clip_gate: Mutex::new(0.0),
            record_gate: Mutex::new(0.0),
        })
    }

    // ------------------------------------------------------------------ kurulum
    pub fn bootstrap(self: &Arc<Self>) {
        let version = env!("CARGO_PKG_VERSION");
        self.log.info(&format!("LiMedal {} başlıyor", version));
        if !self.ff.available() {
            self.log
                .error("ffmpeg bulunamadı! 'sudo apt install ffmpeg'");
        } else {
            self.log.info(&format!(
                "ffmpeg: {} | kodlayıcı: {}",
                self.ff.path(),
                self.ff.best_encoder(&self.config().capture.encoder)
            ));
        }

        let config = self.config();
        if config.app.auto_start_capture {
            if let Err(err) = self.engine.start() {
                self.log.warn(&format!("otomatik kayıt başlatılamadı: {}", err));
            }
        }

        self.library.cleanup_tmp(1800.0);
        hotkeys::spawn(self.clone());
        self.spawn_detector();
        self.spawn_janitor();
        ipc::spawn(self.clone());

        if config.app.share_enabled {
            match share::start(self.clone(), config.app.share_port) {
                Ok(handle) => {
                    self.log
                        .info(&format!("Paylaşım sunucusu: 0.0.0.0:{}", handle.port));
                    if let Ok(mut guard) = self.share.lock() {
                        *guard = Some(handle);
                    }
                }
                Err(err) => self.log.warn(&format!("paylaşım sunucusu: {}", err)),
            }
        }
    }

    fn spawn_detector(self: &Arc<Self>) {
        let core = self.clone();
        std::thread::Builder::new()
            .name("limedal-detect".to_string())
            .spawn(move || {
                let mut previous: Option<String> = None;
                loop {
                    if core.should_quit() {
                        break;
                    }
                    if core.config().app.game_detection {
                        let found = core.detector.refresh();
                        let current = found.first().map(|game| game.name.clone());
                        if current != previous {
                            match &current {
                                Some(name) => core.log.info(&format!("Oyun algılandı: {}", name)),
                                None => core.log.info("Oyun kapandı"),
                            }
                            previous = current;
                        }
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            })
            .ok();
    }

    fn spawn_janitor(self: &Arc<Self>) {
        let core = self.clone();
        std::thread::Builder::new()
            .name("limedal-janitor".to_string())
            .spawn(move || loop {
                if core.should_quit() {
                    break;
                }
                core.engine.prune();
                std::thread::sleep(Duration::from_secs(4));
            })
            .ok();
    }

    // ------------------------------------------------------------------ ayarlar
    pub fn config(&self) -> Config {
        match self.cfg.lock() {
            Ok(config) => config.clone(),
            Err(_) => Config::default(),
        }
    }

    pub fn apply_config(&self, mut config: Config) {
        config.sanitize();
        {
            if let Ok(mut guard) = self.cfg.lock() {
                *guard = config.clone();
            }
        }
        config.save();
        self.repaint();
    }

    // ------------------------------------------------------------------ durum
    pub fn status(&self) -> CaptureStatus {
        self.engine.status()
    }

    pub fn stats(&self) -> Stats {
        self.library.stats()
    }

    pub fn clips(&self, query: &str, sort: &str) -> Vec<Clip> {
        self.library.list(query, sort)
    }

    pub fn game_name(&self) -> Option<String> {
        self.detector.current_name()
    }

    pub fn detected(&self) -> Vec<Game> {
        self.detector.list()
    }

    pub fn last_clip(&self) -> Option<Clip> {
        match self.last_clip.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        }
    }

    // ------------------------------------------------------------------ eylemler
    /// Kısa aralıktaki tekrarları eler. Aynı tuş hem egui penceresinden hem
    /// global /dev/input dinleyicisinden gelebildiği için gerekli.
    fn gate(slot: &Mutex<f64>, window: f64) -> bool {
        let stamp = now();
        match slot.lock() {
            Ok(mut guard) => {
                if stamp - *guard < window {
                    return false;
                }
                *guard = stamp;
                true
            }
            Err(_) => true,
        }
    }

    pub fn save_clip_now(
        &self,
        seconds: Option<u32>,
        title: Option<String>,
    ) -> Result<Clip, String> {
        if !Core::gate(&self.clip_gate, 1.2) {
            return match self.last_clip() {
                Some(clip) => Ok(clip),
                None => Err("Klip isteği çok sık tekrarlandı.".to_string()),
            };
        }
        let game = self.game_name();
        match self.engine.save_clip(seconds, title, game.clone()) {
            Ok(clip) => {
                self.notify("Klip kaydedildi", &clip.meta.title);
                self.toast(
                    format!("Klip kaydedildi · {}", clip.meta.duration_text()),
                    ToastKind::Ok,
                );
                if let Ok(mut guard) = self.last_clip.lock() {
                    *guard = Some(clip.clone());
                }
                self.repaint();
                Ok(clip)
            }
            Err(err) => {
                self.toast(format!("Klip kaydedilemedi: {}", err), ToastKind::Error);
                self.log.warn(&format!("klip kaydedilemedi: {}", err));
                Err(err)
            }
        }
    }

    /// Tam kaydı açar/kapatır: (kayıt_sürüyor_mu)
    pub fn toggle_record(&self) -> Result<bool, String> {
        if !Core::gate(&self.record_gate, 0.8) {
            return Ok(self.engine.is_recording());
        }
        if self.engine.is_recording() {
            let _ = self.stop_recording()?;
            Ok(false)
        } else {
            self.start_recording()?;
            Ok(true)
        }
    }

    pub fn start_recording(&self) -> Result<(), String> {
        if self.engine.is_recording() {
            return Ok(());
        }
        match self.engine.start_recording() {
            Ok(()) => {
                self.notify("Kayıt başladı", "Durdurmak için F10");
                self.toast("Tam kayıt başladı".to_string(), ToastKind::Ok);
                self.repaint();
                Ok(())
            }
            Err(err) => {
                self.toast(format!("Kayıt başlatılamadı: {}", err), ToastKind::Error);
                self.log.warn(&format!("kayıt başlatılamadı: {}", err));
                Err(err)
            }
        }
    }

    pub fn stop_recording(&self) -> Result<Clip, String> {
        let game = self.game_name();
        match self.engine.stop_recording(game) {
            Ok(clip) => {
                self.notify("Kayıt tamamlandı", &clip.meta.title);
                self.toast(
                    format!("Kayıt kaydedildi · {}", clip.meta.duration_text()),
                    ToastKind::Ok,
                );
                if let Ok(mut guard) = self.last_clip.lock() {
                    *guard = Some(clip.clone());
                }
                self.repaint();
                Ok(clip)
            }
            Err(err) => {
                self.toast(format!("Kayıt durdurulamadı: {}", err), ToastKind::Error);
                Err(err)
            }
        }
    }

    pub fn hotkey_save_clip(self: &Arc<Self>) {
        let core = self.clone();
        std::thread::spawn(move || {
            let seconds = core.config().app.clip_seconds_default;
            let _ = core.save_clip_now(Some(seconds), None);
        });
    }

    pub fn hotkey_toggle_record(self: &Arc<Self>) {
        let core = self.clone();
        std::thread::spawn(move || {
            let _ = core.toggle_record();
        });
    }

    pub fn notify(&self, title: &str, body: &str) {
        if !self.config().app.notifications || which("notify-send").is_none() {
            return;
        }
        let _ = Command::new("notify-send")
            .args(["-a", "LiMedal", "-t", "3200", title, body])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    pub fn open_path(&self, path: &std::path::Path) {
        let _ = Command::new("xdg-open")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    // ------------------------------------------------------------------ bildirim
    pub fn toast(&self, text: String, kind: ToastKind) {
        if let Ok(mut guard) = self.toasts.lock() {
            if guard.len() >= 6 {
                guard.remove(0);
            }
            guard.push(Toast {
                text,
                kind,
                at: now(),
            });
        }
        self.repaint();
    }

    pub fn toasts(&self) -> Vec<Toast> {
        let mut alive: Vec<Toast> = Vec::new();
        if let Ok(mut guard) = self.toasts.lock() {
            let stamp = now();
            guard.retain(|toast| stamp - toast.at <= 4.0);
            alive = guard.clone();
        }
        alive
    }

    // ------------------------------------------------------------------ egui
    pub fn set_ctx(&self, ctx: egui::Context) {
        if let Ok(mut guard) = self.ctx.lock() {
            *guard = Some(ctx);
        }
    }

    pub fn repaint(&self) {
        if let Ok(guard) = self.ctx.lock() {
            if let Some(ctx) = guard.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    // ------------------------------------------------------------------ kısayol durumu
    pub fn set_hotkey_state(&self, state: HotkeyState) {
        if let Ok(mut guard) = self.hotkeys.lock() {
            *guard = state;
        }
        self.repaint();
    }

    pub fn hotkey_state(&self) -> HotkeyState {
        match self.hotkeys.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => HotkeyState {
                available: false,
                reason: "okunamadı".to_string(),
                devices: 0,
                combos: Vec::new(),
            },
        }
    }

    // ------------------------------------------------------------------ paylaşım
    pub fn share_url(&self, clip: &Clip) -> Option<String> {
        let port = match self.share.lock() {
            Ok(guard) => guard.as_ref().map(|handle| handle.port),
            Err(_) => None,
        }?;
        if clip.meta.token.is_empty() {
            return None;
        }
        Some(format!(
            "http://{}:{}/s/{}",
            crate::share::lan_ip(),
            port,
            clip.meta.token
        ))
    }

    pub fn stop_share(&self) {
        let handle = match self.share.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some(handle) = handle {
            handle.stop();
            self.log.info("Paylaşım sunucusu kapatıldı");
        }
    }

    // ------------------------------------------------------------------ kapanış
    pub fn should_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    pub fn request_quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
        self.repaint();
    }

    /// `limedal open` ya da ikinci kez `limedal run`: pencereyi öne getirir.
    pub fn request_focus(&self) {
        self.focus_request.store(true, Ordering::SeqCst);
        self.repaint();
    }

    pub fn take_focus_request(&self) -> bool {
        self.focus_request.swap(false, Ordering::SeqCst)
    }

    pub fn shutdown(self: &Arc<Self>) {
        self.log.info("LiMedal kapanıyor");
        self.stop_share();
        self.engine.shutdown();
        ipc::cleanup();
    }

    // ------------------------------------------------------------------ tanı
    pub fn doctor(&self) -> Vec<(bool, String, String)> {
        let mut lines: Vec<(bool, String, String)> = Vec::new();
        lines.push((
            true,
            "sürüm".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        ));
        lines.push((
            self.ff.available(),
            "ffmpeg".to_string(),
            self.ff.path(),
        ));
        let encoders = self.ff.encoders();
        lines.push((
            !encoders.is_empty(),
            "kodlayıcılar".to_string(),
            encoders.join(", "),
        ));
        for name in ["h264_nvenc", "h264_vaapi", "h264_qsv", "libx264"] {
            if !encoders.iter().any(|item| item == name) {
                continue;
            }
            let config = self.config();
            match self.ff.self_test(name, &config.capture.quality, config.capture.fps) {
                Ok(()) => lines.push((true, name.to_string(), encoder_label(name).to_string())),
                Err(err) => lines.push((
                    false,
                    name.to_string(),
                    format!("{} -> {}", encoder_label(name), err),
                )),
            }
        }
        lines.push((
            self.ff.vaapi_device().is_some(),
            "vaapi aygıtı".to_string(),
            self.ff
                .vaapi_device()
                .unwrap_or_else(|| "yok".to_string()),
        ));
        let config = self.config();
        let display = config.display();
        let (width, height, source) = self.ff.display_resolution(&display);
        lines.push((
            width >= 320,
            "çözünürlük".to_string(),
            format!("{}x{} ({}, DISPLAY={})", width, height, source, display),
        ));
        let audio = self.audio.status();
        lines.push((
            audio.available,
            "pactl".to_string(),
            "pulseaudio-utils".to_string(),
        ));
        let monitors: Vec<String> = audio
            .sources
            .iter()
            .filter(|source| source.kind == crate::audio::SourceKind::Monitor)
            .map(|source| source.label.clone())
            .collect();
        let mics: Vec<String> = audio
            .sources
            .iter()
            .filter(|source| source.kind == crate::audio::SourceKind::Input)
            .map(|source| source.label.clone())
            .collect();
        lines.push((
            !monitors.is_empty(),
            "sistem sesi".to_string(),
            monitors
                .first()
                .cloned()
                .unwrap_or_else(|| "monitör kaynağı yok".to_string()),
        ));
        lines.push((
            true,
            "mikrofon".to_string(),
            mics.first().cloned().unwrap_or_else(|| "yok".to_string()),
        ));

        lines.push((
            true,
            "hyprland".to_string(),
            crate::hyprland::summary(),
        ));

        let devices = count_input_devices();
        lines.push((
            devices > 0,
            "kısayol erişimi".to_string(),
            if devices > 0 {
                format!("{} aygıt okunabilir", devices)
            } else {
                "erişim yok (sudo usermod -aG input $USER)".to_string()
            },
        ));
        lines.push((
            which("notify-send").is_some(),
            "bildirimler".to_string(),
            which("notify-send")
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| "libnotify-bin kurun".to_string()),
        ));

        let library_dir = config.library_dir();
        lines.push((
            library_dir.exists(),
            "kütüphane".to_string(),
            library_dir.to_string_lossy().to_string(),
        ));
        lines.push((
            true,
            "günlük".to_string(),
            self.log.path().to_string_lossy().to_string(),
        ));
        lines.push((
            true,
            "soket".to_string(),
            ipc::path().to_string_lossy().to_string(),
        ));
        lines
    }
}

fn count_input_devices() -> usize {
    let entries = match std::fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let mut count = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("event") {
            continue;
        }
        if std::fs::File::open(entry.path()).is_ok() {
            count += 1;
        }
    }
    count
}
