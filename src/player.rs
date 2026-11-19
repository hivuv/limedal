//! Klip önizlemesi: ffmpeg'den ham RGBA kare akışı.
//!
//! egui'de video oynatıcı yok; bu yüzden ffmpeg'i iki şekilde kullanıyoruz:
//! zaman çizgisi sürüklerken tek kare ("scrub"), oynatırken `-re` ile gerçek
//! zamanlı kare akışı. Ses için varsa `ffplay` (SDL) kullanılır.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::util::which;

pub struct DecodedFrame {
    pub image: egui::ColorImage,
    pub at: f64,
}

struct ScrubRequest {
    at: f64,
    generation: u64,
}

/// Kaynak ölçüsünden önizleme ölçüsü üretir (en fazla 760 piksel genişlik).
pub fn preview_size(width: u32, height: u32) -> (usize, usize) {
    let width = width.max(160) as f32;
    let height = height.max(90) as f32;
    let scale = (760.0 / width).min(1.0);
    let out_width = (width * scale) as usize;
    let out_height = (height * scale) as usize;
    let out_width = out_width.max(2);
    let out_height = out_height.max(2);
    (out_width - out_width % 2, out_height - out_height % 2)
}

pub struct Player {
    ffmpeg: String,
    pub path: PathBuf,
    pub duration: f64,
    pub fps: f32,
    pub width: usize,
    pub height: usize,
    pub playing: bool,
    pub clock: f64,
    pub audio_enabled: bool,
    pub audio_available: bool,
    pub error: Option<String>,
    stream: Option<Child>,
    audio: Option<Child>,
    frame: Arc<Mutex<Option<DecodedFrame>>>,
    stop: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    pending: Arc<Mutex<Option<ScrubRequest>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    pub fn new(ffmpeg: String, path: PathBuf, duration: f64, fps: f32, width: u32, height: u32) -> Player {
        let (preview_width, preview_height) = preview_size(width, height);
        let audio_available = which("ffplay").is_some();
        let mut player = Player {
            ffmpeg,
            path,
            duration: duration.max(0.0),
            fps: if fps > 1.0 { fps } else { 30.0 },
            width: preview_width,
            height: preview_height,
            playing: false,
            clock: 0.0,
            audio_enabled: audio_available,
            audio_available,
            error: None,
            stream: None,
            audio: None,
            frame: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(Mutex::new(None)),
            worker: None,
        };
        player.spawn_worker();
        player.request_scrub(0.0);
        player
    }

    // ------------------------------------------------------------------ komut
    fn scale_filter(&self) -> String {
        format!(
            "scale={}:{}:flags=fast_bilinear",
            self.width, self.height
        )
    }

    fn stream_args(&self, start: f64) -> Vec<String> {
        vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-nostdin".to_string(),
            "-re".to_string(),
            "-ss".to_string(),
            format!("{:.3}", start.max(0.0)),
            "-i".to_string(),
            self.path.to_string_lossy().to_string(),
            "-an".to_string(),
            "-vf".to_string(),
            self.scale_filter(),
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pix_fmt".to_string(),
            "rgba".to_string(),
            "-".to_string(),
        ]
    }

    // ------------------------------------------------------------------ oynatma
    pub fn play(&mut self) {
        if self.playing {
            return;
        }
        if self.duration > 0.0 && self.clock >= self.duration - 0.12 {
            self.clock = 0.0;
            self.request_scrub(0.0);
        }
        self.playing = true;
        self.error = None;
        self.start_stream();
        self.start_audio();
    }

    pub fn pause(&mut self) {
        if !self.playing {
            self.request_scrub(self.clock);
            return;
        }
        self.playing = false;
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.kill_stream();
        self.kill_audio();
        self.request_scrub(self.clock);
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&mut self, at: f64) {
        let target = if self.duration > 0.0 {
            at.clamp(0.0, self.duration)
        } else {
            at.max(0.0)
        };
        self.clock = target;
        if self.playing {
            self.start_stream();
            self.kill_audio();
            self.start_audio();
        } else {
            self.request_scrub(target);
        }
    }

    pub fn step(&mut self, frames: f64) {
        let delta = frames / self.fps.max(1.0) as f64;
        self.seek(self.clock + delta);
    }

    pub fn set_audio(&mut self, enabled: bool) {
        self.audio_enabled = enabled && self.audio_available;
        if !self.audio_enabled {
            self.kill_audio();
        } else if self.playing {
            self.kill_audio();
            self.start_audio();
        }
    }

    // ------------------------------------------------------------------ akış
    fn start_stream(&mut self) {
        self.kill_stream();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut slot) = self.pending.lock() {
            *slot = None;
        }

        let args = self.stream_args(self.clock);
        let mut command = Command::new(&self.ffmpeg);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take();
                self.stream = Some(child);
                if let Some(stdout) = stdout {
                    let frame = self.frame.clone();
                    let stop = self.stop.clone();
                    let shared_generation = self.generation.clone();
                    let (width, height) = (self.width, self.height);
                    let fps = self.fps.max(1.0) as f64;
                    let base = self.clock;
                    let _ = std::thread::Builder::new()
                        .name("limedal-preview".to_string())
                        .spawn(move || {
                            read_stream(
                                stdout,
                                width,
                                height,
                                fps,
                                base,
                                frame,
                                stop,
                                shared_generation,
                                generation,
                            );
                        });
                }
            }
            Err(err) => {
                self.error = Some(format!("önizleme başlatılamadı: {}", err));
                self.playing = false;
            }
        }
    }

    fn kill_stream(&mut self) {
        if let Some(mut child) = self.stream.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn start_audio(&mut self) {
        self.kill_audio();
        if !self.audio_enabled || !self.audio_available {
            return;
        }
        if self.duration > 0.0 && self.clock >= self.duration - 0.15 {
            return;
        }
        let ffplay = match which("ffplay") {
            Some(path) => path,
            None => return,
        };
        let path = self.path.to_string_lossy().to_string();
        let start = format!("{:.3}", self.clock);
        let args: Vec<String> = vec![
            "-nodisp".to_string(),
            "-autoexit".to_string(),
            "-loglevel".to_string(),
            "quiet".to_string(),
            "-ss".to_string(),
            start,
            path,
        ];
        if let Ok(child) = Command::new(ffplay)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            self.audio = Some(child);
        }
    }

    fn kill_audio(&mut self) {
        if let Some(mut child) = self.audio.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    // ------------------------------------------------------------------ kare
    fn request_scrub(&mut self, at: f64) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some(ScrubRequest { at, generation });
        }
    }

    /// Yeni kare geldiyse `(görüntü, zaman)` döner.
    pub fn poll(&mut self) -> Option<(egui::ColorImage, f64)> {
        if let Some(child) = self.stream.as_mut() {
            if let Ok(Some(_)) = child.try_wait() {
                if self.playing {
                    self.playing = false;
                    self.kill_audio();
                }
            }
        }
        let taken = match self.frame.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        taken.map(|frame| {
            self.clock = frame.at;
            (frame.image, frame.at)
        })
    }

    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        if self.duration > 0.0 && self.clock >= self.duration - 0.04 {
            self.clock = self.duration;
            self.playing = false;
            self.generation.fetch_add(1, Ordering::SeqCst);
            self.kill_stream();
            self.kill_audio();
        }
    }

    // ------------------------------------------------------------------ işçi
    fn spawn_worker(&mut self) {
        let pending = self.pending.clone();
        let frame = self.frame.clone();
        let stop = self.stop.clone();
        let generation = self.generation.clone();
        let ffmpeg = self.ffmpeg.clone();
        let path = self.path.to_string_lossy().to_string();
        let (width, height) = (self.width, self.height);

        let handle = std::thread::Builder::new()
            .name("limedal-scrub".to_string())
            .spawn(move || {
                let size = width * height * 4;
                let mut buffer = vec![0u8; size];
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let request = match pending.lock() {
                        Ok(mut slot) => slot.take(),
                        Err(_) => None,
                    };
                    let request = match request {
                        Some(value) => value,
                        None => {
                            std::thread::sleep(std::time::Duration::from_millis(24));
                            continue;
                        }
                    };
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let args: Vec<String> = vec![
                        "-hide_banner".to_string(),
                        "-loglevel".to_string(),
                        "error".to_string(),
                        "-nostdin".to_string(),
                        "-ss".to_string(),
                        format!("{:.3}", request.at.max(0.0)),
                        "-i".to_string(),
                        path.clone(),
                        "-an".to_string(),
                        "-vf".to_string(),
                        format!("scale={}:{}:flags=fast_bilinear", width, height),
                        "-frames:v".to_string(),
                        "1".to_string(),
                        "-f".to_string(),
                        "rawvideo".to_string(),
                        "-pix_fmt".to_string(),
                        "rgba".to_string(),
                        "-".to_string(),
                    ];
                    let child = Command::new(&ffmpeg)
                        .args(&args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn();
                    let mut child = match child {
                        Ok(child) => child,
                        Err(_) => {
                            std::thread::sleep(std::time::Duration::from_millis(120));
                            continue;
                        }
                    };
                    if let Some(mut stdout) = child.stdout.take() {
                        if stdout.read_exact(&mut buffer).is_ok()
                            && generation.load(Ordering::SeqCst) == request.generation
                            && !stop.load(Ordering::SeqCst)
                        {
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [width, height],
                                &buffer,
                            );
                            if let Ok(mut slot) = frame.lock() {
                                *slot = Some(DecodedFrame {
                                    image,
                                    at: request.at,
                                });
                            }
                        }
                    }
                    let _ = child.wait();
                }
            })
            .ok();
        self.worker = handle;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.kill_stream();
        self.kill_audio();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_stream(
    mut stdout: std::process::ChildStdout,
    width: usize,
    height: usize,
    _fps: f64,
    base: f64,
    frame: Arc<Mutex<Option<DecodedFrame>>>,
    stop: Arc<AtomicBool>,
    shared_generation: Arc<AtomicU64>,
    my_generation: u64,
) {
    let size = width * height * 4;
    let mut buffer = vec![0u8; size];
    // Saat kare sayısından değil gerçek zamandan gelir: `-re` ile ffmpeg
    // zaten gerçek hızda üretir, bu yüzden klip fps'i ne olursa olsun doğru.
    let started = std::time::Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) || shared_generation.load(Ordering::SeqCst) != my_generation {
            break;
        }
        if stdout.read_exact(&mut buffer).is_err() {
            break;
        }
        if shared_generation.load(Ordering::SeqCst) != my_generation {
            break;
        }
        let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &buffer);
        let at = base + started.elapsed().as_secs_f64();
        if let Ok(mut slot) = frame.lock() {
            *slot = Some(DecodedFrame { image, at });
        }
    }
}
