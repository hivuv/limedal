//! Yollar, loglama ve küçük yardımcılar.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn home() -> PathBuf {
    match std::env::var("HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from("/tmp"),
    }
}

fn xdg(var: &str, fallback: PathBuf) -> PathBuf {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => fallback,
    }
}

pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", home().join(".config")).join("limedal")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn cache_dir() -> PathBuf {
    let path = xdg("XDG_CACHE_HOME", home().join(".cache")).join("limedal");
    let _ = std::fs::create_dir_all(&path);
    path
}

pub fn state_dir() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let path = if !runtime.is_empty() && std::path::Path::new(&runtime).is_dir() {
        PathBuf::from(runtime).join("limedal")
    } else {
        cache_dir().join("run")
    };
    let _ = std::fs::create_dir_all(&path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
    }
    path
}

/// "~/Videos/LiMedal" gibi yolları genişletir.
pub fn expand(path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return home().join("Videos").join("LiMedal");
    }
    if trimmed == "~" {
        return home();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(trimmed)
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(name);
        if let Ok(meta) = std::fs::metadata(&candidate) {
            if meta.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn human_bytes(bytes: u64) -> String {
    let mut value = bytes as f64;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut index = 0;
    while value >= 1024.0 && index < units.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{} B", bytes)
    } else if value >= 100.0 {
        format!("{:.0} {}", value, units[index])
    } else {
        format!("{:.1} {}", value, units[index])
    }
}

pub fn human_duration(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.round() as u64
    } else {
        0
    };
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

pub fn time_ago(created_at: f64) -> String {
    if created_at <= 0.0 {
        return String::new();
    }
    let diff = (now() - created_at).max(0.0);
    if diff < 60.0 {
        "az önce".to_string()
    } else if diff < 3600.0 {
        format!("{} dk önce", (diff / 60.0) as u64)
    } else if diff < 86400.0 {
        format!("{} saat önce", (diff / 3600.0) as u64)
    } else if diff < 604800.0 {
        format!("{} gün önce", (diff / 86400.0) as u64)
    } else {
        format!("{} gün önce", (diff / 86400.0) as u64)
    }
}

pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let mapped = match ch {
            'ı' | 'İ' => 'i',
            'ş' | 'Ş' => 's',
            'ğ' | 'Ğ' => 'g',
            'ü' | 'Ü' => 'u',
            'ö' | 'Ö' => 'o',
            'ç' | 'Ç' => 'c',
            other => other,
        };
        if mapped.is_ascii_alphanumeric() {
            out.push(mapped.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "klip".to_string()
    } else {
        trimmed
    }
}

/// Tahmin edilmesi zor, dosya/URL güvenli token üretir.
pub fn random_token(len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::with_capacity(len);
    let mut counter = now().to_bits() ^ (std::process::id() as u64);
    while out.len() < len {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(counter);
        hasher.write_u32(std::process::id());
        counter = counter
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let value = hasher.finish();
        for shift in 0..8 {
            if out.len() >= len {
                break;
            }
            let byte = ((value >> (shift * 8)) & 0xff) as usize;
            out.push(ALPHABET[byte % ALPHABET.len()] as char);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Loglama
// ---------------------------------------------------------------------------

struct LogInner {
    file: Option<File>,
    ring: VecDeque<String>,
}

pub struct Logger {
    path: PathBuf,
    inner: Mutex<LogInner>,
}

impl Logger {
    pub fn new(path: PathBuf) -> Logger {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > 2_000_000 {
                let rotated = path.with_extension("log.1");
                let _ = std::fs::remove_file(&rotated);
                let _ = std::fs::rename(&path, &rotated);
            }
        }
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        Logger {
            path,
            inner: Mutex::new(LogInner {
                file,
                ring: VecDeque::with_capacity(400),
            }),
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn log(&self, level: &str, message: &str) {
        let stamp = local_stamp();
        let line = format!("{} [{}] {}", stamp, level, message);
        if std::env::var("LIMEDAL_DEBUG").is_ok() {
            eprintln!("{}", line);
        }
        if let Ok(mut inner) = self.inner.lock() {
            if inner.ring.len() >= 600 {
                inner.ring.pop_front();
            }
            inner.ring.push_back(line.clone());
            if let Some(file) = inner.file.as_mut() {
                let _ = writeln!(file, "{}", line);
                let _ = file.flush();
            }
        }
    }

    pub fn info(&self, message: &str) {
        self.log("INFO", message);
    }

    pub fn warn(&self, message: &str) {
        self.log("WARN", message);
    }

    pub fn error(&self, message: &str) {
        self.log("ERROR", message);
    }

    pub fn tail(&self, count: usize) -> Vec<String> {
        match self.inner.lock() {
            Ok(inner) => {
                let skip = inner.ring.len().saturating_sub(count);
                inner.ring.iter().skip(skip).cloned().collect()
            }
            Err(_) => Vec::new(),
        }
    }
}

/// Yerel saat bileşenleri: (yıl, ay, gün, saat, dakika, saniye).
pub fn local_parts() -> (i32, u32, u32, u32, u32, u32) {
    unsafe {
        let stamp = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&stamp, &mut tm).is_null() {
            let secs = now() as i64;
            let days = secs.div_euclid(86_400);
            let rem = secs.rem_euclid(86_400);
            return (
                1970 + (days / 365) as i32,
                1,
                1,
                (rem / 3600) as u32,
                ((rem % 3600) / 60) as u32,
                (rem % 60) as u32,
            );
        }
        (
            tm.tm_year + 1900,
            (tm.tm_mon + 1) as u32,
            tm.tm_mday as u32,
            tm.tm_hour as u32,
            tm.tm_min as u32,
            tm.tm_sec as u32,
        )
    }
}

/// "2026-01-02 13:45:01" biçiminde yerel saat damgası.
pub fn local_stamp() -> String {
    let (year, month, day, hour, minute, second) = local_parts();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second
    )
}

/// "13:45:01" biçiminde saat.
pub fn clock_stamp() -> String {
    let (_, _, _, hour, minute, second) = local_parts();
    format!("{:02}:{:02}:{:02}", hour, minute, second)
}

/// "02.01.2026 13:45" biçiminde kısa tarih.
pub fn short_stamp() -> String {
    let (year, month, day, hour, minute, _) = local_parts();
    format!("{:02}.{:02}.{} {:02}:{:02}", day, month, year, hour, minute)
}

/// Klasör/dosya adı için "20260102-134501".
pub fn id_stamp() -> String {
    let (year, month, day, hour, minute, second) = local_parts();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year, month, day, hour, minute, second
    )
}

