//! Oyun algılama: /proc üzerinden çalışan oyun süreçlerini bulur.

use std::sync::Mutex;

const KNOWN: &[(&str, &str)] = &[
    ("cs2", "Counter-Strike 2"),
    ("csgo", "Counter-Strike: Global Offensive"),
    ("hl2_linux", "Half-Life 2"),
    ("dota2", "Dota 2"),
    ("tf_linux64", "Team Fortress 2"),
    ("rustclient", "Rust"),
    ("deadlock", "Deadlock"),
    ("gtav", "Grand Theft Auto V"),
    ("gta5", "Grand Theft Auto V"),
    ("rdr2", "Red Dead Redemption 2"),
    ("valorant", "VALORANT"),
    ("leagueclient", "League of Legends"),
    ("fortnite", "Fortnite"),
    ("minecraft", "Minecraft"),
    ("factorio", "Factorio"),
    ("terraria", "Terraria"),
    ("stardew valley", "Stardew Valley"),
    ("witcher3", "The Witcher 3"),
    ("cyberpunk2077", "Cyberpunk 2077"),
    ("eldenring", "Elden Ring"),
    ("sekiro", "Sekiro"),
    ("dark souls iii", "Dark Souls III"),
    ("baldurs gate 3", "Baldur's Gate 3"),
    ("bg3", "Baldur's Gate 3"),
    ("hades", "Hades"),
    ("hades2", "Hades II"),
    ("hollow_knight", "Hollow Knight"),
    ("celeste", "Celeste"),
    ("among us", "Among Us"),
    ("rocketleague", "Rocket League"),
    ("destiny2", "Destiny 2"),
    ("warframe", "Warframe"),
    ("pathofexile", "Path of Exile"),
    ("poe2", "Path of Exile 2"),
    ("apex", "Apex Legends"),
    ("r5apex", "Apex Legends"),
    ("overwatch", "Overwatch 2"),
    ("pubg", "PUBG"),
    ("eurotrucks2", "Euro Truck Simulator 2"),
    ("beamng.drive", "BeamNG.drive"),
    ("assettocorsa", "Assetto Corsa"),
    ("arma3", "Arma 3"),
    ("dayz", "DayZ"),
    ("palworld", "Palworld"),
    ("helldivers2", "Helldivers 2"),
    ("deep rock galactic", "Deep Rock Galactic"),
    ("valheim", "Valheim"),
    ("satisfactory", "Satisfactory"),
    ("vrising", "V Rising"),
    ("projectzomboid", "Project Zomboid"),
    ("eft", "Escape from Tarkov"),
];

const IGNORED: &[&str] = &[
    "wine",
    "wine64",
    "wine-preloader",
    "wineserver",
    "wineboot",
    "proton",
    "pressure-vessel",
    "steam",
    "steamwebhelper",
    "gameoverlayui",
    "gamescope",
    "gamemoded",
    "mangohud",
    "ffmpeg",
    "ffprobe",
    "pipewire",
    "pulseaudio",
    "wireplumber",
    "python3",
    "python",
    "limedal",
    "bash",
    "sh",
    "systemd",
    "dbus-daemon",
    "Xorg",
    "Xwayland",
    "gnome-shell",
    "plasmashell",
    "kwin_x11",
    "kwin_wayland",
    "xdg-desktop-por",
    "gjs",
    "node",
    "discord",
    "spotify",
    "firefox",
    "chrome",
    "chromium",
    "code",
];

const HINTS: &[&str] = &[
    "/steamapps/common/",
    "/steam/steamapps/common/",
    "/.steam/steam/steamapps/common/",
    "/.local/share/Steam/steamapps/common/",
    "/GOG Games/",
    "/Epic Games/",
    "/Games/",
    "/games/",
    "/.wine/drive_c/Program Files/",
    "/.wine/drive_c/Program Files (x86)/",
];

#[derive(Clone, Debug)]
pub struct Game {
    pub name: String,
    pub pid: u32,
    pub source: String,
    pub started: u64,
}

#[derive(Default)]
pub struct Detection {
    pub current: Option<Game>,
    pub list: Vec<Game>,
}

pub struct Detector {
    shared: Mutex<Detection>,
}

impl Detector {
    pub fn new() -> Detector {
        Detector {
            shared: Mutex::new(Detection::default()),
        }
    }

    pub fn refresh(&self) -> Vec<Game> {
        let found = self.scan();
        let current = found.first().cloned();
        if let Ok(mut guard) = self.shared.lock() {
            guard.current = current;
            guard.list = found.clone();
        }
        found
    }

    pub fn current(&self) -> Option<Game> {
        match self.shared.lock() {
            Ok(guard) => guard.current.clone(),
            Err(_) => None,
        }
    }

    pub fn current_name(&self) -> Option<String> {
        self.current().map(|game| game.name)
    }

    pub fn list(&self) -> Vec<Game> {
        match self.shared.lock() {
            Ok(guard) => guard.list.clone(),
            Err(_) => Vec::new(),
        }
    }

    pub fn scan(&self) -> Vec<Game> {
        let mut found: Vec<Game> = Vec::new();
        let entries = match std::fs::read_dir("/proc") {
            Ok(entries) => entries,
            Err(_) => return found,
        };
        let own_pid = std::process::id();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let text = name.to_string_lossy().to_string();
            let pid: u32 = match text.parse() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if pid == own_pid {
                continue;
            }
            if let Some(game) = inspect(pid) {
                found.push(game);
            }
        }
        found.sort_by(|a, b| b.started.cmp(&a.started));
        found
    }
}

fn read_proc(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn exe_path(pid: u32) -> String {
    match std::fs::read_link(format!("/proc/{}/exe", pid)) {
        Ok(path) => path.to_string_lossy().to_string(),
        Err(_) => String::new(),
    }
}

fn cmdline(pid: u32) -> Vec<String> {
    let raw = match read_proc(&format!("/proc/{}/cmdline", pid)) {
        Some(bytes) => bytes,
        None => return Vec::new(),
    };
    raw.split(|byte| *byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

fn start_time(pid: u32) -> u64 {
    let text = match std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
        Ok(text) => text,
        Err(_) => return 0,
    };
    let tail = match text.rfind(')') {
        Some(index) => &text[index + 1..],
        None => return 0,
    };
    let fields: Vec<&str> = tail.split_whitespace().collect();
    fields
        .get(19)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn inspect(pid: u32) -> Option<Game> {
    let exe = exe_path(pid);
    let args = cmdline(pid);
    if exe.is_empty() && args.is_empty() {
        return None;
    }

    let exe_name = exe
        .rsplit('/')
        .next()
        .unwrap_or("")
        .replace(" (deleted)", "")
        .to_lowercase();
    if !exe_name.is_empty() && IGNORED.iter().any(|item| *item == exe_name) {
        return None;
    }

    let mut name: Option<String> = None;
    let mut source = "";

    // 1) Steam / Proton oyun klasörü yolu
    for hint in HINTS {
        if let Some(index) = exe.find(hint) {
            let rest = &exe[index + hint.len()..];
            let segment = rest.split('/').next().unwrap_or("");
            if !segment.is_empty() && !IGNORED.iter().any(|item| *item == segment.to_lowercase()) {
                name = Some(prettify(segment));
                source = "yol";
                break;
            }
        }
    }

    // 2) Komut satırındaki .exe (Proton/Wine oyunları)
    if name.is_none() {
        for arg in &args {
            let lowered = arg.to_lowercase();
            if !lowered.ends_with(".exe") {
                continue;
            }
            for hint in HINTS {
                let lowered_hint = hint.to_lowercase();
                if let Some(index) = lowered.find(&lowered_hint) {
                    let rest = &lowered[index + lowered_hint.len()..];
                    let segment = rest.split('/').next().unwrap_or("");
                    if !segment.is_empty() {
                        name = Some(prettify(segment));
                        source = "wine";
                        break;
                    }
                }
            }
            if name.is_some() {
                break;
            }
            let base = lowered
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".exe")
                .to_string();
            if base.is_empty() || IGNORED.iter().any(|item| *item == base) {
                continue;
            }
            if let Some(mapped) = lookup_known(&base) {
                name = Some(mapped.to_string());
                source = "wine";
                break;
            }
        }
    }

    // 3) Bilinen süreç adı
    if name.is_none() {
        let mut candidates: Vec<String> = Vec::new();
        if !exe_name.is_empty() {
            candidates.push(exe_name.clone());
        }
        if let Some(first) = args.first() {
            let base = first.rsplit('/').next().unwrap_or("").to_lowercase();
            if !base.is_empty() {
                candidates.push(base);
            }
        }
        for candidate in candidates {
            if let Some(mapped) = lookup_known(&candidate) {
                name = Some(mapped.to_string());
                source = "süreç";
                break;
            }
            let spaced = candidate.replace('-', " ").replace('_', " ");
            if let Some(mapped) = lookup_known(&spaced) {
                name = Some(mapped.to_string());
                source = "süreç";
                break;
            }
        }
    }

    let name = name?;
    if IGNORED.iter().any(|item| *item == name.to_lowercase()) {
        return None;
    }

    Some(Game {
        name,
        pid,
        source: source.to_string(),
        started: start_time(pid),
    })
}

fn lookup_known(key: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|(process, _)| *process == key)
        .map(|(_, display)| *display)
}

fn prettify(folder: &str) -> String {
    let mut text = folder.replace('_', " ");
    text = text.trim().to_string();
    let lowered = text.to_lowercase();
    if lowered.ends_with(" (linux)") || lowered.ends_with(" linux") {
        if let Some(index) = text.rfind(' ') {
            text = text[..index].trim().to_string();
        }
    }
    if text.is_empty() {
        folder.to_string()
    } else {
        text
    }
}
