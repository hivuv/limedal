//! CLI ile çalışan örnek arasındaki kontrol kanalı (unix soket).

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::core::Core;
use crate::util::state_dir;

pub fn path() -> PathBuf {
    state_dir().join("control.sock")
}

pub fn cleanup() {
    let _ = std::fs::remove_file(path());
}

pub fn is_running() -> bool {
    UnixStream::connect(path()).is_ok()
}

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("limedal-ipc".to_string())
        .spawn(move || serve(core))
        .ok();
}

fn serve(core: Arc<Core>) {
    let socket = path();
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(err) => {
            core.log
                .error(&format!("kontrol soketi açılamadı: {}", err));
            return;
        }
    };

    for incoming in listener.incoming() {
        if core.should_quit() {
            break;
        }
        let core_clone = core.clone();
        if let Ok(mut stream) = incoming {
            std::thread::spawn(move || {
                handle(&mut stream, core_clone);
            });
        }
    }
    cleanup();
}

fn handle(stream: &mut UnixStream, core: Arc<Core>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(240)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let line = match read_line(stream) {
        Some(line) => line,
        None => return,
    };
    let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    let response = dispatch(&core, &request);
    let text = serde_json::to_string(&response).unwrap_or_else(|_| "{\"ok\":false}".to_string());
    let _ = stream.write_all(text.as_bytes());
    let _ = stream.write_all(b"\n");
    let _ = stream.flush();
}

fn dispatch(core: &Arc<Core>, request: &Value) -> Value {
    let command = request
        .get("cmd")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    match command {
        "clip" => {
            let seconds = request
                .get("seconds")
                .and_then(|value| value.as_u64())
                .map(|value| value as u32);
            let title = request
                .get("title")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string());
            match core.save_clip_now(seconds, title) {
                Ok(clip) => json!({
                    "ok": true,
                    "id": clip.meta.id,
                    "title": clip.meta.title,
                    "duration": clip.meta.duration,
                    "path": clip.video().to_string_lossy(),
                }),
                Err(err) => json!({ "ok": false, "error": err }),
            }
        }
        "record_start" => match core.start_recording() {
            Ok(()) => json!({ "ok": true, "recording": true }),
            Err(err) => json!({ "ok": false, "error": err }),
        },
        "record_stop" => match core.stop_recording() {
            Ok(clip) => json!({
                "ok": true,
                "recording": false,
                "id": clip.meta.id,
                "title": clip.meta.title,
                "duration": clip.meta.duration,
                "path": clip.video().to_string_lossy(),
            }),
            Err(err) => json!({ "ok": false, "error": err }),
        },
        "record_toggle" => match core.toggle_record() {
            Ok(recording) => json!({ "ok": true, "recording": recording }),
            Err(err) => json!({ "ok": false, "error": err }),
        },
        "status" => status_json(core),
        "config" => json!({ "ok": true, "config": core.config() }),
        "show" => {
            core.request_focus();
            json!({ "ok": true })
        }
        "quit" => {
            core.request_quit();
            json!({ "ok": true })
        }
        _ => json!({ "ok": false, "error": "bilinmeyen komut" }),
    }
}

fn status_json(core: &Arc<Core>) -> Value {
    let status = core.status();
    let stats = core.stats();
    let hotkeys = core.hotkey_state();
    json!({
        "ok": true,
        "running": status.running,
        "recording": status.recording,
        "ready": status.ready,
        "buffer_seconds": status.buffer_seconds,
        "fill_seconds": status.fill_seconds,
        "recording_seconds": status.recording_seconds,
        "encoder": status.encoder_label,
        "resolution": status.resolution,
        "fps": status.fps,
        "segments": status.segments,
        "error": status.error,
        "note": status.note,
        "game": core.game_name(),
        "library": {
            "count": stats.count,
            "size": stats.size,
            "root": stats.root,
        },
        "hotkeys": {
            "available": hotkeys.available,
            "reason": hotkeys.reason,
            "combos": hotkeys.combos,
        },
    })
}

pub fn send(request: Value) -> Result<Value, String> {
    let mut stream = UnixStream::connect(path()).map_err(|_| {
        "LiMedal çalışmıyor. Önce 'limedal run' ile başlatın.".to_string()
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let text = serde_json::to_string(&request).map_err(|err| err.to_string())?;
    stream
        .write_all(text.as_bytes())
        .map_err(|err| format!("gönderilemedi: {}", err))?;
    stream
        .write_all(b"\n")
        .map_err(|err| format!("gönderilemedi: {}", err))?;
    let _ = stream.flush();

    let line = read_line(&mut stream).ok_or_else(|| "yanıt alınamadı".to_string())?;
    serde_json::from_str(&line).map_err(|err| format!("geçersiz yanıt: {}", err))
}

fn read_line(stream: &mut UnixStream) -> Option<String> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buffer.push(byte[0]);
                if buffer.len() > 1_000_000 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    if buffer.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&buffer).to_string())
    }
}
