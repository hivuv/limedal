//! LiMedal — Linux için Medal.tv alternatifi.
//!
//! Kullanım: `limedal run` (arayüz), `limedal clip` (klip al), `limedal doctor`.

mod audio;
mod capture;
mod config;
mod core;
mod ffmpeg;
mod gamedetect;
mod hotkeys;
mod ipc;
mod library;
mod share;
mod ui;
mod util;

use serde_json::json;

use crate::config::Config;
use crate::core::Core;
use crate::util::{human_bytes, human_duration};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = dispatch(&args);
    std::process::exit(code);
}

fn dispatch(args: &[String]) -> i32 {
    let command = args.first().map(|value| value.as_str()).unwrap_or("run");
    match command {
        "run" | "gui" => run_gui(),
        "clip" => cmd_clip(args),
        "record" => cmd_record(args),
        "status" => cmd_status(args),
        "stop" | "quit" => cmd_stop(),
        "open" | "show" => cmd_open(),
        "doctor" => cmd_doctor(),
        "paths" => cmd_paths(),
        "config" => cmd_config(args),
        "version" | "--version" | "-V" => {
            println!("LiMedal {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "help" | "--help" | "-h" => {
            print_help();
            0
        }
        other => {
            eprintln!("Bilinmeyen komut: {}", other);
            eprintln!();
            print_help();
            2
        }
    }
}

fn print_help() {
    println!("LiMedal {} — Linux için Medal alternatifi", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Kullanım: limedal [komut] [seçenekler]");
    println!();
    println!("  run                      Arayüzü ve kayıt motorunu başlat");
    println!("  clip [--seconds N] [--title T]   Son N saniyeyi klip yap");
    println!("  record start|stop        Tam kayıt");
    println!("  status [--json]          Durum özeti");
    println!("  open                     Arayüzü öne getir");
    println!("  stop                     Uygulamayı kapat");
    println!("  doctor                   ffmpeg, kodlayıcı, ses, izin kontrolü");
    println!("  paths                    Klasör yolları");
    println!("  config path|dump|set k=v Ayarlar");
    println!("  version                  Sürüm");
}

// ---------------------------------------------------------------------------
// Arayüz
// ---------------------------------------------------------------------------

fn run_gui() -> i32 {
    if ipc::is_running() {
        println!("LiMedal zaten çalışıyor; pencere öne getiriliyor.");
        let _ = ipc::send(json!({ "cmd": "show" }));
        return 0;
    }

    let core = Core::new();
    core.bootstrap();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("LiMedal"),
        ..Default::default()
    };

    let app_core = core.clone();
    let result = eframe::run_native(
        "LiMedal",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, app_core)))),
    );

    core.shutdown();

    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("Arayüz başlatılamadı: {}", err);
            eprintln!("Grafik oturumu (DISPLAY) açık mı? Konsolda 'limedal clip' kullanabilirsiniz.");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Komutlar
// ---------------------------------------------------------------------------

fn cmd_clip(args: &[String]) -> i32 {
    let mut seconds: Option<u32> = None;
    let mut title: Option<String> = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--seconds" | "-s" => {
                if let Some(value) = args.get(index + 1) {
                    seconds = value.parse::<u32>().ok();
                    index += 1;
                }
            }
            "--title" | "-t" => {
                if let Some(value) = args.get(index + 1) {
                    title = Some(value.clone());
                    index += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }

    let mut payload = json!({ "cmd": "clip" });
    if let Some(seconds) = seconds {
        payload["seconds"] = json!(seconds);
    }
    if let Some(title) = title {
        payload["title"] = json!(title);
    }

    match ipc::send(payload) {
        Ok(response) => {
            if response
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                println!(
                    "Klip kaydedildi: {} ({})",
                    response
                        .get("title")
                        .and_then(|value| value.as_str())
                        .unwrap_or("klip"),
                    human_duration(
                        response
                            .get("duration")
                            .and_then(|value| value.as_f64())
                            .unwrap_or(0.0)
                    )
                );
                if let Some(path) = response.get("path").and_then(|value| value.as_str()) {
                    println!("{}", path);
                }
                0
            } else {
                eprintln!(
                    "Klip kaydedilemedi: {}",
                    response
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("bilinmeyen hata")
                );
                1
            }
        }
        Err(err) => {
            eprintln!("{}", err);
            1
        }
    }
}

fn cmd_record(args: &[String]) -> i32 {
    let action = args.get(1).map(|value| value.as_str()).unwrap_or("");
    let command = match action {
        "start" => "record_start",
        "stop" => "record_stop",
        "toggle" => "record_toggle",
        _ => {
            eprintln!("Kullanım: limedal record start|stop");
            return 2;
        }
    };
    match ipc::send(json!({ "cmd": command })) {
        Ok(response) => {
            if response
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                if action == "stop" {
                    println!("Kayıt tamamlandı ve kütüphaneye eklendi.");
                } else if action == "start" {
                    println!("Kayıt başladı.");
                } else {
                    println!("Tamam.");
                }
                0
            } else {
                eprintln!(
                    "{}",
                    response
                        .get("error")
                        .and_then(|value| value.as_str())
                        .unwrap_or("işlem başarısız")
                );
                1
            }
        }
        Err(err) => {
            eprintln!("{}", err);
            1
        }
    }
}

fn cmd_status(args: &[String]) -> i32 {
    let as_json = args.iter().any(|value| value == "--json");
    let response = match ipc::send(json!({ "cmd": "status" })) {
        Ok(response) => response,
        Err(err) => {
            eprintln!("{}", err);
            return 1;
        }
    };

    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&response).unwrap_or_default()
        );
        return 0;
    }

    let running = response.get("running").and_then(|v| v.as_bool()).unwrap_or(false);
    let recording = response.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
    println!("LiMedal {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  Kayıt       : {}{}",
        if running { "çalışıyor" } else { "durdu" },
        response
            .get("error")
            .and_then(|v| v.as_str())
            .map(|err| format!(" — {}", err))
            .unwrap_or_default()
    );
    if running {
        println!(
            "  Tampon      : {} / {}",
            human_duration(response.get("fill_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0)),
            human_duration(response.get("buffer_seconds").and_then(|v| v.as_u64()).unwrap_or(0) as f64)
        );
    }
    if recording {
        println!(
            "  Tam kayıt   : {}",
            human_duration(response.get("recording_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0))
        );
    }
    println!(
        "  Kodlayıcı   : {}",
        response.get("encoder").and_then(|v| v.as_str()).unwrap_or("-")
    );
    println!(
        "  Çözünürlük  : {} @ {} fps",
        response.get("resolution").and_then(|v| v.as_str()).unwrap_or("-"),
        response.get("fps").and_then(|v| v.as_u64()).unwrap_or(0)
    );
    println!(
        "  Oyun        : {}",
        response.get("game").and_then(|v| v.as_str()).unwrap_or("-")
    );
    if let Some(hotkeys) = response.get("hotkeys") {
        let available = hotkeys.get("available").and_then(|v| v.as_bool()).unwrap_or(false);
        if available {
            let combos: Vec<String> = hotkeys
                .get("combos")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(|text| text.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            println!("  Kısayollar  : {}", combos.join(", "));
        } else {
            println!(
                "  Kısayollar  : kapalı ({})",
                hotkeys.get("reason").and_then(|v| v.as_str()).unwrap_or("?")
            );
        }
    }
    if let Some(library) = response.get("library") {
        println!(
            "  Kütüphane   : {} klip, {}",
            library.get("count").and_then(|v| v.as_u64()).unwrap_or(0),
            human_bytes(library.get("size").and_then(|v| v.as_u64()).unwrap_or(0))
        );
        println!(
            "  Klasör      : {}",
            library.get("root").and_then(|v| v.as_str()).unwrap_or("-")
        );
    }
    0
}

fn cmd_stop() -> i32 {
    match ipc::send(json!({ "cmd": "quit" })) {
        Ok(_) => {
            println!("LiMedal kapatılıyor.");
            0
        }
        Err(err) => {
            eprintln!("{}", err);
            1
        }
    }
}

fn cmd_open() -> i32 {
    match ipc::send(json!({ "cmd": "show" })) {
        Ok(_) => {
            println!("Pencere öne getirildi.");
            0
        }
        Err(err) => {
            eprintln!("{}", err);
            1
        }
    }
}

fn cmd_doctor() -> i32 {
    let core = Core::new();
    println!("LiMedal {} — sistem kontrolü\n", env!("CARGO_PKG_VERSION"));
    let mut problems = 0;
    for (ok, label, detail) in core.doctor() {
        println!(
            "  [{}] {:<16}{}",
            if ok { "ok" } else { "!!" },
            label,
            detail
        );
        if !ok {
            problems += 1;
        }
    }
    println!();
    if problems > 0 {
        println!("{} eksik var. Yukarıdaki [!!] satırlarına bakın.", problems);
        return 1;
    }
    println!("Her şey yerinde. 'limedal run' ile başlatabilirsiniz.");
    0
}

fn cmd_paths() -> i32 {
    let config = Core::new().config();
    println!("Ayarlar     : {}", util::config_path().display());
    println!("Kütüphane   : {}", config.library_dir().display());
    println!("Önbellek    : {}", util::cache_dir().display());
    println!("Çalışma anı : {}", util::state_dir().display());
    println!("Soket       : {}", ipc::path().display());
    0
}

fn cmd_config(args: &[String]) -> i32 {
    let action = args.get(1).map(|value| value.as_str()).unwrap_or("path");
    match action {
        "path" => {
            println!("{}", util::config_path().display());
            0
        }
        "dump" => {
            let config = Core::new().config();
            match serde_json::to_string_pretty(&config) {
                Ok(text) => {
                    println!("{}", text);
                    0
                }
                Err(_) => 1,
            }
        }
        "set" => {
            let pair = match args.get(2) {
                Some(pair) => pair.clone(),
                None => {
                    eprintln!("Kullanım: limedal config set capture.fps=60");
                    return 2;
                }
            };
            let (key, raw) = match pair.split_once('=') {
                Some((key, value)) => (key.trim().to_string(), value.trim().to_string()),
                None => {
                    eprintln!("Kullanım: limedal config set capture.fps=60");
                    return 2;
                }
            };
            let value: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw.clone()));

            let mut root = match serde_json::to_value(Core::new().config()) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("Ayarlar okunamadı: {}", err);
                    return 1;
                }
            };
            let parts: Vec<String> = key.split('.').map(|part| part.to_string()).collect();
            if parts.is_empty() {
                return 2;
            }
            let mut node: &mut serde_json::Value = &mut root;
            for part in parts.iter().take(parts.len() - 1) {
                let is_object = node
                    .get(part.as_str())
                    .map(|value| value.is_object())
                    .unwrap_or(false);
                if !is_object {
                    node[part.as_str()] = json!({});
                }
                node = match node.get_mut(part.as_str()) {
                    Some(next) => next,
                    None => {
                        eprintln!("Ayar yolu açılamadı: {}", key);
                        return 1;
                    }
                };
            }
            if let Some(last) = parts.last() {
                node[last.as_str()] = value;
            }

            match serde_json::from_value::<Config>(root) {
                Ok(mut config) => {
                    config.sanitize();
                    config.save();
                    println!("{} ayarlandı.", key);
                    0
                }
                Err(err) => {
                    eprintln!("Geçersiz değer: {}", err);
                    1
                }
            }
        }
        other => {
            eprintln!("Bilinmeyen eylem: {}", other);
            2
        }
    }
}
