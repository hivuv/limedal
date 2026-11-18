//! FFmpeg keşfi, kodlayıcı seçimi ve komut üretimi.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::util::which;

pub const ENCODER_ORDER: [&str; 4] = ["h264_nvenc", "h264_vaapi", "h264_qsv", "libx264"];

pub fn encoder_label(name: &str) -> &'static str {
    match name {
        "h264_nvenc" => "NVIDIA NVENC",
        "h264_vaapi" => "VA-API (AMD / Intel)",
        "h264_qsv" => "Intel Quick Sync",
        "libx264" => "CPU (libx264)",
        "auto" => "Otomatik",
        _ => "bilinmiyor",
    }
}

pub struct Preset {
    pub vbr: u32,
    pub maxrate: u32,
    pub x264_preset: &'static str,
    pub nvenc_preset: &'static str,
    pub qsv_preset: &'static str,
}

pub fn preset(quality: &str) -> Preset {
    match quality {
        "low" => Preset {
            vbr: 6,
            maxrate: 9,
            x264_preset: "ultrafast",
            nvenc_preset: "p2",
            qsv_preset: "veryfast",
        },
        "ultra" => Preset {
            vbr: 35,
            maxrate: 50,
            x264_preset: "faster",
            nvenc_preset: "p5",
            qsv_preset: "fast",
        },
        "medium" => Preset {
            vbr: 12,
            maxrate: 18,
            x264_preset: "veryfast",
            nvenc_preset: "p3",
            qsv_preset: "faster",
        },
        _ => Preset {
            vbr: 20,
            maxrate: 30,
            x264_preset: "veryfast",
            nvenc_preset: "p4",
            qsv_preset: "faster",
        },
    }
}

/// Komutu çalıştırır: (dönüş kodu, stdout, stderr)
pub fn run(cmd: &[String]) -> (i32, String, String) {
    if cmd.is_empty() {
        return (2, String::new(), "boş komut".to_string());
    }
    match Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(out) => (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        ),
        Err(err) => (127, String::new(), format!("{}: {}", cmd[0], err)),
    }
}

fn s(value: &str) -> String {
    value.to_string()
}

struct ResolveCache {
    key: String,
    encoder: String,
    vf: Option<String>,
    note: String,
}

pub struct Ffmpeg {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    encoders: Mutex<Option<Vec<String>>>,
    cache: Mutex<Option<ResolveCache>>,
}

impl Ffmpeg {
    pub fn new() -> Ffmpeg {
        Ffmpeg {
            ffmpeg: which("ffmpeg"),
            ffprobe: which("ffprobe"),
            encoders: Mutex::new(None),
            cache: Mutex::new(None),
        }
    }

    pub fn available(&self) -> bool {
        self.ffmpeg.is_some()
    }

    pub fn path(&self) -> String {
        match &self.ffmpeg {
            Some(path) => path.to_string_lossy().to_string(),
            None => "ffmpeg".to_string(),
        }
    }

    pub fn encoders(&self) -> Vec<String> {
        if let Ok(guard) = self.encoders.lock() {
            if let Some(list) = guard.as_ref() {
                return list.clone();
            }
        }
        let mut found: Vec<String> = Vec::new();
        if self.ffmpeg.is_some() {
            let out = run(&[self.path(), s("-hide_banner"), s("-encoders")]);
            for line in out.1.lines().chain(out.2.lines()) {
                let trimmed = line.trim_start();
                let mut parts = trimmed.split_whitespace();
                let flags = match parts.next() {
                    Some(value) => value,
                    None => continue,
                };
                if flags.len() != 6 || !flags.starts_with('V') {
                    continue;
                }
                if !flags.chars().all(|c| c == '.' || c.is_ascii_uppercase()) {
                    continue;
                }
                if let Some(name) = parts.next() {
                    if !found.iter().any(|existing| existing == name) {
                        found.push(name.to_string());
                    }
                }
            }
        }
        if let Ok(mut guard) = self.encoders.lock() {
            *guard = Some(found.clone());
        }
        found
    }

    pub fn has_encoder(&self, name: &str) -> bool {
        self.encoders().iter().any(|item| item == name)
    }

    pub fn vaapi_device(&self) -> Option<String> {
        for candidate in ["/dev/dri/renderD128", "/dev/dri/renderD129", "/dev/dri/renderD130"] {
            if Path::new(candidate).exists() {
                return Some(candidate.to_string());
            }
        }
        if let Ok(entries) = std::fs::read_dir("/dev/dri") {
            let mut names: Vec<String> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .filter(|name| name.starts_with("renderD"))
                .collect();
            names.sort();
            if let Some(first) = names.first() {
                return Some(format!("/dev/dri/{}", first));
            }
        }
        None
    }

    pub fn best_encoder(&self, preference: &str) -> String {
        let available = self.encoders();
        if preference != "auto" && available.iter().any(|item| item == preference) {
            if preference != "h264_vaapi" || self.vaapi_device().is_some() {
                return preference.to_string();
            }
        }
        for candidate in ENCODER_ORDER {
            if available.iter().any(|item| item == candidate) {
                if candidate == "h264_vaapi" && self.vaapi_device().is_none() {
                    continue;
                }
                return candidate.to_string();
            }
        }
        "libx264".to_string()
    }

    /// Ekran çözünürlüğü: (genişlik, yükseklik, kaynak)
    pub fn display_resolution(&self, display: &str) -> (u32, u32, String) {
        let out = run(&[s("xrandr"), s("--current")]);
        if out.0 == 0 && !out.1.is_empty() {
            let mut primary: Option<(u32, u32)> = None;
            let mut fallback: Option<(u32, u32)> = None;
            for line in out.1.lines() {
                if !line.contains(" connected") {
                    continue;
                }
                if let Some(size) = parse_geometry(line) {
                    if line.contains(" primary ") && primary.is_none() {
                        primary = Some(size);
                    } else if fallback.is_none() {
                        fallback = Some(size);
                    }
                }
            }
            if let Some((w, h)) = primary.or(fallback) {
                return (w, h, "xrandr".to_string());
            }
        }

        let out = Command::new("xdpyinfo")
            .env("DISPLAY", display.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        if let Ok(output) = out {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("dimensions:") {
                    if let Some(size) = parse_geometry(&format!("{}+0", rest.trim())) {
                        return (size.0, size.1, "xdpyinfo".to_string());
                    }
                }
            }
        }
        (1920, 1080, "varsayılan".to_string())
    }

    /// (süre, genişlik, yükseklik)
    pub fn probe_media(&self, path: &Path) -> (Option<f64>, Option<u32>, Option<u32>) {
        let mut duration: Option<f64> = None;
        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;

        if let Some(probe) = &self.ffprobe {
            let out = run(&[
                probe.to_string_lossy().to_string(),
                s("-v"),
                s("error"),
                s("-show_entries"),
                s("format=duration:stream=width,height"),
                s("-of"),
                s("default=noprint_wrappers=1"),
                path.to_string_lossy().to_string(),
            ]);
            if out.0 == 0 {
                for line in out.1.lines() {
                    if let Some(value) = line.strip_prefix("duration=") {
                        duration = value.trim().parse::<f64>().ok();
                    } else if let Some(value) = line.strip_prefix("width=") {
                        width = value.trim().parse::<u32>().ok();
                    } else if let Some(value) = line.strip_prefix("height=") {
                        height = value.trim().parse::<u32>().ok();
                    }
                }
            }
        }

        if duration.is_none() && self.ffmpeg.is_some() {
            let out = run(&[
                self.path(),
                s("-hide_banner"),
                s("-i"),
                path.to_string_lossy().to_string(),
            ]);
            let text = out.2;
            if let Some(index) = text.find("Duration: ") {
                let rest = &text[index + 10..];
                if let Some(comma) = rest.find(',') {
                    duration = parse_hms(rest[..comma].trim());
                }
            }
            if width.is_none() {
                for line in text.lines() {
                    if line.contains("Video:") {
                        if let Some(size) = parse_size(line) {
                            width = Some(size.0);
                            height = Some(size.1);
                        }
                        break;
                    }
                }
            }
        }

        (duration, width, height)
    }

    /// Kodlayıcıyı 1,2 saniyelik sentetik girdiyle dener.
    pub fn self_test(&self, encoder: &str, quality: &str, fps: u32) -> Result<(), String> {
        if self.ffmpeg.is_none() {
            return Err("ffmpeg bulunamadı".to_string());
        }
        let mut vf: Option<String> = None;
        let mut cmd: Vec<String> = vec![
            self.path(),
            s("-hide_banner"),
            s("-loglevel"),
            s("error"),
            s("-nostdin"),
        ];
        if encoder == "h264_vaapi" {
            match self.vaapi_device() {
                Some(device) => {
                    cmd.push(s("-vaapi_device"));
                    cmd.push(device);
                }
                None => return Err("VA-API aygıtı yok (/dev/dri/render*)".to_string()),
            }
            vf = Some("format=nv12,hwupload".to_string());
        }
        cmd.push(s("-f"));
        cmd.push(s("lavfi"));
        cmd.push(s("-i"));
        cmd.push(format!("testsrc2=size=640x360:rate={}", fps.max(15)));
        if let Some(filter) = vf {
            cmd.push(s("-vf"));
            cmd.push(filter);
        }
        cmd.extend(self.video_args(encoder, quality, fps, 2));
        cmd.push(s("-t"));
        cmd.push(s("1.2"));
        cmd.push(s("-f"));
        cmd.push(s("null"));
        cmd.push(s("-"));

        let out = run(&cmd);
        if out.0 == 0 {
            return Ok(());
        }
        let mut reason = String::new();
        for line in out.2.lines() {
            let lowered = line.trim().to_lowercase();
            if lowered.is_empty() {
                continue;
            }
            if lowered.contains("error") || lowered.contains("failed") || lowered.contains("cannot") {
                reason = line.trim().to_string();
                break;
            }
        }
        if reason.is_empty() {
            reason = out.2.trim().chars().take(300).collect();
        }
        if reason.is_empty() {
            reason = "bilinmeyen hata".to_string();
        }
        Err(reason)
    }

    /// (kodlayıcı, video filtresi, not)
    pub fn resolve(&self, preference: &str, quality: &str, fps: u32) -> (String, Option<String>, String) {
        let wanted = self.best_encoder(preference);
        let key = format!("{}|{}|{}", wanted, quality, fps);
        if let Ok(guard) = self.cache.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.key == key {
                    return (cached.encoder.clone(), cached.vf.clone(), cached.note.clone());
                }
            }
        }

        let (encoder, vf, note) = match self.self_test(&wanted, quality, fps) {
            Ok(()) => {
                let vf = if wanted == "h264_vaapi" {
                    Some("format=nv12,hwupload".to_string())
                } else {
                    None
                };
                (wanted.clone(), vf, String::new())
            }
            Err(err) => (
                "libx264".to_string(),
                None,
                format!(
                    "{} çalışmadı ({}); libx264 kullanılacak",
                    encoder_label(&wanted),
                    err
                ),
            ),
        };

        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some(ResolveCache {
                key,
                encoder: encoder.clone(),
                vf: vf.clone(),
                note: note.clone(),
            });
        }
        (encoder, vf, note)
    }

    pub fn video_args(&self, encoder: &str, quality: &str, fps: u32, segment_seconds: u32) -> Vec<String> {
        let p = preset(quality);
        let gop = (fps.max(1) as u64 * segment_seconds.max(1) as u64).max(1);
        let gop = gop.to_string();
        let vbr = format!("{}M", p.vbr);
        let maxrate = format!("{}M", p.maxrate);
        let bufsize = format!("{}M", p.vbr * 2);

        match encoder {
            "h264_nvenc" => vec![
                s("-c:v"),
                s("h264_nvenc"),
                s("-preset"),
                s(p.nvenc_preset),
                s("-tune"),
                s("hq"),
                s("-rc"),
                s("vbr"),
                s("-b:v"),
                vbr,
                s("-maxrate"),
                maxrate,
                s("-bufsize"),
                bufsize,
                s("-g"),
                gop,
                s("-forced-idr"),
                s("1"),
                s("-pix_fmt"),
                s("yuv420p"),
            ],
            "h264_vaapi" => vec![
                s("-c:v"),
                s("h264_vaapi"),
                s("-b:v"),
                vbr,
                s("-maxrate"),
                maxrate,
                s("-bufsize"),
                bufsize,
                s("-g"),
                gop,
                s("-bf"),
                s("0"),
            ],
            "h264_qsv" => vec![
                s("-c:v"),
                s("h264_qsv"),
                s("-preset"),
                s(p.qsv_preset),
                s("-b:v"),
                vbr,
                s("-maxrate"),
                maxrate,
                s("-bufsize"),
                bufsize,
                s("-g"),
                gop,
                s("-look_ahead"),
                s("0"),
                s("-pix_fmt"),
                s("nv12"),
            ],
            _ => vec![
                s("-c:v"),
                s("libx264"),
                s("-preset"),
                s(p.x264_preset),
                s("-tune"),
                s("zerolatency"),
                s("-profile:v"),
                s("high"),
                s("-b:v"),
                vbr,
                s("-maxrate"),
                maxrate,
                s("-bufsize"),
                bufsize,
                s("-g"),
                gop.clone(),
                s("-keyint_min"),
                gop.clone(),
                s("-sc_threshold"),
                s("0"),
                s("-pix_fmt"),
                s("yuv420p"),
            ],
        }
    }

    pub fn concat(&self, list_file: &Path, out_file: &Path) -> Result<(), String> {
        if self.ffmpeg.is_none() {
            return Err("ffmpeg bulunamadı".to_string());
        }
        let out = run(&[
            self.path(),
            s("-hide_banner"),
            s("-loglevel"),
            s("error"),
            s("-nostdin"),
            s("-f"),
            s("concat"),
            s("-safe"),
            s("0"),
            s("-i"),
            list_file.to_string_lossy().to_string(),
            s("-c"),
            s("copy"),
            s("-movflags"),
            s("+faststart"),
            s("-avoid_negative_ts"),
            s("make_zero"),
            s("-y"),
            out_file.to_string_lossy().to_string(),
        ]);
        if out.0 == 0 && file_size(out_file) > 1024 {
            return Ok(());
        }
        Err(short(&out.2, 400))
    }

    pub fn trim(
        &self,
        src: &Path,
        out_file: &Path,
        seconds_from_end: Option<f64>,
        start: Option<f64>,
        duration: Option<f64>,
    ) -> Result<(), String> {
        if self.ffmpeg.is_none() {
            return Err("ffmpeg bulunamadı".to_string());
        }
        let mut cmd: Vec<String> = vec![
            self.path(),
            s("-hide_banner"),
            s("-loglevel"),
            s("error"),
            s("-nostdin"),
        ];
        if let Some(value) = seconds_from_end {
            cmd.push(s("-sseof"));
            cmd.push(format!("-{:.0}", value.max(1.0)));
        }
        if let Some(value) = start {
            cmd.push(s("-ss"));
            cmd.push(format!("{:.3}", value));
        }
        cmd.push(s("-i"));
        cmd.push(src.to_string_lossy().to_string());
        if let Some(value) = duration {
            cmd.push(s("-t"));
            cmd.push(format!("{:.3}", value));
        }
        cmd.push(s("-c"));
        cmd.push(s("copy"));
        cmd.push(s("-movflags"));
        cmd.push(s("+faststart"));
        cmd.push(s("-avoid_negative_ts"));
        cmd.push(s("make_zero"));
        cmd.push(s("-y"));
        cmd.push(out_file.to_string_lossy().to_string());

        let out = run(&cmd);
        if out.0 == 0 && file_size(out_file) > 1024 {
            return Ok(());
        }
        Err(short(&out.2, 400))
    }

    pub fn thumbnail(&self, src: &Path, out_file: &Path, at: f64) -> bool {
        if self.ffmpeg.is_none() {
            return false;
        }
        let out = run(&[
            self.path(),
            s("-hide_banner"),
            s("-loglevel"),
            s("error"),
            s("-nostdin"),
            s("-ss"),
            format!("{:.2}", at.max(0.0)),
            s("-i"),
            src.to_string_lossy().to_string(),
            s("-frames:v"),
            s("1"),
            s("-vf"),
            s("scale=640:-2"),
            s("-q:v"),
            s("4"),
            s("-y"),
            out_file.to_string_lossy().to_string(),
        ]);
        out.0 == 0 && file_size(out_file) > 512
    }

    /// Klibin başından sonuna 5 karelik film şeridi üretir.
    pub fn filmstrip(&self, src: &Path, out_file: &Path) -> bool {
        if self.ffmpeg.is_none() {
            return false;
        }
        let out = run(&[
            self.path(),
            s("-hide_banner"),
            s("-loglevel"),
            s("error"),
            s("-nostdin"),
            s("-i"),
            src.to_string_lossy().to_string(),
            s("-vf"),
            s("fps=1/2,scale=260:-1,tile=5x1"),
            s("-frames:v"),
            s("1"),
            s("-q:v"),
            s("5"),
            s("-y"),
            out_file.to_string_lossy().to_string(),
        ]);
        out.0 == 0 && file_size(out_file) > 512
    }
}

pub fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

fn short(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "bilinmeyen hata".to_string();
    }
    trimmed.chars().take(limit).collect()
}

fn parse_hms(text: &str) -> Option<f64> {
    let mut parts = text.split(':');
    let hours: f64 = parts.next()?.trim().parse().ok()?;
    let minutes: f64 = parts.next()?.trim().parse().ok()?;
    let seconds: f64 = parts.next()?.trim().parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// "1920x1080 [SAR 1:1 DAR 16:9]" gibi metinlerden ölçü çıkarır.
pub fn parse_size(text: &str) -> Option<(u32, u32)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index >= bytes.len() || (bytes[index] != b'x' && bytes[index] != b'X') {
            continue;
        }
        let width_text = &text[start..index];
        let height_start = index + 1;
        let mut cursor = height_start;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == height_start {
            continue;
        }
        let width: u32 = match width_text.parse() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let height: u32 = match text[height_start..cursor].parse() {
            Ok(value) => value,
            Err(_) => continue,
        };
        if width >= 320 && height >= 240 {
            return Some((width, height));
        }
    }
    None
}

/// "1920x1080+0+0" veya "dimensions: 1920x1080 pixels" içinden ölçü çıkarır.
pub fn parse_geometry(text: &str) -> Option<(u32, u32)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let width_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let width_text = &text[width_start..index];
        if index < bytes.len() && (bytes[index] == b'x' || bytes[index] == b'X') {
            let height_start = index + 1;
            let mut cursor = height_start;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }
            if cursor > height_start && cursor < bytes.len() && bytes[cursor] == b'+' {
                let width: u32 = width_text.parse().ok()?;
                let height: u32 = text[height_start..cursor].parse().ok()?;
                if width >= 320 && height >= 240 {
                    return Some((width, height));
                }
            }
            index = cursor.max(index + 1);
        }
    }
    None
}
