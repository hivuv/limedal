//! Yakalama motoru: sürekli segment tamponu, klip çıkarma, tam kayıt.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::audio::AudioSources;
use crate::config::Config;
use crate::ffmpeg::{encoder_label, file_size, Ffmpeg};
use crate::library::{Clip, Library};
use crate::util::{cache_dir, human_duration, now, state_dir, Logger};

#[derive(Clone, Debug)]
pub struct CaptureStatus {
    pub running: bool,
    pub recording: bool,
    pub buffer_seconds: u32,
    pub fill_seconds: f64,
    pub ready: bool,
    pub backend: String,
    pub encoder: String,
    pub encoder_label: String,
    pub resolution: String,
    pub fps: u32,
    pub recording_seconds: f64,
    pub segments: usize,
    pub error: Option<String>,
    pub note: String,
}

impl Default for CaptureStatus {
    fn default() -> Self {
        CaptureStatus {
            running: false,
            recording: false,
            buffer_seconds: 60,
            fill_seconds: 0.0,
            ready: false,
            backend: String::new(),
            encoder: String::new(),
            encoder_label: String::new(),
            resolution: String::new(),
            fps: 60,
            recording_seconds: 0.0,
            segments: 0,
            error: None,
            note: String::new(),
        }
    }
}

struct Inner {
    child: Option<Child>,
    buffer_dir: Option<PathBuf>,
    started_at: f64,
    seg_seconds: u32,
    backend: String,
    encoder: String,
    resolution: String,
    note: String,
    last_error: Option<String>,
    rec_child: Option<Child>,
    rec_started: f64,
    rec_target: Option<PathBuf>,
}

impl Inner {
    fn new() -> Inner {
        Inner {
            child: None,
            buffer_dir: None,
            started_at: 0.0,
            seg_seconds: 2,
            backend: String::new(),
            encoder: String::new(),
            resolution: String::new(),
            note: String::new(),
            last_error: None,
            rec_child: None,
            rec_started: 0.0,
            rec_target: None,
        }
    }
}

struct BuildInfo {
    encoder: String,
    backend: String,
    resolution: String,
    note: String,
    seg_seconds: u32,
}

pub struct Engine {
    cfg: Arc<Mutex<Config>>,
    pub log: Arc<Logger>,
    lib: Arc<Library>,
    audio: Arc<AudioSources>,
    ff: Arc<Ffmpeg>,
    inner: Mutex<Inner>,
    op: Mutex<()>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    clip_lock: Mutex<()>,
}

impl Engine {
    pub fn new(
        cfg: Arc<Mutex<Config>>,
        log: Arc<Logger>,
        lib: Arc<Library>,
        audio: Arc<AudioSources>,
        ff: Arc<Ffmpeg>,
    ) -> Engine {
        Engine {
            cfg,
            log,
            lib,
            audio,
            ff,
            inner: Mutex::new(Inner::new()),
            op: Mutex::new(()),
            stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
            clip_lock: Mutex::new(()),
        }
    }

    pub fn config_snapshot(&self) -> Config {
        match self.cfg.lock() {
            Ok(config) => config.clone(),
            Err(_) => Config::default(),
        }
    }

    // ------------------------------------------------------------------ durum
    pub fn status(&self) -> CaptureStatus {
        let config = self.config_snapshot();
        let mut status = CaptureStatus {
            buffer_seconds: config.capture.buffer_seconds,
            fps: config.capture.fps,
            ..CaptureStatus::default()
        };
        let mut died = false;

        if let Ok(mut inner) = self.inner.lock() {
            let mut child_died = false;
            if let Some(child) = inner.child.as_mut() {
                match child.try_wait() {
                    Ok(None) => status.running = true,
                    Ok(Some(_)) => child_died = true,
                    Err(_) => status.running = true,
                }
            }
            if child_died {
                inner.child = None;
                inner.started_at = 0.0;
                died = true;
            }

            let mut rec_died = false;
            if let Some(child) = inner.rec_child.as_mut() {
                match child.try_wait() {
                    Ok(None) => status.recording = true,
                    Ok(Some(_)) => rec_died = true,
                    Err(_) => status.recording = true,
                }
            }
            if rec_died {
                inner.rec_child = None;
                inner.rec_started = 0.0;
            }

            status.backend = inner.backend.clone();
            status.encoder = inner.encoder.clone();
            status.encoder_label = encoder_label(&inner.encoder).to_string();
            status.resolution = inner.resolution.clone();
            status.note = inner.note.clone();
            status.error = inner.last_error.clone();
            if status.running {
                status.fill_seconds = (now() - inner.started_at)
                    .max(0.0)
                    .min(config.capture.buffer_seconds as f64);
            }
            if status.recording {
                status.recording_seconds = (now() - inner.rec_started).max(0.0);
            }
            if let Some(dir) = inner.buffer_dir.as_ref() {
                status.segments = list_segments(dir)
                    .iter()
                    .filter(|(_, path)| file_size(path) > 512)
                    .count();
            }
        }

        if died {
            let tail = self.error_tail();
            let message = if tail.is_empty() {
                "ffmpeg beklenmedik şekilde kapandı.".to_string()
            } else {
                tail
            };
            if let Ok(mut inner) = self.inner.lock() {
                if inner.last_error.is_none() {
                    inner.last_error = Some(message.clone());
                }
            }
            status.error = Some(message);
        }

        status.ready = status.running && status.fill_seconds >= 3.0;
        status
    }

    pub fn is_running(&self) -> bool {
        match self.inner.lock() {
            Ok(mut inner) => match inner.child.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(None) | Err(_)),
                None => false,
            },
            Err(_) => false,
        }
    }

    pub fn is_recording(&self) -> bool {
        match self.inner.lock() {
            Ok(mut inner) => match inner.rec_child.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(None) | Err(_)),
                None => false,
            },
            Err(_) => false,
        }
    }

    // ------------------------------------------------------------------ başlat/durdur
    pub fn start(&self) -> Result<(), String> {
        let _op = match self.op.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("motor kilidi bozuk".to_string()),
        };
        if self.is_running() {
            return Ok(());
        }
        self.stop_locked();

        // Segmentler diskte (XDG_CACHE_HOME) tutulur: uzun tamponlar tmpfs'te
        // RAM'i şişirip yazmayı yarıda kesebiliyordu.
        let base = cache_dir().join("buffer");
        let _ = std::fs::remove_dir_all(&base);
        // Eski sürümlerden kalan tmpfs dizinini de temizle.
        let _ = std::fs::remove_dir_all(state_dir().join("buffer"));
        std::fs::create_dir_all(&base).map_err(|err| format!("Tampon dizini: {}", err))?;

        let config = self.config_snapshot();
        let segment = config.capture.segment_seconds.clamp(1, 10);
        let args: Vec<String> = vec![
            "-f".to_string(),
            "segment".to_string(),
            "-segment_time".to_string(),
            segment.to_string(),
            "-segment_format".to_string(),
            "mp4".to_string(),
            "-reset_timestamps".to_string(),
            "1".to_string(),
            "-segment_start_number".to_string(),
            "1".to_string(),
            base.join("seg_%06d.mp4").to_string_lossy().to_string(),
        ];

        let (cmd, info) = self.build(&args).map_err(|err| {
            if let Ok(mut inner) = self.inner.lock() {
                inner.last_error = Some(err.clone());
            }
            err
        })?;

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env("DISPLAY", config.display())
            .env("XDG_RUNTIME_DIR", runtime_dir())
            .spawn()
            .map_err(|err| format!("ffmpeg başlatılamadı: {}", err))?;

        let stderr = child.stderr.take();
        pump_stderr(
            self.stderr_tail.clone(),
            self.log.clone(),
            stderr,
            "tampon".to_string(),
        );

        if let Ok(mut inner) = self.inner.lock() {
            inner.child = Some(child);
            inner.buffer_dir = Some(base.clone());
            inner.started_at = now();
            inner.seg_seconds = info.seg_seconds;
            inner.backend = info.backend.clone();
            inner.encoder = info.encoder.clone();
            inner.resolution = info.resolution.clone();
            inner.note = info.note.clone();
            inner.last_error = None;
        }

        std::thread::sleep(Duration::from_millis(1500));

        let died = match self.inner.lock() {
            Ok(mut inner) => match inner.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => true,
                    _ => false,
                },
                None => true,
            },
            Err(_) => true,
        };
        if died {
            let detail = self.error_tail();
            if let Ok(mut inner) = self.inner.lock() {
                inner.last_error = Some(if detail.is_empty() {
                    "ffmpeg beklenmedik şekilde kapandı.".to_string()
                } else {
                    detail
                });
                inner.child = None;
                inner.started_at = 0.0;
            }
            return Err(self
                .status()
                .error
                .unwrap_or_else(|| "Kayıt başlatılamadı.".to_string()));
        }

        self.log.info(&format!(
            "Tampon hazır: {}s geri sarma, {} ({})",
            config.capture.buffer_seconds, info.resolution, info.encoder
        ));
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        let _op = match self.op.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("motor kilidi bozuk".to_string()),
        };
        if self.is_recording() {
            let _ = self.stop_recording(None);
        }
        self.stop_locked();
        self.log.info("Tampon durduruldu.");
        Ok(())
    }

    fn stop_locked(&self) {
        let child = match self.inner.lock() {
            Ok(mut inner) => {
                inner.started_at = 0.0;
                inner.child.take()
            }
            Err(_) => None,
        };
        if let Some(mut child) = child {
            terminate(&mut child, 4000);
        }
    }

    pub fn restart(&self) -> Result<(), String> {
        let _ = self.stop();
        self.start()
    }

    // ------------------------------------------------------------------ komut
    fn build(&self, out_args: &[String]) -> Result<(Vec<String>, BuildInfo), String> {
        if !self.ff.available() {
            return Err("ffmpeg bulunamadı. 'sudo apt install ffmpeg' ile kurun.".to_string());
        }
        let config = self.config_snapshot();
        let cap = config.capture.clone();
        let fps = cap.fps;
        let quality = cap.quality.clone();
        let segment = cap.segment_seconds.clamp(1, 10);

        let (encoder, mut vf, note) = self.ff.resolve(&cap.encoder, &quality, fps);
        if !note.is_empty() {
            self.log.warn(&note);
        }

        let (width, height) = self.video_size(&config);
        let (game_src, mic_src) = self
            .audio
            .resolve(&cap.audio.game_source, &cap.audio.mic_source);

        let mut cmd: Vec<String> = vec![
            self.ff.path(),
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
            "-y".to_string(),
        ];

        if encoder == "h264_vaapi" {
            match self.ff.vaapi_device() {
                Some(device) => {
                    cmd.push("-vaapi_device".to_string());
                    cmd.push(device);
                }
                None => return Err("VA-API aygıtı bulunamadı (/dev/dri/render*)".to_string()),
            }
        }

        let display = config.display();
        cmd.push("-thread_queue_size".to_string());
        cmd.push("1024".to_string());
        cmd.push("-f".to_string());
        cmd.push("x11grab".to_string());
        cmd.push("-draw_mouse".to_string());
        cmd.push(if cap.draw_cursor { "1" } else { "0" }.to_string());
        cmd.push("-framerate".to_string());
        cmd.push(fps.to_string());
        cmd.push("-video_size".to_string());
        cmd.push(format!("{}x{}", width, height));
        cmd.push("-i".to_string());
        cmd.push(format!("{}+0,0", display));

        let mut audio_inputs: Vec<(String, String, f32)> = Vec::new();
        if let Some(source) = game_src {
            audio_inputs.push(("game".to_string(), source, cap.audio.game_volume));
        }
        if let Some(source) = mic_src {
            audio_inputs.push(("mic".to_string(), source, cap.audio.mic_volume));
        }
        for (_, name, _) in &audio_inputs {
            cmd.push("-thread_queue_size".to_string());
            cmd.push("1024".to_string());
            cmd.push("-f".to_string());
            cmd.push("pulse".to_string());
            cmd.push("-i".to_string());
            cmd.push(name.clone());
        }

        let mut filters: Vec<String> = Vec::new();
        let mut maps: Vec<String> = Vec::new();
        match vf.take() {
            Some(filter) => {
                filters.push(format!("[0:v:0]{}[vout]", filter));
                maps.push("-map".to_string());
                maps.push("[vout]".to_string());
            }
            None => {
                maps.push("-map".to_string());
                maps.push("0:v:0".to_string());
            }
        }
        for (index, (role, _, volume)) in audio_inputs.iter().enumerate() {
            let label = format!("a{}", role);
            let mut chain = "aresample=async=1:first_pts=0".to_string();
            if (*volume - 1.0).abs() > 0.01 {
                chain.push_str(&format!(",volume={:.2}", volume.clamp(0.0, 3.0)));
            }
            filters.push(format!("[{}:a:0]{}[{}]", index + 1, chain, label));
            maps.push("-map".to_string());
            maps.push(format!("[{}]", label));
        }
        if !filters.is_empty() {
            cmd.push("-filter_complex".to_string());
            cmd.push(filters.join(";"));
        }
        cmd.extend(maps);

        cmd.extend(self.ff.video_args(&encoder, &quality, fps, segment));
        if !audio_inputs.is_empty() {
            cmd.push("-c:a".to_string());
            cmd.push("aac".to_string());
            cmd.push("-b:a".to_string());
            cmd.push("192k".to_string());
            cmd.push("-ar".to_string());
            cmd.push("48000".to_string());
            cmd.push("-ac".to_string());
            cmd.push("2".to_string());
            for (index, (role, _, _)) in audio_inputs.iter().enumerate() {
                let title = if role == "game" { "Oyun" } else { "Mikrofon" };
                cmd.push(format!("-metadata:s:a:{}", index));
                cmd.push(format!("title={}", title));
            }
        }
        cmd.extend(out_args.iter().cloned());

        self.log.info(&format!(
            "komut: {} / x11grab / {}x{} @{} / ses: {}{}",
            encoder,
            width,
            height,
            fps,
            audio_inputs
                .iter()
                .map(|(role, _, _)| role.clone())
                .collect::<Vec<String>>()
                .join("+"),
            if audio_inputs.is_empty() { " (sessiz)" } else { "" }
        ));

        Ok((
            cmd,
            BuildInfo {
                encoder,
                backend: "x11grab".to_string(),
                resolution: format!("{}x{}", width, height),
                note,
                seg_seconds: segment,
            },
        ))
    }

    fn video_size(&self, config: &Config) -> (u32, u32) {
        let mut width = config.capture.width;
        let mut height = config.capture.height;
        if width.is_none() || height.is_none() {
            let (det_w, det_h, _) = self.ff.display_resolution(&config.display());
            if width.is_none() {
                width = Some(det_w);
            }
            if height.is_none() {
                height = Some(det_h);
            }
        }
        let mut w = width.unwrap_or(1920);
        let mut h = height.unwrap_or(1080);
        w -= w % 2;
        h -= h % 2;
        if w < 320 || h < 240 {
            return (1920, 1080);
        }
        (w, h)
    }

    // ------------------------------------------------------------------ klip
    pub fn save_clip(
        &self,
        seconds: Option<u32>,
        title: Option<String>,
        game: Option<String>,
    ) -> Result<Clip, String> {
        let _clip = match self.clip_lock.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("klip kilidi bozuk".to_string()),
        };

        if !self.is_running() {
            return Err(self
                .status()
                .error
                .unwrap_or_else(|| "Kayıt motoru çalışmıyor.".to_string()));
        }

        let config = self.config_snapshot();
        let wanted = seconds
            .unwrap_or(config.app.clip_seconds_default)
            .clamp(2, 1800);

        let (buffer_dir, seg_seconds) = match self.inner.lock() {
            Ok(inner) => (inner.buffer_dir.clone(), inner.seg_seconds),
            Err(_) => return Err("durum okunamadı".to_string()),
        };
        let buffer_dir = buffer_dir.ok_or_else(|| "Tampon dizini hazır değil.".to_string())?;

        // Yalnızca sonlandırılmış segmentler kullanılabilir: ffmpeg'in hâlâ
        // yazdığı dosyada moov atomu yoktur, concat demuxer'ı onu açamaz
        // ("moov atom not found"). Bu yüzden her dosyayı tek tek doğruluyoruz.
        let segments: Vec<(u64, PathBuf)> = list_segments(&buffer_dir)
            .into_iter()
            .filter(|(_, path)| is_finalized_mp4(path))
            .collect();
        if segments.is_empty() {
            return Err(
                "Tampon henüz dolmadı (ilk segment yazılıyor). Birkaç saniye sonra tekrar deneyin."
                    .to_string(),
            );
        }

        let needed = ((wanted as f64) / (seg_seconds.max(1) as f64)).ceil() as usize;
        let needed = needed.max(1).min(segments.len());
        let chosen: Vec<PathBuf> = segments[segments.len() - needed..]
            .iter()
            .map(|(_, path)| path.clone())
            .collect();
        let available = (chosen.len() as f64) * seg_seconds as f64;

        let mut list_text = String::new();
        for path in &chosen {
            list_text.push_str(&format!("file '{}'\n", path.to_string_lossy()));
        }
        let list_file = buffer_dir.join("concat.txt");
        std::fs::write(&list_file, list_text).map_err(|err| format!("Liste yazılamadı: {}", err))?;

        let tmp_dir = self.lib.tmp_dir();
        let stamp = crate::util::id_stamp();
        let joined = tmp_dir.join(format!("{}-joined.mp4", stamp));
        let trimmed = tmp_dir.join(format!("{}-final.mp4", stamp));

        if let Err(first_error) = self.ff.concat(&list_file, &joined) {
            // Son segment yarım kalmış olabilir: bir öncekiyle bir kez daha dene.
            let mut recovered = false;
            if chosen.len() > 1 {
                let mut retry_text = String::new();
                for path in &chosen[..chosen.len() - 1] {
                    retry_text.push_str(&format!("file '{}'\n", path.to_string_lossy()));
                }
                if std::fs::write(&list_file, retry_text).is_ok()
                    && self.ff.concat(&list_file, &joined).is_ok()
                {
                    recovered = true;
                    self.log.warn(
                        "Son segment yarım kalmıştı; klip bir önceki segmentle oluşturuldu.",
                    );
                }
            }
            if !recovered {
                return Err(format!(
                    "Klipler birleştirilemedi ({} segment): {}",
                    chosen.len(),
                    first_error
                ));
            }
        }

        let mut final_path = joined.clone();
        if available > wanted as f64 + 0.5 {
            match self
                .ff
                .trim(&joined, &trimmed, Some(wanted as f64), None, None)
            {
                Ok(()) => {
                    final_path = trimmed.clone();
                    let _ = std::fs::remove_file(&joined);
                }
                Err(err) => {
                    self.log
                        .warn(&format!("kırpma başarısız, tam segment kullanılıyor: {}", err));
                }
            }
        }

        let clip_title = title.unwrap_or_else(|| auto_title(game.as_deref()));
        let result = self
            .lib
            .create(&final_path, &clip_title, game.as_deref().unwrap_or(""), "tampon", None);

        let _ = std::fs::remove_file(&joined);
        let _ = std::fs::remove_file(&trimmed);

        match result {
            Ok(clip) => {
                self.log.info(&format!(
                    "Klip kaydedildi: {} ({}, {})",
                    clip.meta.title,
                    clip.meta.id,
                    human_duration(clip.meta.duration)
                ));
                Ok(clip)
            }
            Err(err) => {
                self.log.error(&format!("klip oluşturulamadı: {}", err));
                Err(err)
            }
        }
    }

    // ------------------------------------------------------------------ tam kayıt
    pub fn start_recording(&self) -> Result<(), String> {
        let _op = match self.op.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("motor kilidi bozuk".to_string()),
        };
        if self.is_recording() {
            return Err("Kayıt zaten sürüyor.".to_string());
        }

        let tmp_dir = self.lib.tmp_dir();
        let target = tmp_dir.join(format!("rec-{}.mp4", crate::util::id_stamp()));
        let out_args = vec![
            "-movflags".to_string(),
            "+faststart".to_string(),
            target.to_string_lossy().to_string(),
        ];
        let (cmd, _info) = self.build(&out_args)?;
        let config = self.config_snapshot();

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env("DISPLAY", config.display())
            .env("XDG_RUNTIME_DIR", runtime_dir())
            .spawn()
            .map_err(|err| format!("ffmpeg başlatılamadı: {}", err))?;
        let stderr = child.stderr.take();
        pump_stderr(
            self.stderr_tail.clone(),
            self.log.clone(),
            stderr,
            "kayıt".to_string(),
        );

        if let Ok(mut inner) = self.inner.lock() {
            inner.rec_child = Some(child);
            inner.rec_started = now();
            inner.rec_target = Some(target.clone());
        }

        std::thread::sleep(Duration::from_millis(1200));
        if !self.is_recording() {
            let detail = self.error_tail();
            if let Ok(mut inner) = self.inner.lock() {
                inner.rec_child = None;
                inner.rec_started = 0.0;
                inner.rec_target = None;
            }
            let _ = std::fs::remove_file(&target);
            return Err(if detail.is_empty() {
                "Kayıt başlatılamadı.".to_string()
            } else {
                detail
            });
        }
        self.log
            .info(&format!("Tam kayıt başladı: {}", target.display()));
        Ok(())
    }

    pub fn stop_recording(&self, game: Option<String>) -> Result<Clip, String> {
        let (child, target, started) = match self.inner.lock() {
            Ok(mut inner) => {
                let child = inner.rec_child.take();
                let target = inner.rec_target.take();
                let started = inner.rec_started;
                inner.rec_started = 0.0;
                (child, target, started)
            }
            Err(_) => return Err("durum okunamadı".to_string()),
        };

        let child = match child {
            Some(child) => child,
            None => return Err("Süren bir kayıt yok.".to_string()),
        };
        let target = match target {
            Some(target) => target,
            None => return Err("Kayıt yolu bulunamadı.".to_string()),
        };

        let mut child = child;
        terminate(&mut child, 8000);

        if file_size(&target) < 2048 {
            let _ = std::fs::remove_file(&target);
            return Err("Kayıt sonuçlandırılamadı (dosya oluşmadı).".to_string());
        }

        let duration = if started > 0.0 {
            Some((now() - started).max(0.5))
        } else {
            None
        };
        let title = auto_title(game.as_deref());
        let clip = self
            .lib
            .create(&target, &title, game.as_deref().unwrap_or(""), "kayıt", duration)?;
        self.log
            .info(&format!("Kayıt tamamlandı: {}", clip.meta.id));
        Ok(clip)
    }

    // ------------------------------------------------------------------ bakım
    pub fn prune(&self) {
        let (buffer_dir, seg_seconds) = match self.inner.lock() {
            Ok(inner) => (inner.buffer_dir.clone(), inner.seg_seconds),
            Err(_) => return,
        };
        let buffer_dir = match buffer_dir {
            Some(dir) => dir,
            None => return,
        };
        if !buffer_dir.exists() {
            return;
        }

        let config = self.config_snapshot();
        let keep_seconds = config.capture.buffer_seconds as f64 + 8.0;
        let max_files = (keep_seconds / seg_seconds.max(1) as f64).ceil() as usize + 4;

        let segments = list_segments(&buffer_dir);
        if segments.len() > max_files {
            let drop_count = segments.len() - max_files;
            for (_, path) in segments.iter().take(drop_count) {
                let _ = std::fs::remove_file(path);
            }
        }

        if self.is_running() {
            if let Some(free) = available_bytes(&buffer_dir) {
                if free < 1_500_000_000 {
                    self.log.error(&format!(
                        "Disk alanı azaldı ({:.1} GB), kayıt durduruluyor.",
                        free as f64 / 1e9
                    ));
                    if let Ok(mut inner) = self.inner.lock() {
                        inner.last_error = Some("Disk alanı azaldı, kayıt durduruldu.".to_string());
                    }
                    let _ = self.stop();
                }
            }
        }
    }

    pub fn shutdown(&self) {
        if self.is_recording() {
            let _ = self.stop_recording(None);
        }
        self.stop_locked();
        let dir = cache_dir().join("buffer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn error_tail(&self) -> String {
        match self.stderr_tail.lock() {
            Ok(tail) => {
                let lines: Vec<String> = tail
                    .iter()
                    .filter(|line| !line.trim().is_empty())
                    .cloned()
                    .collect();
                if lines.is_empty() {
                    return String::new();
                }
                let take = lines.len().saturating_sub(4);
                lines[take..].join(" | ").chars().take(600).collect()
            }
            Err(_) => String::new(),
        }
    }
}

fn auto_title(game: Option<&str>) -> String {
    let stamp = crate::util::short_stamp();
    match game {
        Some(name) if !name.trim().is_empty() => format!("{} — {}", name, stamp),
        _ => format!("Klip — {}", stamp),
    }
}

fn runtime_dir() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(value) if !value.is_empty() => value,
        _ => format!("/run/user/{}", unsafe { libc::getuid() }),
    }
}

fn pump_stderr(
    tail: Arc<Mutex<VecDeque<String>>>,
    log: Arc<Logger>,
    stream: Option<std::process::ChildStderr>,
    label: String,
) {
    let stream = match stream {
        Some(stream) => stream,
        None => return,
    };
    std::thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let text = match line {
                Ok(text) => text,
                Err(_) => break,
            };
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(mut guard) = tail.lock() {
                if guard.len() >= 40 {
                    guard.pop_front();
                }
                guard.push_back(trimmed.clone());
            }
            log.info(&format!("[{}] {}", label, trimmed));
        }
    });
}

fn terminate(child: &mut Child, grace_ms: u64) {
    let pid = child.id() as libc::pid_t;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let steps = (grace_ms / 50).max(1);
    for _ in 0..steps {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn list_segments(dir: &Path) -> Vec<(u64, PathBuf)> {
    let mut items: Vec<(u64, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("seg_") || !name.ends_with(".mp4") {
                continue;
            }
            let number: u64 = name
                .trim_start_matches("seg_")
                .trim_end_matches(".mp4")
                .parse()
                .unwrap_or(0);
            items.push((number, entry.path()));
        }
    }
    items.sort_by_key(|(number, _)| *number);
    items
}

/// Bir mp4 dosyasının sonlandırılmış olup olmadığını kontrol eder: mp4 muxer
/// `moov` kutusunu dosyanın sonuna yazar, yani hâlâ yazılmakta olan segmentte
/// bu kutu bulunmaz ve concat demuxer'ı dosyayı açamaz.
fn is_finalized_mp4(path: &Path) -> bool {
    let size = file_size(path);
    if size < 4096 {
        return false;
    }
    let window = std::cmp::min(size, 512 * 1024) as usize;
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    if file.seek(SeekFrom::Start(size - window as u64)).is_err() {
        return false;
    }
    let mut buffer = vec![0u8; window];
    let mut filled = 0usize;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(_) => break,
        }
    }
    buffer[..filled].windows(4).any(|chunk| chunk == b"moov")
}

/// `df` ile boş alanı bayt cinsinden döner.
fn available_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("df")
        .arg("-B1")
        .arg("--output=avail")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    for line in text.lines().skip(1) {
        if let Ok(value) = line.trim().parse::<u64>() {
            return Some(value);
        }
    }
    None
}
