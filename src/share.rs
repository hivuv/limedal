//! Yerel ağda klip paylaşımı: minik HTTP sunucusu (yalnızca GET).

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::core::Core;

pub struct ShareHandle {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl ShareHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start(core: Arc<Core>, port: u16) -> Result<ShareHandle, String> {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|err| format!("port {} kullanılamıyor: {}", port, err))?;
    let actual = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(port);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();

    std::thread::Builder::new()
        .name("limedal-share".to_string())
        .spawn(move || {
            for incoming in listener.incoming() {
                if flag.load(Ordering::SeqCst) || core.should_quit() {
                    break;
                }
                if let Ok(stream) = incoming {
                    let core = core.clone();
                    std::thread::spawn(move || {
                        let _ = handle(stream, core);
                    });
                }
            }
        })
        .map_err(|err| err.to_string())?;

    Ok(ShareHandle { port: actual, stop })
}

fn handle(mut stream: TcpStream, core: Arc<Core>) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    let mut range: Option<String> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches('\r').trim_end_matches('\n');
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = Some(value.trim().to_string());
            }
        }
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    if method != "GET" && method != "HEAD" {
        return respond_text(&mut stream, "405 Method Not Allowed", "Yalnızca GET desteklenir.");
    }
    let head_only = method == "HEAD";
    let path = target.split('?').next().unwrap_or("/");

    match path.strip_prefix("/s/") {
        Some(rest) => {
            if let Some(token) = rest.strip_suffix("/video") {
                match core.library.by_token(token) {
                    Some(clip) => {
                        let video = clip.video();
                        respond_file(&mut stream, &video, range.as_deref(), head_only)
                    }
                    None => respond_text(&mut stream, "404 Not Found", "Bağlantı geçersiz."),
                }
            } else {
                match core.library.by_token(rest) {
                    Some(clip) => {
                        let html = page_html(&clip.meta.title, &meta_line(&clip), rest);
                        respond_html(&mut stream, &html, head_only)
                    }
                    None => respond_text(
                        &mut stream,
                        "404 Not Found",
                        "Bağlantı geçersiz veya klip silinmiş.",
                    ),
                }
            }
        }
        None => respond_text(&mut stream, "404 Not Found", "LiMedal paylaşım sunucusu."),
    }
}

fn meta_line(clip: &crate::library::Clip) -> String {
    let mut bits: Vec<String> = Vec::new();
    if !clip.meta.game.trim().is_empty() {
        bits.push(clip.meta.game.clone());
    }
    if clip.meta.duration > 0.0 {
        bits.push(clip.meta.duration_text());
    }
    if !clip.meta.size_text().is_empty() {
        bits.push(clip.meta.size_text());
    }
    bits.join(" • ")
}

fn page_html(title: &str, meta: &str, token: &str) -> String {
    let safe_title = esc(title);
    let safe_meta = esc(meta);
    let safe_token = token
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>();
    format!(
        concat!(
            "<!doctype html><html lang=\"tr\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{title} — LiMedal</title><style>",
            ":root{{color-scheme:dark}}*{{box-sizing:border-box}}",
            "body{{margin:0;min-height:100vh;display:flex;flex-direction:column;align-items:center;",
            "justify-content:center;gap:18px;padding:24px;color:#e9e9f2;",
            "background:radial-gradient(circle at 50% 0%,#1d1b2e 0%,#0b0b12 60%);",
            "font-family:system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif}}",
            "h1{{margin:0;font-size:18px;font-weight:600;text-align:center}}",
            ".meta{{color:#8f8fa8;font-size:13px}}",
            "video{{width:min(92vw,1000px);border-radius:14px;background:#000;",
            "box-shadow:0 24px 60px rgba(0,0,0,.55)}}",
            "a.dl{{color:#9b8cff;font-size:13px;text-decoration:none;padding:9px 16px;",
            "border:1px solid #2c2c44;border-radius:999px}}",
            ".brand{{font-size:11px;letter-spacing:.14em;text-transform:uppercase;color:#5c5c78}}",
            "</style></head><body>",
            "<div class=\"brand\">LiMedal paylaşımı</div>",
            "<h1>{title}</h1><div class=\"meta\">{meta}</div>",
            "<video src=\"/s/{token}/video\" controls autoplay playsinline preload=\"metadata\"></video>",
            "<a class=\"dl\" href=\"/s/{token}/video\" download>Videoyu indir</a>",
            "</body></html>"
        ),
        title = safe_title,
        meta = safe_meta,
        token = safe_token
    )
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn respond_text(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let payload = body.as_bytes();
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        status,
        payload.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(payload)?;
    let _ = stream.flush();
    Ok(())
}

fn respond_html(stream: &mut TcpStream, body: &str, head_only: bool) -> std::io::Result<()> {
    let payload = body.as_bytes();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        payload.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head_only {
        stream.write_all(payload)?;
    }
    let _ = stream.flush();
    Ok(())
}

fn respond_file(
    stream: &mut TcpStream,
    path: &Path,
    range: Option<&str>,
    head_only: bool,
) -> std::io::Result<()> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return respond_text(stream, "404 Not Found", "Dosya bulunamadı."),
    };
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    if size == 0 {
        return respond_text(stream, "404 Not Found", "Dosya boş.");
    }

    let mut start: u64 = 0;
    let mut end: u64 = size - 1;
    let mut partial = false;

    if let Some(spec) = range {
        if let Some(rest) = spec.trim().strip_prefix("bytes=") {
            let first = rest.split(',').next().unwrap_or("");
            let mut bits = first.splitn(2, '-');
            let head = bits.next().unwrap_or("").trim().to_string();
            let tail = bits.next().unwrap_or("").trim().to_string();
            if !head.is_empty() {
                start = head.parse::<u64>().unwrap_or(0);
                if !tail.is_empty() {
                    end = tail.parse::<u64>().unwrap_or(size - 1);
                }
            } else if !tail.is_empty() {
                let wanted = tail.parse::<u64>().unwrap_or(0);
                start = size.saturating_sub(wanted);
            }
            start = start.min(size - 1);
            end = end.min(size - 1);
            if end < start {
                end = start;
            }
            partial = true;
        }
    }

    let length = end - start + 1;
    let status = if partial {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let mut header = String::new();
    header.push_str(&format!("HTTP/1.1 {}\r\n", status));
    header.push_str("Content-Type: video/mp4\r\n");
    header.push_str("Accept-Ranges: bytes\r\n");
    header.push_str(&format!("Content-Length: {}\r\n", length));
    if partial {
        header.push_str(&format!(
            "Content-Range: bytes {}-{}/{}\r\n",
            start, end, size
        ));
    }
    header.push_str("Connection: close\r\nCache-Control: no-cache\r\n\r\n");
    stream.write_all(header.as_bytes())?;

    if head_only {
        let _ = stream.flush();
        return Ok(());
    }

    file.seek(SeekFrom::Start(start))?;
    let mut remaining = length;
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = match file.read(&mut buffer[..want]) {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => break,
        };
        if stream.write_all(&buffer[..read]).is_err() {
            break;
        }
        remaining -= read as u64;
    }
    let _ = stream.flush();
    Ok(())
}

/// Yerel ağ IP'si (paket göndermeden).
pub fn lan_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("10.255.255.255:1").is_ok() {
            if let Ok(address) = socket.local_addr() {
                return address.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}
