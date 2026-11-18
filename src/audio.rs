//! PulseAudio / PipeWire ses kaynakları.

use std::sync::Mutex;

use crate::ffmpeg::run;
use crate::util::{now, which};

#[derive(Clone, Debug, PartialEq)]
pub enum SourceKind {
    Monitor,
    Input,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub name: String,
    pub label: String,
    pub kind: SourceKind,
}

pub struct AudioStatus {
    pub available: bool,
    pub error: Option<String>,
    pub sources: Vec<Source>,
}

struct Inner {
    at: f64,
    sources: Vec<Source>,
    error: Option<String>,
}

pub struct AudioSources {
    inner: Mutex<Inner>,
}

impl AudioSources {
    pub fn new() -> AudioSources {
        AudioSources {
            inner: Mutex::new(Inner {
                at: 0.0,
                sources: Vec::new(),
                error: None,
            }),
        }
    }

    pub fn available(&self) -> bool {
        which("pactl").is_some()
    }

    pub fn sources(&self, refresh: bool) -> Vec<Source> {
        if !refresh {
            if let Ok(inner) = self.inner.lock() {
                if !inner.sources.is_empty() && now() - inner.at < 8.0 {
                    return inner.sources.clone();
                }
            }
        }

        let (sources, error) = self.query();
        if let Ok(mut inner) = self.inner.lock() {
            inner.at = now();
            inner.sources = sources.clone();
            inner.error = error;
        }
        sources
    }

    fn query(&self) -> (Vec<Source>, Option<String>) {
        if !self.available() {
            return (
                Vec::new(),
                Some("pactl bulunamadı (pulseaudio-utils paketi gerekli)".to_string()),
            );
        }
        let out = run(&[
            "pactl".to_string(),
            "list".to_string(),
            "short".to_string(),
            "sources".to_string(),
        ]);
        if out.0 != 0 {
            let message = if out.2.trim().is_empty() {
                "pactl çalıştırılamadı".to_string()
            } else {
                out.2.trim().chars().take(200).collect()
            };
            return (Vec::new(), Some(message));
        }

        let mut sources = Vec::new();
        for line in out.1.lines() {
            let fields: Vec<&str> = if line.contains('\t') {
                line.split('\t').collect()
            } else {
                line.split_whitespace().collect()
            };
            if fields.len() < 2 {
                continue;
            }
            let name = fields[1].trim().to_string();
            if name.is_empty() {
                continue;
            }
            let kind = if name.ends_with(".monitor") {
                SourceKind::Monitor
            } else {
                SourceKind::Input
            };
            sources.push(Source {
                label: pretty(&name),
                name,
                kind,
            });
        }
        (sources, None)
    }

    /// (oyun kaynağı, mikrofon kaynağı)
    pub fn resolve(&self, game_pref: &str, mic_pref: &str) -> (Option<String>, Option<String>) {
        let sources = self.sources(false);
        let monitors: Vec<&Source> = sources
            .iter()
            .filter(|source| source.kind == SourceKind::Monitor)
            .collect();
        let inputs: Vec<&Source> = sources
            .iter()
            .filter(|source| source.kind == SourceKind::Input)
            .collect();

        let pref = game_pref.trim();
        let game = if pref == "none" {
            None
        } else if pref.is_empty() || pref == "auto" {
            monitors.first().map(|source| source.name.clone())
        } else {
            sources
                .iter()
                .find(|source| source.name == pref)
                .map(|source| source.name.clone())
                .or_else(|| monitors.first().map(|source| source.name.clone()))
        };

        let mic_name = mic_pref.trim();
        let mic = if mic_name.is_empty() || mic_name == "none" || mic_name == "off" {
            None
        } else if mic_name == "auto" {
            inputs.first().map(|source| source.name.clone())
        } else {
            sources
                .iter()
                .find(|source| source.name == mic_name)
                .map(|source| source.name.clone())
        };

        (game, mic)
    }

    pub fn status(&self) -> AudioStatus {
        let sources = self.sources(false);
        let error = match self.inner.lock() {
            Ok(inner) => inner.error.clone(),
            Err(_) => None,
        };
        AudioStatus {
            available: self.available(),
            error,
            sources,
        }
    }
}

fn pretty(name: &str) -> String {
    let mut base = name.to_string();
    let is_monitor = base.ends_with(".monitor");
    if is_monitor {
        base.truncate(base.len() - ".monitor".len());
    }
    if let Some(index) = base.find('.') {
        base = base[index + 1..].to_string();
    }
    let cleaned: String = base
        .chars()
        .map(|ch| if ch == '_' || ch == '-' { ' ' } else { ch })
        .collect();
    let mut label = String::new();
    let mut last_space = false;
    for ch in cleaned.chars() {
        if ch == ' ' {
            if !last_space && !label.is_empty() {
                label.push(' ');
            }
            last_space = true;
        } else {
            label.push(ch);
            last_space = false;
        }
    }
    let label = label.trim().to_string();
    let label = if label.is_empty() {
        name.to_string()
    } else {
        label
    };
    if is_monitor {
        format!("{} (sistem sesi)", label)
    } else {
        label
    }
}
