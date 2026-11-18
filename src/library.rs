//! Klip kütüphanesi: meta veri, küçük resim, kırpma, disk temizliği.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::ffmpeg::{file_size, Ffmpeg};
use crate::util::{human_bytes, human_duration, id_stamp, now, random_token, slug, Logger};

pub const VIDEO_NAME: &str = "clip.mp4";
pub const THUMB_NAME: &str = "thumb.jpg";
pub const STRIP_NAME: &str = "strip.jpg";
pub const META_NAME: &str = "meta.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub slug: String,
}

impl ClipMeta {
    pub fn duration_text(&self) -> String {
        human_duration(self.duration)
    }

    pub fn size_text(&self) -> String {
        human_bytes(self.size)
    }

    pub fn resolution_text(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{}×{}", w, h),
            _ => String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub meta: ClipMeta,
    pub dir: PathBuf,
}

impl Clip {
    pub fn id(&self) -> String {
        self.meta.id.clone()
    }

    pub fn video(&self) -> PathBuf {
        let named = self.dir.join(&self.meta.file);
        if !self.meta.file.is_empty() && named.exists() {
            return named;
        }
        self.dir.join(VIDEO_NAME)
    }

    pub fn thumb(&self) -> PathBuf {
        self.dir.join(THUMB_NAME)
    }

    pub fn strip(&self) -> PathBuf {
        self.dir.join(STRIP_NAME)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub count: usize,
    pub size: u64,
    pub duration: f64,
    pub favorites: usize,
    pub games: Vec<(String, usize)>,
    pub root: String,
}

pub struct Library {
    pub cfg: Arc<Mutex<Config>>,
    pub log: Arc<Logger>,
    pub ff: Arc<Ffmpeg>,
}

impl Library {
    pub fn new(cfg: Arc<Mutex<Config>>, log: Arc<Logger>, ff: Arc<Ffmpeg>) -> Library {
        Library { cfg, log, ff }
    }

    pub fn root(&self) -> PathBuf {
        match self.cfg.lock() {
            Ok(config) => config.library_dir(),
            Err(_) => PathBuf::from("/tmp/limedal"),
        }
    }

    pub fn tmp_dir(&self) -> PathBuf {
        let path = self.root().join(".tmp");
        if std::fs::create_dir_all(&path).is_err() {
            let fallback = std::path::PathBuf::from("/tmp/limedal-tmp");
            let _ = std::fs::create_dir_all(&fallback);
            return fallback;
        }
        path
    }

    pub fn clip_dir(&self, id: &str) -> PathBuf {
        self.root().join(id)
    }

    fn read_meta(&self, dir: &Path) -> Option<ClipMeta> {
        let text = std::fs::read_to_string(dir.join(META_NAME)).ok()?;
        let mut meta: ClipMeta = serde_json::from_str(&text).ok()?;
        let id = dir.file_name()?.to_string_lossy().to_string();
        if meta.id.is_empty() {
            meta.id = id;
        }
        let clip = Clip {
            meta: meta.clone(),
            dir: dir.to_path_buf(),
        };
        let video = clip.video();
        if !video.exists() {
            return None;
        }
        meta.size = file_size(&video);
        if meta.file.is_empty() {
            meta.file = video
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| VIDEO_NAME.to_string());
        }
        Some(meta)
    }

    fn write_meta(&self, dir: &Path, meta: &ClipMeta) -> Result<(), String> {
        let text = serde_json::to_string_pretty(meta).map_err(|err| err.to_string())?;
        let target = dir.join(META_NAME);
        let tmp = dir.join("meta.json.tmp");
        std::fs::write(&tmp, text).map_err(|err| format!("meta.json yazılamadı: {}", err))?;
        std::fs::rename(&tmp, &target).map_err(|err| format!("meta.json yazılamadı: {}", err))?;
        Ok(())
    }

    pub fn list(&self, query: &str, sort: &str) -> Vec<Clip> {
        let root = self.root();
        let mut clips: Vec<Clip> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if let Some(meta) = self.read_meta(&path) {
                    clips.push(Clip { meta, dir: path });
                }
            }
        }

        let needle = query.trim().to_lowercase();
        if !needle.is_empty() {
            clips.retain(|clip| {
                let haystack = format!("{} {}", clip.meta.title, clip.meta.game).to_lowercase();
                needle.split_whitespace().all(|word| haystack.contains(word))
            });
        }

        match sort {
            "oldest" => clips.sort_by(|a, b| a.meta.created_at.partial_cmp(&b.meta.created_at).unwrap_or(std::cmp::Ordering::Equal)),
            "duration" => clips.sort_by(|a, b| b.meta.duration.partial_cmp(&a.meta.duration).unwrap_or(std::cmp::Ordering::Equal)),
            "size" => clips.sort_by(|a, b| b.meta.size.cmp(&a.meta.size)),
            "title" => clips.sort_by(|a, b| a.meta.title.to_lowercase().cmp(&b.meta.title.to_lowercase())),
            _ => clips.sort_by(|a, b| b.meta.created_at.partial_cmp(&a.meta.created_at).unwrap_or(std::cmp::Ordering::Equal)),
        }
        clips
    }

    pub fn get(&self, id: &str) -> Option<Clip> {
        if id.is_empty() || id.contains('/') || id.contains("..") {
            return None;
        }
        let dir = self.clip_dir(id);
        let meta = self.read_meta(&dir)?;
        Some(Clip { meta, dir })
    }

    pub fn create(
        &self,
        src: &Path,
        title: &str,
        game: &str,
        source: &str,
        duration: Option<f64>,
    ) -> Result<Clip, String> {
        let size = file_size(src);
        if size < 2048 {
            return Err("Kaynak dosya geçersiz ya da çok küçük.".to_string());
        }

        let id = format!("{}-{}", id_stamp(), random_token(4).to_lowercase());
        let dir = self.clip_dir(&id);
        std::fs::create_dir_all(&dir).map_err(|err| format!("Klasör oluşturulamadı: {}", err))?;
        let target = dir.join(VIDEO_NAME);
        if std::fs::rename(src, &target).is_err() {
            if std::fs::copy(src, &target).is_err() {
                let _ = std::fs::remove_dir_all(&dir);
                return Err("Dosya kütüphaneye taşınamadı.".to_string());
            }
            let _ = std::fs::remove_file(src);
        }

        let (probed, width, height) = self.ff.probe_media(&target);
        let final_duration = duration.filter(|value| *value > 0.5).or(probed).unwrap_or(0.0);
        let created = now();
        let mut meta = ClipMeta {
            id: id.clone(),
            title: truncate(title, 120),
            game: truncate(game, 80),
            created_at: created,
            duration: (final_duration * 100.0).round() / 100.0,
            width,
            height,
            favorite: false,
            source: source.to_string(),
            token: random_token(14),
            size: file_size(&target),
            file: VIDEO_NAME.to_string(),
            slug: slug(title),
        };
        if meta.title.trim().is_empty() {
            meta.title = "Adsız klip".to_string();
        }
        self.write_meta(&dir, &meta)?;
        self.make_thumb(&dir, &target, meta.duration);
        self.enforce_limit();
        Ok(Clip { meta, dir })
    }

    fn make_thumb(&self, dir: &Path, video: &Path, duration: f64) {
        let thumb = dir.join(THUMB_NAME);
        let base = if duration > 0.0 { duration } else { 2.0 };
        let candidates = [
            (base * 0.35).clamp(0.5, 6.0),
            0.3,
            (base - 0.5).max(0.1),
        ];
        for candidate in candidates {
            if self.ff.thumbnail(video, &thumb, candidate) {
                return;
            }
        }
        self.log.warn(&format!("küçük resim üretilemedi: {}", dir.display()));
    }

    /// Film şeridi yoksa üretir.
    pub fn ensure_strip(&self, clip: &Clip) -> Option<PathBuf> {
        let strip = clip.strip();
        if strip.exists() {
            return Some(strip);
        }
        let video = clip.video();
        if !video.exists() {
            return None;
        }
        if self.ff.filmstrip(&video, &strip) {
            Some(strip)
        } else {
            None
        }
    }

    pub fn set_title(&self, id: &str, title: &str) -> Option<Clip> {
        let mut clip = self.get(id)?;
        let cleaned = truncate(title, 120);
        if cleaned.trim().is_empty() {
            return Some(clip);
        }
        clip.meta.title = cleaned.clone();
        clip.meta.slug = slug(&cleaned);
        let _ = self.write_meta(&clip.dir, &clip.meta);
        Some(clip)
    }

    pub fn set_game(&self, id: &str, game: &str) -> Option<Clip> {
        let mut clip = self.get(id)?;
        clip.meta.game = truncate(game, 80);
        let _ = self.write_meta(&clip.dir, &clip.meta);
        Some(clip)
    }

    pub fn toggle_favorite(&self, id: &str) -> Option<Clip> {
        let mut clip = self.get(id)?;
        clip.meta.favorite = !clip.meta.favorite;
        let _ = self.write_meta(&clip.dir, &clip.meta);
        Some(clip)
    }

    pub fn delete(&self, id: &str) -> bool {
        let clip = match self.get(id) {
            Some(clip) => clip,
            None => return false,
        };
        let _ = std::fs::remove_dir_all(&clip.dir);
        let gone = !clip.dir.exists();
        if gone {
            self.log.info(&format!("Klip silindi: {}", id));
        }
        gone
    }

    pub fn trim(&self, id: &str, start: f64, end: f64) -> Result<Clip, String> {
        let clip = self.get(id).ok_or_else(|| "Klip bulunamadı".to_string())?;
        let video = clip.video();
        if !video.exists() {
            return Err("Klip dosyası yok.".to_string());
        }
        let duration = clip.meta.duration;
        let start = start.max(0.0);
        let end = if end > 0.0 {
            if duration > 0.0 {
                end.min(duration)
            } else {
                end
            }
        } else {
            duration
        };
        if end - start < 0.5 {
            return Err("Seçilen aralık çok kısa (en az 0,5 saniye).".to_string());
        }

        let out = self
            .tmp_dir()
            .join(format!("{}-trim-{}.mp4", id, random_token(4)));
        if let Err(err) = self.ff.trim(&video, &out, None, Some(start), Some(end - start)) {
            let _ = std::fs::remove_file(&out);
            return Err(format!("Kırpma başarısız: {}", err));
        }
        let title = format!("{} (kırpılmış)", clip.meta.title);
        let result = self.create(&out, &title, &clip.meta.game, "kırpma", None);
        let _ = std::fs::remove_file(&out);
        result
    }

    pub fn by_token(&self, token: &str) -> Option<Clip> {
        if token.is_empty() {
            return None;
        }
        self.list("", "newest")
            .into_iter()
            .find(|clip| clip.meta.token == token)
    }

    pub fn stats(&self) -> Stats {
        let clips = self.list("", "newest");
        let mut stats = Stats {
            count: clips.len(),
            root: self.root().to_string_lossy().to_string(),
            ..Stats::default()
        };
        let mut games: Vec<(String, usize)> = Vec::new();
        for clip in &clips {
            stats.size += clip.meta.size;
            stats.duration += clip.meta.duration;
            if clip.meta.favorite {
                stats.favorites += 1;
            }
            let name = if clip.meta.game.trim().is_empty() {
                "Diğer".to_string()
            } else {
                clip.meta.game.clone()
            };
            match games.iter_mut().find(|(key, _)| *key == name) {
                Some((_, count)) => *count += 1,
                None => games.push((name, 1)),
            }
        }
        games.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        stats.games = games;
        stats
    }

    pub fn enforce_limit(&self) -> usize {
        let limit_gb = match self.cfg.lock() {
            Ok(config) => config.app.max_storage_gb as u64,
            Err(_) => 0,
        };
        if limit_gb == 0 {
            return 0;
        }
        let limit = limit_gb * 1024 * 1024 * 1024;
        let mut clips = self.list("", "oldest");
        let mut total: u64 = clips.iter().map(|clip| clip.meta.size).sum();
        let mut removed = 0;
        for clip in clips.drain(..) {
            if total <= limit {
                break;
            }
            if clip.meta.favorite {
                continue;
            }
            let size = clip.meta.size;
            if self.delete(&clip.meta.id) {
                total = total.saturating_sub(size);
                removed += 1;
            }
        }
        if removed > 0 {
            self.log.info(&format!(
                "Depolama sınırı: {} klip silindi ({} GB)",
                removed, limit_gb
            ));
        }
        removed
    }

    pub fn cleanup_tmp(&self, older_than_seconds: f64) {
        let dir = self.tmp_dir();
        let cutoff = now() - older_than_seconds;
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|meta| meta.modified().ok())
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs_f64())
                    .unwrap_or(0.0);
                if modified > 0.0 && modified < cutoff {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    trimmed.chars().take(limit).collect()
}
