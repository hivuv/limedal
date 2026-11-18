//! Global kısayollar: /dev/input üzerinden ham klavye dinleyicisi.
//!
//! X11 ve Wayland'de aynı şekilde çalışır. Kullanıcının `input` grubunda
//! olması gerekir:  sudo usermod -aG input $USER

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;
use std::time::Duration;

use crate::core::Core;
use crate::util::now;

const EV_KEY: u16 = 0x01;

#[repr(C)]
#[derive(Copy, Clone)]
struct InputEvent {
    sec: libc::time_t,
    usec: libc::suseconds_t,
    kind: u16,
    code: u16,
    value: i32,
}

/// Linux girdi kodu -> okunur ad (standart klavye bloğu).
const KEYS: &[(u16, &str)] = &[
    (1, "esc"),
    (2, "1"),
    (3, "2"),
    (4, "3"),
    (5, "4"),
    (6, "5"),
    (7, "6"),
    (8, "7"),
    (9, "8"),
    (10, "9"),
    (11, "0"),
    (12, "minus"),
    (13, "equal"),
    (14, "backspace"),
    (15, "tab"),
    (16, "q"),
    (17, "w"),
    (18, "e"),
    (19, "r"),
    (20, "t"),
    (21, "y"),
    (22, "u"),
    (23, "i"),
    (24, "o"),
    (25, "p"),
    (28, "enter"),
    (29, "leftctrl"),
    (30, "a"),
    (31, "s"),
    (32, "d"),
    (33, "f"),
    (34, "g"),
    (35, "h"),
    (36, "j"),
    (37, "k"),
    (38, "l"),
    (41, "grave"),
    (42, "leftshift"),
    (44, "z"),
    (45, "x"),
    (46, "c"),
    (47, "v"),
    (48, "b"),
    (49, "n"),
    (50, "m"),
    (51, "comma"),
    (52, "dot"),
    (53, "slash"),
    (54, "rightshift"),
    (56, "leftalt"),
    (57, "space"),
    (58, "capslock"),
    (59, "f1"),
    (60, "f2"),
    (61, "f3"),
    (62, "f4"),
    (63, "f5"),
    (64, "f6"),
    (65, "f7"),
    (66, "f8"),
    (67, "f9"),
    (68, "f10"),
    (87, "f11"),
    (88, "f12"),
    (96, "kpenter"),
    (97, "rightctrl"),
    (100, "rightalt"),
    (102, "home"),
    (103, "up"),
    (104, "pageup"),
    (105, "left"),
    (106, "right"),
    (107, "end"),
    (108, "down"),
    (109, "pagedown"),
    (110, "insert"),
    (111, "delete"),
    (119, "pause"),
    (125, "leftmeta"),
    (126, "rightmeta"),
    (183, "f13"),
    (184, "f14"),
    (185, "f15"),
    (186, "f16"),
    (187, "f17"),
    (188, "f18"),
    (189, "f19"),
    (190, "f20"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    SaveClip,
    ToggleRecord,
}

struct Binding {
    modifiers: Vec<&'static str>,
    code: u16,
    action: Action,
    text: String,
}

fn key_name(code: u16) -> Option<&'static str> {
    KEYS.iter().find(|(item, _)| *item == code).map(|(_, name)| *name)
}

fn key_code(name: &str) -> Option<u16> {
    KEYS.iter().find(|(_, item)| *item == name).map(|(code, _)| *code)
}

fn modifier_role(name: &str) -> Option<&'static str> {
    match name {
        "leftctrl" | "rightctrl" => Some("ctrl"),
        "leftshift" | "rightshift" => Some("shift"),
        "leftalt" | "rightalt" => Some("alt"),
        "leftmeta" | "rightmeta" => Some("meta"),
        _ => None,
    }
}

fn alias(name: &str) -> Option<&'static str> {
    match name {
        "ctrl" | "control" | "strg" => Some("ctrl"),
        "shift" => Some("shift"),
        "alt" | "altgr" => Some("alt"),
        "meta" | "super" | "win" | "windows" | "cmd" => Some("meta"),
        _ => None,
    }
}

/// "ctrl+shift+F9" -> (modifikatörler, tuş kodu)
pub fn parse_combo(text: &str) -> Option<(Vec<&'static str>, u16)> {
    let cleaned: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    let parts: Vec<&str> = cleaned.split('+').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let key_text = parts[parts.len() - 1].to_lowercase();
    if alias(&key_text).is_some() {
        return None; // yalnızca değiştirici tuş atanamaz
    }
    let code = key_code(&key_text)?;
    let mut modifiers: Vec<&'static str> = Vec::new();
    for part in &parts[..parts.len() - 1] {
        let role = alias(&part.to_lowercase())?;
        if !modifiers.contains(&role) {
            modifiers.push(role);
        }
    }
    Some((modifiers, code))
}

#[derive(Clone, Debug)]
pub struct HotkeyState {
    pub available: bool,
    pub reason: String,
    pub devices: usize,
    pub combos: Vec<String>,
}

pub fn spawn(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("limedal-hotkeys".to_string())
        .spawn(move || run(core))
        .ok();
}

fn run(core: Arc<Core>) {
    let mut files: Vec<File> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut bindings: Vec<Binding> = Vec::new();
    let mut pressed: Vec<u16> = Vec::new();
    let mut last_fire: Vec<(String, f64)> = Vec::new();
    let mut last_scan = 0.0f64;
    let mut last_reload = 0.0f64;
    let mut warned = false;

    loop {
        if core.should_quit() {
            break;
        }

        if now() - last_reload > 2.0 {
            last_reload = now();
            bindings = load_bindings(&core);
            core.set_hotkey_state(hotkey_state(&bindings, files.len()));
        }

        if files.is_empty() || now() - last_scan > 10.0 {
            last_scan = now();
            scan_devices(&mut files, &mut paths);
            if files.is_empty() {
                let reason = "Klavye aygıtı açılamadı (/dev/input/event*). Şunu çalıştırıp oturumu yenileyin: sudo usermod -aG input $USER".to_string();
                if !warned {
                    core.log.warn(&format!("Kısayollar devre dışı: {}", reason));
                    warned = true;
                }
                core.set_hotkey_state(HotkeyState {
                    available: false,
                    reason,
                    devices: 0,
                    combos: bindings.iter().map(|item| item.text.clone()).collect(),
                });
            } else {
                warned = false;
                core.set_hotkey_state(hotkey_state(&bindings, files.len()));
            }
        }

        let mut buffer = [0u8; 1536];
        for file in files.iter_mut() {
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        handle_bytes(
                            &buffer[..count],
                            &bindings,
                            &mut pressed,
                            &mut last_fire,
                            &core,
                        );
                    }
                    Err(err) => {
                        if err.kind() == std::io::ErrorKind::WouldBlock {
                            break;
                        }
                        break;
                    }                }
            }
        }

        std::thread::sleep(Duration::from_millis(40));
    }
}

fn hotkey_state(bindings: &[Binding], devices: usize) -> HotkeyState {
    HotkeyState {
        available: devices > 0,
        reason: if devices > 0 {
            String::new()
        } else {
            "Klavye aygıtı açılamadı (/dev/input/event*).".to_string()
        },
        devices,
        combos: bindings.iter().map(|item| item.text.clone()).collect(),
    }
}

fn load_bindings(core: &Arc<Core>) -> Vec<Binding> {
    let config = core.config();
    if !config.hotkeys.enabled {
        return Vec::new();
    }
    let mut list: Vec<Binding> = Vec::new();
    let candidates = [
        (config.hotkeys.save_clip.clone(), Action::SaveClip),
        (config.hotkeys.toggle_record.clone(), Action::ToggleRecord),
    ];
    for (text, action) in candidates {
        if text.trim().is_empty() {
            continue;
        }
        if let Some((modifiers, code)) = parse_combo(&text) {
            list.push(Binding {
                modifiers,
                code,
                action,
                text,
            });
        } else {
            core.log
                .warn(&format!("Geçersiz kısayol: {:?}", text));
        }
    }
    list
}

fn scan_devices(files: &mut Vec<File>, paths: &mut Vec<String>) {
    let entries = match std::fs::read_dir("/dev/input") {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut found: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("event") {
            found.push(format!("/dev/input/{}", name));
        }
    }
    found.sort_by_key(|path| {
        path.rsplit("event")
            .next()
            .and_then(|tail| tail.parse::<u32>().ok())
            .unwrap_or(0)
    });

    for path in found {
        if paths.contains(&path) {
            continue;
        }
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path);
        if let Ok(file) = opened {
            files.push(file);
            paths.push(path);
        }
    }
}

fn handle_bytes(
    bytes: &[u8],
    bindings: &[Binding],
    pressed: &mut Vec<u16>,
    last_fire: &mut Vec<(String, f64)>,
    core: &Arc<Core>,
) {
    let size = std::mem::size_of::<InputEvent>();
    if size == 0 {
        return;
    }
    let count = bytes.len() / size;
    for index in 0..count {
        let offset = index * size;
        let event: InputEvent =
            unsafe { std::ptr::read_unaligned(bytes[offset..].as_ptr() as *const InputEvent) };
        if event.kind != EV_KEY {
            continue;
        }
        let name = match key_name(event.code) {
            Some(name) => name,
            None => continue,
        };
        if event.value == 0 {
            pressed.retain(|code| *code != event.code);
            continue;
        }
        if event.value == 2 {
            continue; // basılı tutma
        }
        if !pressed.contains(&event.code) {
            pressed.push(event.code);
        }

        let mut active: Vec<&'static str> = Vec::new();
        for code in pressed.iter() {
            if let Some(pressed_name) = key_name(*code) {
                if let Some(role) = modifier_role(pressed_name) {
                    if !active.contains(&role) {
                        active.push(role);
                    }
                }
            }
        }

        for binding in bindings {
            if binding.code != event.code {
                continue;
            }
            if binding.modifiers.len() != active.len() {
                continue;
            }
            if !binding.modifiers.iter().all(|role| active.contains(role)) {
                continue;
            }
            let stamp = now();
            let already = last_fire
                .iter()
                .find(|(text, _)| *text == binding.text)
                .map(|(_, at)| *at)
                .unwrap_or(0.0);
            if stamp - already < 0.35 {
                continue;
            }
            last_fire.retain(|(text, _)| *text != binding.text);
            last_fire.push((binding.text.clone(), stamp));
            core.log
                .info(&format!("Kısayol: {} ({})", binding.text, name));
            match binding.action {
                Action::SaveClip => core.hotkey_save_clip(),
                Action::ToggleRecord => core.hotkey_toggle_record(),
            }
        }
    }
}
