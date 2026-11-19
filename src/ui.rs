//! LiMedal arayüzü — sıfırdan yazılmış Medal benzeri düzen.
//!
//! Kabuk: solda gezinme rayı, üstte durum ve eylemler, ortada sayfa içeriği,
//! sağda kayan klip çekmecesi. Tüm renkler `theme` modülünden gelir.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui;

use crate::anim;
use crate::capture::CaptureStatus;
use crate::config::Config;
use crate::core::{Core, Toast, ToastKind};
use crate::editor::{Editor, EditorAction};
use crate::gamedetect::Game;
use crate::hyprland;
use crate::library::{Clip, Stats};
use crate::theme::{self, ACCENT, ACCENT_2, ACCENT_HI, BG, BG_DEEP, BORDER, BORDER_HI, DANGER, GOLD, LINE, MUTED, MUTED_DIM, OK, PINK, RADIUS, RADIUS_SM, SURFACE, SURFACE_HI, SURFACE_LO, TEXT, TEXT_SOFT};
use crate::util::{human_bytes, human_duration, now, time_ago};

// ---------------------------------------------------------------------------
// Sayfalar
// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum View {
    Library,
    Live,
    Games,
    Settings,
}

impl View {
    fn index(self) -> u32 {
        match self {
            View::Library => 0,
            View::Live => 1,
            View::Games => 2,
            View::Settings => 3,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Filter {
    All,
    Favorites,
}

#[derive(PartialEq, Clone, Copy)]
enum SortKey {
    Newest,
    Oldest,
    Longest,
    Largest,
    Title,
}

impl SortKey {
    fn value(self) -> &'static str {
        match self {
            SortKey::Newest => "newest",
            SortKey::Oldest => "oldest",
            SortKey::Longest => "duration",
            SortKey::Largest => "size",
            SortKey::Title => "title",
        }
    }
}

// ---------------------------------------------------------------------------
// Uygulama durumu
// ---------------------------------------------------------------------------

pub struct App {
    core: Arc<Core>,
    view: View,
    search: String,
    sort: SortKey,
    filter: Filter,
    game_filter: Option<String>,
    clips: Vec<Clip>,
    stats: Stats,
    status: CaptureStatus,
    game: Option<String>,
    detected: Vec<Game>,
    toasts: Vec<Toast>,
    textures: HashMap<String, egui::TextureHandle>,
    failed: Vec<String>,
    selected: Option<String>,
    strip: Option<egui::TextureHandle>,
    editor: Option<Editor>,
    draft: Config,
    draft_dirty: bool,
    rename_buffer: String,
    rename_id: Option<String>,
    trim_start: f32,
    trim_end: f32,
    trim_id: Option<String>,
    share_url: Option<String>,
    confirm_delete: Option<String>,
    show_help: bool,
    doctor: Vec<(bool, String, String)>,
    doctor_at: f64,
    log_tail: Vec<String>,
    last_refresh: f64,
    started_at: f64,
    view_entered_at: f64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>) -> App {
        theme::apply(&cc.egui_ctx);
        core.set_ctx(cc.egui_ctx.clone());

        let mut app = App {
            view: View::Library,
            search: String::new(),
            sort: SortKey::Newest,
            filter: Filter::All,
            game_filter: None,
            clips: Vec::new(),
            stats: Stats::default(),
            status: CaptureStatus::default(),
            game: None,
            detected: Vec::new(),
            toasts: Vec::new(),
            textures: HashMap::new(),
            failed: Vec::new(),
            selected: None,
            strip: None,
            editor: None,
            draft: core.config(),
            draft_dirty: false,
            rename_buffer: String::new(),
            rename_id: None,
            trim_start: 0.0,
            trim_end: 0.0,
            trim_id: None,
            share_url: None,
            confirm_delete: None,
            show_help: false,
            doctor: Vec::new(),
            doctor_at: 0.0,
            log_tail: Vec::new(),
            last_refresh: 0.0,
            started_at: now(),
            view_entered_at: 0.0,
            core,
        };
        app.refresh(true);
        app
    }

    // ------------------------------------------------------------------ veri
    fn refresh(&mut self, force: bool) {
        let stamp = now();
        if !force && stamp - self.last_refresh < 1.0 {
            return;
        }
        self.last_refresh = stamp;
        self.status = self.core.status();
        self.stats = self.core.stats();
        self.game = self.core.game_name();
        self.detected = self.core.detected();
        self.toasts = self.core.toasts();
        if !self.draft_dirty {
            self.draft = self.core.config();
        }

        let mut clips = self.core.clips(&self.search, self.sort.value());
        if self.filter == Filter::Favorites {
            clips.retain(|clip| clip.meta.favorite);
        }
        if let Some(game) = &self.game_filter {
            clips.retain(|clip| &clip.meta.game == game);
        }
        self.clips = clips;

        if self.view == View::Live {
            self.log_tail = self.core.log.tail(200);
        }
    }

    fn invalidate(&mut self) {
        self.last_refresh = 0.0;
        self.refresh(true);
    }

    fn set_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.view_entered_at = now();
        if view == View::Settings {
            self.draft = self.core.config();
            self.draft_dirty = false;
        }
        if view == View::Live {
            self.log_tail = self.core.log.tail(200);
        }
        self.last_refresh = 0.0;
    }

    fn texture_for(&mut self, ctx: &egui::Context, path: &Path, key: &str) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(key) {
            return Some(texture.clone());
        }
        if self.failed.iter().any(|item| item == key) {
            return None;
        }
        match load_texture(ctx, path) {
            Some(texture) => {
                self.textures.insert(key.to_string(), texture.clone());
                Some(texture)
            }
            None => {
                self.failed.push(key.to_string());
                None
            }
        }
    }

    fn selected_clip(&self) -> Option<Clip> {
        let id = self.selected.clone()?;
        self.clips.iter().find(|clip| clip.meta.id == id).cloned()
    }

    fn open_editor(&mut self, clip: Clip) {
        self.editor = Some(Editor::open(self.core.clone(), clip));
        self.selected = None;
        self.strip = None;
        self.trim_id = None;
    }

    // ------------------------------------------------------------------ eylemler
    fn save_clip(&mut self) {
        let seconds = self.draft.app.clip_seconds_default;
        let core = self.core.clone();
        std::thread::spawn(move || {
            let _ = core.save_clip_now(Some(seconds), None);
        });
    }

    fn toggle_record(&mut self) {
        let core = self.core.clone();
        std::thread::spawn(move || {
            let _ = core.toggle_record();
        });
    }

    fn restart_capture(&mut self) {
        let core = self.core.clone();
        std::thread::spawn(move || match core.engine.restart() {
            Ok(()) => core.toast("Kayıt yeniden başladı".to_string(), ToastKind::Ok),
            Err(err) => core.toast(format!("Başlatılamadı: {}", err), ToastKind::Error),
        });
    }

    fn stop_capture(&mut self) {
        let core = self.core.clone();
        std::thread::spawn(move || {
            if let Err(err) = core.engine.stop() {
                core.toast(format!("Durdurulamadı: {}", err), ToastKind::Error);
            }
        });
    }

    fn copy_text(&self, ctx: &egui::Context, text: &str) {
        ctx.copy_text(text.to_string());
        self.core
            .toast("Kopyalandı".to_string(), ToastKind::Ok);
    }

    // ------------------------------------------------------------------ gezinme
    fn draw_nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(theme::NAV_WIDTH)
            .frame(
                egui::Frame::none()
                    .fill(BG_DEEP)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                self.nav_brand(ui);
                ui.add_space(20.0);

                let items = [
                    (View::Library, "Kütüphane", "kliplerin"),
                    (View::Live, "Canlı", "kayıt durumu"),
                    (View::Games, "Oyunlar", "algılama"),
                    (View::Settings, "Ayarlar", "tercihler"),
                ];

                // Kayan vurgu: seçili öğenin arkasında yumuşak bir zemin.
                for (view, label, hint) in items {
                    let selected = self.view == view;
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 46.0),
                        egui::Sense::click(),
                    );
                    let t = ui
                        .ctx()
                        .animate_bool(egui::Id::new(("nav", view.index())), selected);
                    let hover = ui.ctx().animate_bool(
                        egui::Id::new(("nav-hover", view.index())),
                        response.hovered(),
                    );
                    let painter = ui.painter();
                    if t > 0.01 {
                        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, anim::with_alpha(ACCENT, 0.16 * t));
                        painter.rect_stroke(
                            rect.shrink2(egui::vec2(0.0, 1.0)),
                            RADIUS_SM,
                            egui::Stroke::new(1.0, anim::with_alpha(ACCENT, 0.45 * t)),
                        );
                        let bar = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + 1.0, rect.center().y - 11.0 * t),
                            egui::vec2(3.0, 22.0 * t),
                        );
                        painter.rect_filled(bar, 2.0, ACCENT_HI);
                    } else if hover > 0.01 {
                        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, anim::with_alpha(SURFACE_HI, 0.8 * hover));
                    }

                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 26.0, rect.center().y),
                        egui::vec2(18.0, 18.0),
                    );
                    let icon_color = if selected {
                        ACCENT_2
                    } else {
                        anim::lerp_color(MUTED, TEXT_SOFT, hover)
                    };
                    paint_icon(painter, icon_rect, view, icon_color);

                    painter.text(
                        egui::pos2(rect.left() + 48.0, rect.center().y - 7.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(13.5),
                        if selected { TEXT } else { TEXT_SOFT },
                    );
                    painter.text(
                        egui::pos2(rect.left() + 48.0, rect.center().y + 8.0),
                        egui::Align2::LEFT_CENTER,
                        hint,
                        egui::FontId::proportional(10.5),
                        MUTED_DIM,
                    );

                    if view == View::Library {
                        let count = format!("{}", self.stats.count);
                        painter.text(
                            egui::pos2(rect.right() - 12.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            count,
                            egui::FontId::proportional(11.5),
                            if selected { TEXT } else { MUTED_DIM },
                        );
                    }
                    if view == View::Live && self.status.recording {
                        let color = anim::lerp_color(PINK, egui::Color32::WHITE, anim::pulse(now(), 1.2) * 0.5);
                        painter.circle_filled(egui::pos2(rect.right() - 14.0, rect.center().y), 4.0, color);
                    }

                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        self.set_view(view);
                    }
                    ui.add_space(4.0);
                }

                ui.add_space(10.0);
                self.nav_status_card(ui);

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(4.0);
                    let hotkeys = self.core.hotkey_state();
                    let (color, text) = if hotkeys.available {
                        (OK, "Kısayollar etkin".to_string())
                    } else {
                        (MUTED_DIM, "Kısayollar kapalı".to_string())
                    };
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 3.0, color);
                        ui.label(egui::RichText::new(text).size(10.5).color(MUTED_DIM));
                    });
                    ui.add_space(2.0);
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("Yardım  [F1]").size(11.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.show_help = true;
                    }
                });
            });
    }

    fn nav_brand(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 52.0), egui::Sense::hover());
        let painter = ui.painter();
        let mark = egui::Rect::from_min_size(rect.min + egui::vec2(2.0, 8.0), egui::vec2(34.0, 34.0));
        painter.rect_filled(mark, 11.0, ACCENT);
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(mark.center().x - 4.0, mark.center().y - 7.0),
                egui::pos2(mark.center().x - 4.0, mark.center().y + 7.0),
                egui::pos2(mark.center().x + 7.0, mark.center().y),
            ],
            egui::Color32::WHITE,
            egui::Stroke::NONE,
        ));
        painter.text(
            egui::pos2(mark.right() + 11.0, rect.center().y - 7.0),
            egui::Align2::LEFT_CENTER,
            "LiMedal",
            egui::FontId::proportional(16.0),
            TEXT,
        );
        let subtitle = self
            .game
            .clone()
            .unwrap_or_else(|| if self.status.running { "kayıt açık".to_string() } else { "beklemede".to_string() });
        painter.text(
            egui::pos2(mark.right() + 11.0, rect.center().y + 9.0),
            egui::Align2::LEFT_CENTER,
            truncate_chars(&subtitle, 20),
            egui::FontId::proportional(10.5),
            MUTED,
        );
    }

    fn nav_status_card(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 92.0), egui::Sense::hover());
        let painter = ui.painter();
        theme::paint_surface(painter, rect, SURFACE, BORDER, RADIUS_SM);

        let capturing = self.status.running;
        let recording = self.status.recording;
        let (color, label) = if recording {
            (PINK, "Tam kayıt sürüyor")
        } else if capturing {
            (OK, "Tampon hazır")
        } else {
            (DANGER, "Kayıt durdu")
        };

        let glow = if capturing { anim::pulse(now(), 1.4) } else { 0.0 };
        theme::dot(
            painter,
            egui::pos2(rect.left() + 18.0, rect.top() + 22.0),
            color,
            glow,
        );
        painter.text(
            egui::pos2(rect.left() + 30.0, rect.top() + 22.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.5),
            TEXT_SOFT,
        );

        let buffer = self.status.buffer_seconds.max(1) as f32;
        let ratio = (self.status.fill_seconds as f32 / buffer).clamp(0.0, 1.0);
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 14.0, rect.top() + 42.0),
            egui::vec2((rect.width() - 28.0).max(1.0), 6.0),
        );
        theme::progress(painter, bar, ratio, ACCENT, ACCENT_2);

        let text = format!(
            "{} / {}",
            human_duration(self.status.fill_seconds),
            human_duration(self.status.buffer_seconds as f64)
        );
        painter.text(
            egui::pos2(rect.left() + 14.0, rect.top() + 60.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(10.5),
            MUTED,
        );
        let hotkey = if self.draft.hotkeys.save_clip.is_empty() {
            "—".to_string()
        } else {
            self.draft.hotkeys.save_clip.clone()
        };
        painter.text(
            egui::pos2(rect.right() - 14.0, rect.top() + 60.0),
            egui::Align2::RIGHT_TOP,
            format!("klip: {}", hotkey),
            egui::FontId::proportional(10.5),
            MUTED_DIM,
        );

        if let Some(error) = &self.status.error {
            painter.text(
                egui::pos2(rect.left() + 14.0, rect.top() + 76.0),
                egui::Align2::LEFT_TOP,
                truncate_chars(error, 32),
                egui::FontId::proportional(10.0),
                DANGER,
            );
        }
    }

    // ------------------------------------------------------------------ üst bar
    fn draw_top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(theme::TOP_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.vertical(|ui| {
                        let (title, subtitle) = self.page_text();
                        ui.label(egui::RichText::new(title).size(19.0).strong().color(TEXT));
                        ui.label(egui::RichText::new(subtitle).size(11.5).color(MUTED));
                    });

                    if self.view == View::Library {
                        ui.add_space(18.0);
                        let width = (ui.available_width() - 420.0).clamp(160.0, 340.0);
                        let search = egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Klip veya oyun ara…")
                            .desired_width(width);
                        if ui.add(search).changed() {
                            self.last_refresh = 0.0;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save_enabled = self.status.running;
                        let label = if self.draft.hotkeys.save_clip.is_empty() {
                            "Klip kaydet".to_string()
                        } else {
                            format!("Klip kaydet · {}", self.draft.hotkeys.save_clip)
                        };
                        if save_enabled {
                            if theme::primary_button(ui, &label).clicked() {
                                self.save_clip();
                            }
                        } else if ui
                            .add_enabled(false, egui::Button::new(label))
                            .clicked()
                        {
                            self.save_clip();
                        }

                        ui.add_space(8.0);
                        let record_label = if self.status.recording {
                            format!("Kaydı bitir · {}", human_duration(self.status.recording_seconds))
                        } else {
                            "Tam kayıt".to_string()
                        };
                        let record_button = egui::Button::new(
                            egui::RichText::new(record_label)
                                .size(12.5)
                                .color(if self.status.recording { PINK } else { TEXT_SOFT }),
                        )
                        .fill(if self.status.recording {
                            anim::with_alpha(PINK, 0.14)
                        } else {
                            SURFACE_HI
                        });
                        if ui.add(record_button).clicked() {
                            self.toggle_record();
                        }

                        ui.add_space(12.0);
                        self.pill_status(ui);
                    });
                });
            });
    }

    fn page_text(&self) -> (&'static str, String) {
        match self.view {
            View::Library => (
                "Kütüphanen",
                format!(
                    "{} klip · {} · son klip {}",
                    self.stats.count,
                    human_bytes(self.stats.size),
                    self.clips
                        .first()
                        .map(|clip| time_ago(clip.meta.created_at))
                        .unwrap_or_else(|| "yok".to_string())
                ),
            ),
            View::Live => ("Canlı", "Kayıt motorunun durumu ve günlük".to_string()),
            View::Games => ("Oyunlar", "Algılama ve oyun bazlı klipler".to_string()),
            View::Settings => ("Ayarlar", "Kayıt, ses, kısayol ve kitaplık".to_string()),
        }
    }

    fn pill_status(&self, ui: &mut egui::Ui) {
        let (color, text) = if self.status.recording {
            (PINK, "kayıt")
        } else if self.status.ready {
            (OK, "hazır")
        } else if self.status.running {
            (GOLD, "doluyor")
        } else {
            (DANGER, "durdu")
        };
        let galley = ui.painter().layout_no_wrap(
            text.to_string(),
            egui::FontId::proportional(11.0),
            color,
        );
        let size = egui::vec2(galley.size().x + 34.0, 26.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter();
        theme::paint_surface(painter, rect, anim::with_alpha(color, 0.12), anim::with_alpha(color, 0.4), 999.0);
        let glow = if self.status.running { anim::pulse(now(), 1.5) } else { 0.0 };
        theme::dot(painter, egui::pos2(rect.left() + 13.0, rect.center().y), color, glow);
        painter.text(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(11.0),
            color,
        );
    }

    // ------------------------------------------------------------------ kütüphane
    fn view_library(&mut self, ui: &mut egui::Ui) {
        self.library_toolbar(ui);
        ui.add_space(14.0);

        if !self.status.running && self.stats.count == 0 {
            self.onboarding(ui);
            ui.add_space(16.0);
        }

        if self.clips.is_empty() {
            self.library_empty(ui);
            return;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let available = ui.available_width();
                let gap = 16.0;
                let columns = ((available / 290.0).floor() as usize).max(1);
                let card_width =
                    ((available - gap * (columns as f32 - 1.0)) / columns as f32).max(180.0);
                let clips = self.clips.clone();
                let mut index = 0usize;
                for chunk in clips.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for clip in chunk {
                            self.clip_card(ui, clip, card_width, index);
                            index += 1;
                        }
                    });
                    ui.add_space(gap);
                }
                ui.add_space(8.0);
            });
    }

    fn library_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if theme::segment(ui, "Tümü", self.filter == Filter::All).clicked() {
                self.filter = Filter::All;
                self.last_refresh = 0.0;
            }
            if theme::segment(ui, "Favoriler", self.filter == Filter::Favorites).clicked() {
                self.filter = Filter::Favorites;
                self.last_refresh = 0.0;
            }
            ui.add_space(10.0);
            let sorts = [
                (SortKey::Newest, "En yeni"),
                (SortKey::Oldest, "En eski"),
                (SortKey::Longest, "En uzun"),
                (SortKey::Largest, "En büyük"),
                (SortKey::Title, "A-Z"),
            ];
            for (key, label) in sorts {
                if theme::segment(ui, label, self.sort == key).clicked() {
                    self.sort = key;
                    self.last_refresh = 0.0;
                }
            }
            if self.game_filter.is_some() || !self.search.is_empty() {
                ui.add_space(10.0);
                if theme::segment(ui, "Filtreyi temizle", false).clicked() {
                    self.game_filter = None;
                    self.search.clear();
                    self.last_refresh = 0.0;
                }
            }
        });

        let games = self.stats.games.clone();
        if games.len() > 1 {
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if theme::segment(ui, "Tüm oyunlar", self.game_filter.is_none()).clicked() {
                    self.game_filter = None;
                    self.last_refresh = 0.0;
                }
                for (name, count) in games.iter().take(12) {
                    let label = format!("{} · {}", truncate_chars(name, 18), count);
                    let active = self.game_filter.as_deref() == Some(name.as_str());
                    if theme::segment(ui, &label, active).clicked() {
                        self.game_filter = if active { None } else { Some(name.clone()) };
                        self.last_refresh = 0.0;
                    }
                }
            });
        }
    }

    fn onboarding(&mut self, ui: &mut egui::Ui) {
        theme::card_frame()
            .fill(SURFACE)
            .stroke(egui::Stroke::new(1.0, anim::with_alpha(ACCENT, 0.45)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Hoş geldin")
                                .size(16.0)
                                .strong()
                                .color(TEXT),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(
                                "LiMedal arka planda kayıt tutar. Oyun oynarken kısayola bastığında\
                                 son saniyeler klibe dönüşür — hiçbir şeyi sonradan kesmek zorunda kalmazsın.",
                            )
                            .size(12.0)
                            .color(MUTED),
                        );
                        ui.add_space(10.0);
                        for (ok, text) in [
                            (true, "1. Kayıt motoru klipleri yakalar (aşağıdaki düğme)"),
                            (true, "2. Oyun oynarken F9'a bas"),
                            (true, "3. Klip kütüphanende belirir — düzenle, kırp, paylaş"),
                        ] {
                            ui.horizontal(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 6.0, anim::with_alpha(ACCENT, 0.18));
                                ui.painter().circle_filled(rect.center(), 2.5, if ok { OK } else { MUTED });
                                ui.label(egui::RichText::new(text).size(12.0).color(TEXT_SOFT));
                            });
                        }
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if theme::primary_button(ui, "Kaydı başlat").clicked() {
                                self.restart_capture();
                            }
                            ui.add_space(8.0);
                            if ui.button("Sistem kontrolü").clicked() {
                                self.doctor = self.core.doctor();
                                self.doctor_at = now();
                                self.set_view(View::Settings);
                            }
                        });
                    });
                });
            });
    }

    fn library_empty(&mut self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 96.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.circle_filled(rect.center(), 46.0, anim::with_alpha(ACCENT, 0.10));
            painter.rect_stroke(
                egui::Rect::from_center_size(rect.center(), egui::vec2(34.0, 26.0)),
                7.0,
                egui::Stroke::new(1.6, ACCENT_HI),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(rect.center().x - 3.0, rect.center().y - 6.0),
                    egui::pos2(rect.center().x - 3.0, rect.center().y + 6.0),
                    egui::pos2(rect.center().x + 6.0, rect.center().y),
                ],
                ACCENT_HI,
                egui::Stroke::NONE,
            ));
            ui.add_space(14.0);
            ui.label(egui::RichText::new("Henüz klip yok").size(17.0).strong().color(TEXT));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "Oyun oynarken {} tuşuna bas; son {} saniye buraya düşer.",
                    self.draft.hotkeys.save_clip, self.draft.app.clip_seconds_default
                ))
                .size(12.5)
                .color(MUTED),
            );
            ui.add_space(16.0);
            if self.status.running {
                if theme::primary_button(ui, "Şimdi klip al").clicked() {
                    self.save_clip();
                }
            } else if theme::primary_button(ui, "Kaydı başlat").clicked() {
                self.restart_capture();
            }
        });
    }

    fn clip_card(&mut self, ui: &mut egui::Ui, clip: &Clip, width: f32, index: usize) {
        let thumb_height = width * 9.0 / 16.0;
        let height = thumb_height + 58.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

        let hover = ui
            .ctx()
            .animate_bool(egui::Id::new(("clip-hover", clip.meta.id.as_str())), response.hovered());
        let appear = anim::stagger(
            index,
            anim::progress((now() - self.view_entered_at) as f32, 0.5),
            0.04,
        );
        let lift = hover * 4.0 + (1.0 - appear) * 12.0;
        let paint_rect = rect.translate(egui::vec2(0.0, -lift));
        let selected = self.selected.as_deref() == Some(clip.meta.id.as_str());

        let painter = ui.painter();
        if hover > 0.02 {
            theme::paint_shadow(painter, paint_rect, RADIUS, hover * 0.9);
        }
        let border = if selected {
            ACCENT
        } else {
            anim::lerp_color(BORDER, BORDER_HI, hover)
        };
        theme::paint_surface(
            painter,
            paint_rect,
            anim::lerp_color(SURFACE, SURFACE_HI, hover),
            border,
            RADIUS,
        );

        let thumb_rect = egui::Rect::from_min_size(
            paint_rect.min + egui::vec2(0.0, 0.0),
            egui::vec2(paint_rect.width(), thumb_height),
        );
        // Küçük resim: yuvarlatılmış üst köşeler için içeri çekilmiş alan.
        let image_rect = thumb_rect.shrink2(egui::vec2(6.0, 6.0));
        let key = format!("thumb:{}", clip.meta.id);
        let tint = egui::Color32::from_rgba_unmultiplied(
            255,
            255,
            255,
            (255.0 * (0.35 + 0.65 * appear)) as u8,
        );
        match self.texture_for(ui.ctx(), &clip.thumb(), &key) {
            Some(texture) => {
                painter.image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    tint,
                );
            }
            None => {
                painter.rect_filled(image_rect, RADIUS_SM, SURFACE_LO);
                painter.text(
                    image_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "önizleme yok",
                    egui::FontId::proportional(11.0),
                    MUTED_DIM,
                );
            }
        }

        // Süre etiketi
        let badge = format!(" {}", clip.meta.duration_text());
        let galley =
            painter.layout_no_wrap(badge, egui::FontId::proportional(10.5), TEXT);
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(
                image_rect.right() - galley.size().x - 16.0,
                image_rect.bottom() - 22.0,
            ),
            egui::vec2(galley.size().x + 12.0, 18.0),
        );
        painter.rect_filled(badge_rect, 6.0, egui::Color32::from_rgba_unmultiplied(6, 6, 10, 200));
        painter.galley(
            egui::pos2(badge_rect.left() + 6.0, badge_rect.center().y - galley.size().y * 0.5),
            galley,
            TEXT,
        );

        if !clip.meta.game.is_empty() {
            painter.text(
                egui::pos2(image_rect.left() + 8.0, image_rect.top() + 9.0),
                egui::Align2::LEFT_TOP,
                truncate_chars(&clip.meta.game, 20),
                egui::FontId::proportional(10.5),
                anim::lerp_color(MUTED, TEXT, hover),
            );
        }

        if clip.meta.favorite || hover > 0.5 {
            let center = egui::pos2(image_rect.right() - 16.0, image_rect.top() + 16.0);
            painter.circle_filled(center, 10.0, egui::Color32::from_rgba_unmultiplied(6, 6, 10, 190));
            paint_star(painter, center, 6.0, if clip.meta.favorite { GOLD } else { TEXT });
        }

        painter.text(
            egui::pos2(paint_rect.left() + 12.0, thumb_rect.bottom() + 10.0),
            egui::Align2::LEFT_TOP,
            truncate_chars(&clip.meta.title, 40),
            egui::FontId::proportional(13.0),
            TEXT,
        );
        painter.text(
            egui::pos2(paint_rect.left() + 12.0, thumb_rect.bottom() + 30.0),
            egui::Align2::LEFT_TOP,
            format!("{} · {}", time_ago(clip.meta.created_at), clip.meta.size_text()),
            egui::FontId::proportional(10.5),
            MUTED,
        );

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        if response.double_clicked() {
            self.open_editor(clip.clone());
            return;
        }
        if response.clicked() {
            if selected {
                self.selected = None;
                self.strip = None;
            } else {
                self.selected = Some(clip.meta.id.clone());
                self.strip = None;
                self.rename_buffer = clip.meta.title.clone();
                self.rename_id = None;
                self.trim_id = None;
                self.share_url = None;
                self.confirm_delete = None;
            }
        }

        let clip_id = clip.meta.id.clone();
        let favorite = clip.meta.favorite;
        let dir = clip.dir.clone();
        response.context_menu(|ui| {
            ui.set_min_width(150.0);
            if ui.button("Düzenle").clicked() {
                self.open_editor(clip.clone());
                ui.close_menu();
            }
            if ui.button(if favorite { "Favoriden çıkar" } else { "Favoriye ekle" }).clicked() {
                self.core.library.toggle_favorite(&clip_id);
                self.invalidate();
                ui.close_menu();
            }
            if ui.button("Klasörü aç").clicked() {
                self.core.open_path(&dir);
                ui.close_menu();
            }
            ui.separator();
            if ui
                .add(egui::Button::new(egui::RichText::new("Sil").color(DANGER)).frame(false))
                .clicked()
            {
                self.confirm_delete = Some(clip_id.clone());
                ui.close_menu();
            }
        });
    }

    // ------------------------------------------------------------------ çekmece
    fn draw_drawer(&mut self, ctx: &egui::Context) {
        let clip = match self.selected_clip() {
            Some(clip) => clip,
            None => return,
        };
        let screen = ctx.screen_rect();
        let t = ctx.animate_bool(egui::Id::new("drawer-open"), true);

        egui::Area::new(egui::Id::new("drawer-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(4, 4, 8, (140.0 * t) as u8),
                );
                if response.clicked() {
                    self.selected = None;
                    self.strip = None;
                    self.confirm_delete = None;
                }
            });

        egui::Area::new(egui::Id::new("clip-drawer"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(8.0, 8.0))
            .show(ctx, |ui| {
                ui.set_width(392.0);
                ui.set_max_height(screen.height() - 16.0);
                egui::Frame::none()
                    .fill(SURFACE)
                    .stroke(egui::Stroke::new(1.0, BORDER_HI))
                    .rounding(egui::Rounding::same(RADIUS))
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        self.drawer_body(ui, &clip);
                    });
            });
    }

    fn drawer_body(&mut self, ui: &mut egui::Ui, clip: &Clip) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Klip").size(12.0).color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Kapat").clicked() {
                    self.selected = None;
                    self.strip = None;
                }
            });
        });
        ui.add_space(8.0);

        // Önizleme (film şeridi varsa o, yoksa küçük resim)
        if self.strip.is_none() {
            if let Some(path) = self.core.library.ensure_strip(clip) {
                self.strip = load_texture(ui.ctx(), &path);
            }
        }
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 176.0),
            egui::Sense::click(),
        );
        let painter = ui.painter();
        painter.rect_filled(rect, RADIUS_SM, BG_DEEP);
        if let Some(texture) = self.strip.clone() {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else if let Some(texture) =
            self.texture_for(ui.ctx(), &clip.thumb(), &format!("thumb:{}", clip.meta.id))
        {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        // Oynat düğmesi
        let center = rect.center();
        painter.circle_filled(center, 24.0, egui::Color32::from_rgba_unmultiplied(6, 6, 10, 190));
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(center.x - 4.0, center.y - 8.0),
                egui::pos2(center.x - 4.0, center.y + 8.0),
                egui::pos2(center.x + 9.0, center.y),
            ],
            TEXT,
            egui::Stroke::NONE,
        ));
        if response.clicked() {
            self.core.open_path(&clip.video());
        }

        ui.add_space(10.0);

        // Başlık (düzenlenebilir)
        let mut buffer = match &self.rename_id {
            Some(id) if id == &clip.meta.id => self.rename_buffer.clone(),
            _ => clip.meta.title.clone(),
        };
        let edit = ui.add(
            egui::TextEdit::singleline(&mut buffer)
                .desired_width(f32::INFINITY)
                .font(egui::TextStyle::Body),
        );
        if edit.changed() {
            self.rename_buffer = buffer.clone();
            self.rename_id = Some(clip.meta.id.clone());
        }
        if edit.lost_focus() && self.rename_id.is_some() {
            if let Some(id) = self.rename_id.clone() {
                self.core.library.set_title(&id, &self.rename_buffer);
                self.rename_id = None;
                self.invalidate();
            }
        }

        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(format!(
                "{} · {} · {}{}",
                if clip.meta.game.trim().is_empty() { "Etiketsiz" } else { clip.meta.game.as_str() },
                clip.meta.duration_text(),
                clip.meta.size_text(),
                if clip.meta.resolution_text().is_empty() {
                    String::new()
                } else {
                    format!(" · {}", clip.meta.resolution_text())
                }
            ))
            .size(11.5)
            .color(MUTED),
        );

        ui.add_space(12.0);
        if ui
            .add(
                egui::Button::new(egui::RichText::new("Düzenle").strong().color(egui::Color32::WHITE))
                    .fill(ACCENT)
                    .min_size(egui::vec2(ui.available_width(), 34.0)),
            )
            .clicked()
        {
            self.open_editor(clip.clone());
            return;
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Oynat").clicked() {
                self.core.open_path(&clip.video());
            }
            if ui.button("Klasör").clicked() {
                self.core.open_path(&clip.dir);
            }
            let fav_label = if clip.meta.favorite { "Favoriden çıkar" } else { "Favori" };
            if ui.button(fav_label).clicked() {
                self.core.library.toggle_favorite(&clip.meta.id);
                self.invalidate();
            }
            if ui.button("Kırp").clicked() {
                self.trim_id = Some(clip.meta.id.clone());
                self.trim_start = 0.0;
                self.trim_end = clip.meta.duration as f32;
            }
            if ui.button("Paylaş").clicked() {
                self.share_url = self.core.share_url(clip);
                if self.share_url.is_none() {
                    self.core.toast(
                        "Paylaşım kapalı — Ayarlar'da 'Ağda paylaşım'ı aç.".to_string(),
                        ToastKind::Error,
                    );
                }
            }
            if ui
                .add(egui::Button::new(egui::RichText::new("Sil").color(DANGER)).frame(false))
                .clicked()
            {
                self.confirm_delete = Some(clip.meta.id.clone());
            }
        });

        if let Some(url) = self.share_url.clone() {
            ui.add_space(10.0);
            theme::inner_frame().show(ui, |ui| {
                ui.label(egui::RichText::new("Ağ bağlantısı").size(10.5).color(MUTED));
                ui.label(
                    egui::RichText::new(url.clone())
                        .size(10.5)
                        .monospace()
                        .color(ACCENT_2),
                );
                if ui.button("Kopyala").clicked() {
                    self.copy_text(ui.ctx(), &url);
                }
            });
        }

        if self.trim_id.as_deref() == Some(clip.meta.id.as_str()) {
            ui.add_space(12.0);
            theme::inner_frame().show(ui, |ui| {
                ui.label(egui::RichText::new("Kırpma aralığı").size(11.5).color(TEXT_SOFT));
                ui.add_space(4.0);
                let duration = clip.meta.duration.max(0.5) as f32;
                ui.add(egui::Slider::new(&mut self.trim_start, 0.0..=duration).text("başlangıç"));
                ui.add(egui::Slider::new(&mut self.trim_end, 0.0..=duration).text("bitiş"));
                ui.label(
                    egui::RichText::new(format!(
                        "{} – {}",
                        human_duration(self.trim_start as f64),
                        human_duration(self.trim_end as f64)
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("İptal").clicked() {
                        self.trim_id = None;
                    }
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("Kırp ve kaydet").color(egui::Color32::WHITE))
                                .fill(ACCENT),
                        )
                        .clicked()
                    {
                        let start = self.trim_start as f64;
                        let end = self.trim_end as f64;
                        let id = clip.meta.id.clone();
                        let core = self.core.clone();
                        self.trim_id = None;
                        std::thread::spawn(move || match core.library.trim(&id, start, end) {
                            Ok(new_clip) => core.toast(
                                format!("Kırpıldı · {}", new_clip.meta.duration_text()),
                                ToastKind::Ok,
                            ),
                            Err(err) => {
                                core.toast(format!("Kırpma başarısız: {}", err), ToastKind::Error)
                            }
                        });
                    }
                });
            });
        }

        if self.confirm_delete.as_deref() == Some(clip.meta.id.as_str()) {
            ui.add_space(12.0);
            theme::inner_frame()
                .fill(anim::with_alpha(DANGER, 0.08))
                .stroke(egui::Stroke::new(1.0, anim::with_alpha(DANGER, 0.45)))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Bu klip kalıcı olarak silinsin mi?")
                            .size(12.0)
                            .color(DANGER),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Vazgeç").clicked() {
                            self.confirm_delete = None;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Sil").color(egui::Color32::WHITE),
                                )
                                .fill(DANGER),
                            )
                            .clicked()
                        {
                            self.core.library.delete(&clip.meta.id);
                            self.textures.remove(&format!("thumb:{}", clip.meta.id));
                            self.confirm_delete = None;
                            self.selected = None;
                            self.strip = None;
                            self.invalidate();
                        }
                    });
                });
        }
    }

    // ------------------------------------------------------------------ canlı
    fn view_live(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    let left = (ui.available_width() * 0.46).max(320.0);
                    ui.vertical(|ui| {
                        ui.set_width(left);
                        self.live_status_card(ui);
                        ui.add_space(14.0);
                        self.live_checks_card(ui);
                    });

                    ui.add_space(16.0);

                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        self.live_log_card(ui);
                    });
                });
                ui.add_space(20.0);
            });
    }

    fn live_status_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.horizontal(|ui| {
                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                let (color, text) = if self.status.recording {
                    (PINK, "Tam kayıt sürüyor")
                } else if self.status.ready {
                    (OK, "Klip almaya hazır")
                } else if self.status.running {
                    (GOLD, "Tampon doluyor")
                } else {
                    (DANGER, "Kayıt durdu")
                };
                let glow = if self.status.running { anim::pulse(now(), 1.4) } else { 0.0 };
                theme::dot(ui.painter(), dot_rect.center(), color, glow);
                ui.label(egui::RichText::new(text).size(13.0).strong().color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(self.status.encoder_label.clone())
                            .size(11.0)
                            .color(MUTED),
                    );
                });
            });

            ui.add_space(14.0);
            // Dairesel tampon göstergesi
            let size = egui::vec2(ui.available_width(), 170.0);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let center = rect.center();
            let radius = 62.0;
            let painter = ui.painter();
            painter.circle_stroke(center, radius, egui::Stroke::new(9.0, SURFACE_HI));
            let buffer = self.status.buffer_seconds.max(1) as f32;
            let ratio = (self.status.fill_seconds as f32 / buffer).clamp(0.0, 1.0);
            let color = if self.status.recording { PINK } else { ACCENT };
            paint_arc(painter, center, radius, ratio, color, 9.0);
            painter.text(
                center - egui::vec2(0.0, 8.0),
                egui::Align2::CENTER_CENTER,
                format!("{}s", self.status.fill_seconds as i64),
                egui::FontId::proportional(26.0),
                TEXT,
            );
            painter.text(
                center + egui::vec2(0.0, 16.0),
                egui::Align2::CENTER_CENTER,
                format!("tampon / {}s", self.status.buffer_seconds),
                egui::FontId::proportional(10.5),
                MUTED_DIM,
            );

            ui.add_space(6.0);
            let tiles = [
                ("Çözünürlük", if self.status.resolution.is_empty() { "—".to_string() } else { self.status.resolution.clone() }),
                ("FPS", format!("{}", self.status.fps)),
                ("Segment", format!("{}", self.status.segments)),
                ("Oyun", self.game.clone().unwrap_or_else(|| "—".to_string())),
            ];
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 24.0) / 2.0;
                ui.vertical(|ui| {
                    ui.set_width(width);
                    for (label, value) in tiles.iter().take(2) {
                        self.stat_tile(ui, label, value);
                        ui.add_space(8.0);
                    }
                });
                ui.vertical(|ui| {
                    ui.set_width(width);
                    for (label, value) in tiles.iter().skip(2) {
                        self.stat_tile(ui, label, value);
                        ui.add_space(8.0);
                    }
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::primary_button(ui, "Yeniden başlat").clicked() {
                    self.restart_capture();
                }
                if ui.button("Durdur").clicked() {
                    self.stop_capture();
                }
                if ui.button("Kayıt / bitir").clicked() {
                    self.toggle_record();
                }
            });

            if let Some(error) = &self.status.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error.clone()).size(11.0).color(DANGER));
            }
            if !self.status.note.is_empty() {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(self.status.note.clone()).size(10.5).color(MUTED));
            }
        });
    }

    fn stat_tile(&self, ui: &mut egui::Ui, label: &str, value: &str) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 48.0), egui::Sense::hover());
        let painter = ui.painter();
        theme::paint_surface(painter, rect, SURFACE_LO, BORDER, RADIUS_SM);
        painter.text(
            egui::pos2(rect.left() + 11.0, rect.top() + 9.0),
            egui::Align2::LEFT_TOP,
            label.to_uppercase(),
            egui::FontId::proportional(9.5),
            MUTED_DIM,
        );
        painter.text(
            egui::pos2(rect.left() + 11.0, rect.bottom() - 9.0),
            egui::Align2::LEFT_BOTTOM,
            truncate_chars(value, 22),
            egui::FontId::proportional(12.5),
            TEXT_SOFT,
        );
    }

    fn live_checks_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.label(egui::RichText::new("Sistem").size(12.0).strong().color(TEXT));
            ui.add_space(6.0);

            let hotkeys = self.core.hotkey_state();
            let audio = self.core.audio.status();
            let rows = [
                (self.core.ff.available(), "ffmpeg", self.core.ff.path()),
                (
                    hotkeys.available,
                    "Global kısayollar",
                    if hotkeys.available {
                        format!("{} klavye dinleniyor", hotkeys.devices)
                    } else {
                        hotkeys.reason.clone()
                    },
                ),
                (
                    audio.available && audio.error.is_none(),
                    "Ses kaynakları",
                    audio
                        .sources
                        .iter()
                        .find(|source| source.kind == crate::audio::SourceKind::Monitor)
                        .map(|source| source.label.clone())
                        .unwrap_or_else(|| "monitör yok".to_string()),
                ),
                (true, "Hyprland", hyprland::summary()),
            ];
            for (ok, label, detail) in rows {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        3.0,
                        if ok { OK } else { DANGER },
                    );
                    ui.label(egui::RichText::new(label).size(11.5).color(TEXT_SOFT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(truncate_chars(&detail, 30))
                                .size(10.5)
                                .color(MUTED_DIM),
                        );
                    });
                });
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Tanı çalıştır").clicked() {
                    self.doctor = self.core.doctor();
                    self.doctor_at = now();
                }
                if ui.button("Klasörü aç").clicked() {
                    let root = self.core.library.root();
                    self.core.open_path(&root);
                }
            });
            if !self.doctor.is_empty() && now() - self.doctor_at < 120.0 {
                ui.add_space(8.0);
                for (ok, label, detail) in self.doctor.clone().into_iter().take(16) {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {} · {}",
                            if ok { "✓" } else { "!" },
                            label,
                            truncate_chars(&detail, 40)
                        ))
                        .size(10.0)
                        .monospace()
                        .color(if ok { MUTED } else { DANGER }),
                    );
                }
            }
        });
    }

    fn live_log_card(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Günlük").size(12.0).strong().color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Yenile").clicked() {
                        self.log_tail = self.core.log.tail(200);
                    }
                });
            });
            ui.add_space(6.0);
            let height = (ui.available_height() - 40.0).max(240.0);
            egui::ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.log_tail.is_empty() {
                        ui.label(
                            egui::RichText::new("henüz kayıt yok")
                                .size(11.0)
                                .color(MUTED_DIM),
                        );
                    }
                    for line in self.log_tail.iter() {
                        let color = if line.contains("[ERROR]") {
                            DANGER
                        } else if line.contains("[WARN") {
                            GOLD
                        } else {
                            MUTED
                        };
                        ui.label(
                            egui::RichText::new(line.clone())
                                .size(10.0)
                                .monospace()
                                .color(color),
                        );
                    }
                });
        });
    }

    // ------------------------------------------------------------------ oyunlar
    fn view_games(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width() - 4.0);
                    ui.label(egui::RichText::new("ŞU AN").size(10.0).color(MUTED_DIM));
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(
                            self.game.clone().unwrap_or_else(|| "Oyun algılanmadı".to_string()),
                        )
                        .size(22.0)
                        .strong()
                        .color(TEXT),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Oyun süreçleri /proc üzerinden taranır; Steam, Proton ve Wine oyunları\
                             yollarından tanınır.",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
                    ui.add_space(10.0);
                    let mut detection = self.draft.app.game_detection;
                    if theme::switch(ui, "game-detect", &mut detection) {
                        self.draft.app.game_detection = detection;
                        self.core.apply_config(self.draft.clone());
                        self.draft = self.core.config();
                        self.draft_dirty = false;
                    }
                    ui.label(
                        egui::RichText::new(if self.draft.app.game_detection {
                            "Otomatik algılama açık"
                        } else {
                            "Otomatik algılama kapalı"
                        })
                        .size(11.0)
                        .color(MUTED),
                    );
                });

                ui.add_space(16.0);

                ui.horizontal_top(|ui| {
                    let width = (ui.available_width() - 16.0) / 2.0;
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        theme::card_frame().show(ui, |ui| {
                            ui.set_width(ui.available_width() - 4.0);
                            ui.label(egui::RichText::new("Algılanan süreçler").size(12.0).strong());
                            ui.add_space(6.0);
                            if self.detected.is_empty() {
                                ui.label(
                                    egui::RichText::new("Şu an oyun süreci görünmüyor.")
                                        .size(11.5)
                                        .color(MUTED),
                                );
                            }
                            for game in self.detected.iter().take(10) {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(truncate_chars(&game.name, 24))
                                            .size(11.5)
                                            .color(TEXT_SOFT),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("pid {}", game.pid))
                                                    .size(10.0)
                                                    .color(MUTED_DIM),
                                            );
                                        },
                                    );
                                });
                            }
                        });
                    });
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        theme::card_frame().show(ui, |ui| {
                            ui.set_width(ui.available_width() - 4.0);
                            ui.label(egui::RichText::new("Oyun bazlı klipler").size(12.0).strong());
                            ui.add_space(6.0);
                            let games = self.stats.games.clone();
                            if games.is_empty() {
                                ui.label(
                                    egui::RichText::new("Henüz oyun etiketli klip yok.")
                                        .size(11.5)
                                        .color(MUTED),
                                );
                            }
                            for (name, count) in games.iter().take(12) {
                                let active = self.game_filter.as_deref() == Some(name.as_str());
                                if theme::segment(
                                    ui,
                                    &format!("{} · {}", truncate_chars(name, 22), count),
                                    active,
                                )
                                .clicked()
                                {
                                    self.game_filter = if active { None } else { Some(name.clone()) };
                                    self.set_view(View::Library);
                                }
                            }
                        });
                    });
                });
                ui.add_space(20.0);
            });
    }

    // ------------------------------------------------------------------ ayarlar
    fn view_settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = (ui.available_width() - 16.0) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        self.settings_capture(ui);
                        ui.add_space(16.0);
                        self.settings_hotkeys(ui);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        self.settings_audio(ui);
                        ui.add_space(16.0);
                        self.settings_library(ui);
                    });
                });

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, "Ayarları kaydet").clicked() {
                        let was_running = self.status.running;
                        self.core.apply_config(self.draft.clone());
                        self.draft = self.core.config();
                        self.draft_dirty = false;
                        if was_running {
                            self.restart_capture();
                        }
                        self.core
                            .toast("Ayarlar kaydedildi".to_string(), ToastKind::Ok);
                        self.invalidate();
                    }
                    if ui.button("Geri al").clicked() {
                        self.draft = self.core.config();
                        self.draft_dirty = false;
                    }
                    if self.draft_dirty {
                        ui.label(
                            egui::RichText::new("kaydedilmemiş değişiklikler")
                                .size(11.0)
                                .color(GOLD),
                        );
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Kayıt ayarları değişince motor kendiliğinden yeniden başlar.",
                    )
                    .size(10.5)
                    .color(MUTED_DIM),
                );
                ui.add_space(20.0);
            });
    }

    fn settings_capture(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            section_title(ui, "Kayıt");

            ui.label(egui::RichText::new("Kalite").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                for (value, label) in [
                    ("low", "Düşük"),
                    ("medium", "Orta"),
                    ("high", "Yüksek"),
                    ("ultra", "Ultra"),
                ] {
                    if theme::segment(ui, label, self.draft.capture.quality == value).clicked() {
                        self.draft.capture.quality = value.to_string();
                        self.draft_dirty = true;
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(egui::RichText::new("FPS").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                for value in [30u32, 60, 120, 144] {
                    if theme::segment(ui, &value.to_string(), self.draft.capture.fps == value).clicked()
                    {
                        self.draft.capture.fps = value;
                        self.draft_dirty = true;
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Çözünürlük").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                let options: [(&str, Option<u32>, Option<u32>); 4] = [
                    ("Otomatik", None, None),
                    ("720p", Some(1280), Some(720)),
                    ("1080p", Some(1920), Some(1080)),
                    ("1440p", Some(2560), Some(1440)),
                ];
                for (label, width, height) in options {
                    let active =
                        self.draft.capture.width == width && self.draft.capture.height == height;
                    if theme::segment(ui, label, active).clicked() {
                        self.draft.capture.width = width;
                        self.draft.capture.height = height;
                        self.draft_dirty = true;
                    }
                }
            });

            ui.add_space(8.0);
            let mut buffer = self.draft.capture.buffer_seconds;
            if ui
                .add(egui::Slider::new(&mut buffer, 5..=600).suffix(" sn").text("Tampon"))
                .changed()
            {
                self.draft.capture.buffer_seconds = buffer;
                self.draft_dirty = true;
            }
            let mut segment = self.draft.capture.segment_seconds;
            if ui
                .add(egui::Slider::new(&mut segment, 1..=10).suffix(" sn").text("Segment"))
                .changed()
            {
                self.draft.capture.segment_seconds = segment;
                self.draft_dirty = true;
            }

            ui.add_space(4.0);
            let mut cursor = self.draft.capture.draw_cursor;
            if theme::switch(ui, "draw-cursor", &mut cursor) {
                self.draft.capture.draw_cursor = cursor;
                self.draft_dirty = true;
            }
            ui.label(egui::RichText::new("Fare imlecini kaydet").size(11.0).color(MUTED));

            ui.add_space(10.0);
            ui.label(egui::RichText::new("Kodlayıcı").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                let current = self.draft.capture.encoder.clone();
                for (value, label) in [
                    ("auto", "Otomatik"),
                    ("h264_nvenc", "NVENC"),
                    ("h264_vaapi", "VA-API"),
                    ("h264_qsv", "QuickSync"),
                    ("libx264", "CPU"),
                ] {
                    if theme::segment(ui, label, current == value).clicked() {
                        self.draft.capture.encoder = value.to_string();
                        self.draft_dirty = true;
                    }
                }
            });
            ui.label(
                egui::RichText::new(format!("şu an: {}", self.status.encoder_label))
                    .size(10.5)
                    .color(MUTED_DIM),
            );
        });
    }

    fn settings_audio(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            section_title(ui, "Ses");

            let audio = self.core.audio.status();
            if let Some(error) = &audio.error {
                ui.label(egui::RichText::new(error.clone()).size(10.5).color(DANGER));
            }

            ui.label(egui::RichText::new("Oyun sesi").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                let current = self.draft.capture.audio.game_source.clone();
                if theme::segment(ui, "Sistem sesi", current == "auto").clicked() {
                    self.draft.capture.audio.game_source = "auto".to_string();
                    self.draft_dirty = true;
                }
                if theme::segment(ui, "Sessiz", current == "none").clicked() {
                    self.draft.capture.audio.game_source = "none".to_string();
                    self.draft_dirty = true;
                }
                for source in audio
                    .sources
                    .iter()
                    .filter(|source| source.kind == crate::audio::SourceKind::Monitor)
                    .take(4)
                {
                    if theme::segment(ui, &truncate_chars(&source.label, 22), current == source.name)
                        .clicked()
                    {
                        self.draft.capture.audio.game_source = source.name.clone();
                        self.draft_dirty = true;
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(egui::RichText::new("Mikrofon").size(11.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                let current = self.draft.capture.audio.mic_source.clone();
                if theme::segment(ui, "Kapalı", current.is_empty()).clicked() {
                    self.draft.capture.audio.mic_source = String::new();
                    self.draft_dirty = true;
                }
                if theme::segment(ui, "Otomatik", current == "auto").clicked() {
                    self.draft.capture.audio.mic_source = "auto".to_string();
                    self.draft_dirty = true;
                }
                for source in audio
                    .sources
                    .iter()
                    .filter(|source| source.kind == crate::audio::SourceKind::Input)
                    .take(4)
                {
                    if theme::segment(ui, &truncate_chars(&source.label, 22), current == source.name)
                        .clicked()
                    {
                        self.draft.capture.audio.mic_source = source.name.clone();
                        self.draft_dirty = true;
                    }
                }
            });

            ui.add_space(10.0);
            let mut game_volume = self.draft.capture.audio.game_volume;
            if ui
                .add(egui::Slider::new(&mut game_volume, 0.0..=2.0).text("oyun düzeyi"))
                .changed()
            {
                self.draft.capture.audio.game_volume = game_volume;
                self.draft_dirty = true;
            }
            let mut mic_volume = self.draft.capture.audio.mic_volume;
            if ui
                .add(egui::Slider::new(&mut mic_volume, 0.0..=2.0).text("mikrofon düzeyi"))
                .changed()
            {
                self.draft.capture.audio.mic_volume = mic_volume;
                self.draft_dirty = true;
            }
        });
    }

    fn settings_hotkeys(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            section_title(ui, "Kısayollar");

            let state = self.core.hotkey_state();
            if state.available {
                ui.label(
                    egui::RichText::new(format!("{} klavye dinleniyor", state.devices))
                        .size(11.0)
                        .color(OK),
                );
            } else {
                ui.label(
                    egui::RichText::new(truncate_chars(&state.reason, 60))
                        .size(10.5)
                        .color(DANGER),
                );
                ui.label(
                    egui::RichText::new("sudo usermod -aG input $USER")
                        .size(10.0)
                        .monospace()
                        .color(MUTED_DIM),
                );
            }

            ui.add_space(8.0);
            let mut enabled = self.draft.hotkeys.enabled;
            if theme::switch(ui, "hotkeys-enabled", &mut enabled) {
                self.draft.hotkeys.enabled = enabled;
                self.draft_dirty = true;
            }
            ui.label(egui::RichText::new("Kısayolları etkinleştir").size(11.0).color(MUTED));

            ui.add_space(8.0);
            settings_row(ui, "Klip kaydet", "Örnek: F9, ctrl+shift+k", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.hotkeys.save_clip)
                        .desired_width(120.0),
                );
            });
            settings_row(ui, "Kayıt aç/kapa", "boş = atanmaz", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.hotkeys.toggle_record)
                        .desired_width(120.0),
                );
            });
            if ui.ctx().input(|input| input.pointer.any_released()) {
                // metin alanları değiştiyse taslağı kirli işaretle
                if self.draft.hotkeys.save_clip != self.core.config().hotkeys.save_clip
                    || self.draft.hotkeys.toggle_record != self.core.config().hotkeys.toggle_record
                {
                    self.draft_dirty = true;
                }
            }
        });
    }

    fn settings_library(&mut self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width() - 4.0);
            section_title(ui, "Kitaplık");

            settings_row(ui, "Klasör", "kliplerin kaydedildiği yer", |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.app.library_dir)
                        .desired_width(170.0),
                );
            });
            ui.add_space(6.0);
            let mut seconds = self.draft.app.clip_seconds_default;
            if ui
                .add(egui::Slider::new(&mut seconds, 5..=300).suffix(" sn").text("Klip uzunluğu"))
                .changed()
            {
                self.draft.app.clip_seconds_default = seconds;
                self.draft_dirty = true;
            }
            let mut storage = self.draft.app.max_storage_gb;
            if ui
                .add(egui::Slider::new(&mut storage, 0..=500).suffix(" GB").text("Depolama sınırı"))
                .changed()
            {
                self.draft.app.max_storage_gb = storage;
                self.draft_dirty = true;
            }

            ui.add_space(8.0);
            let mut autostart = self.draft.app.auto_start_capture;
            if theme::switch(ui, "auto-start", &mut autostart) {
                self.draft.app.auto_start_capture = autostart;
                self.draft_dirty = true;
            }
            ui.label(egui::RichText::new("Açılışta kaydı başlat").size(11.0).color(MUTED));

            let mut notifications = self.draft.app.notifications;
            if theme::switch(ui, "notifications", &mut notifications) {
                self.draft.app.notifications = notifications;
                self.draft_dirty = true;
            }
            ui.label(egui::RichText::new("Masaüstü bildirimleri").size(11.0).color(MUTED));

            let mut share = self.draft.app.share_enabled;
            if theme::switch(ui, "share", &mut share) {
                self.draft.app.share_enabled = share;
                self.draft_dirty = true;
            }
            ui.label(egui::RichText::new("Ağda paylaşım").size(11.0).color(MUTED));

            ui.add_space(10.0);
            if ui.button("Sistem kontrolünü çalıştır").clicked() {
                self.doctor = self.core.doctor();
                self.doctor_at = now();
            }
            if !self.doctor.is_empty() {
                ui.add_space(6.0);
                for (ok, label, detail) in self.doctor.clone().iter().take(14) {
                    ui.label(
                        egui::RichText::new(format!(
                            "[{}] {} · {}",
                            if *ok { "ok" } else { "!!" },
                            label,
                            truncate_chars(detail, 34)
                        ))
                        .size(10.0)
                        .monospace()
                        .color(if *ok { MUTED } else { DANGER }),
                    );
                }
            }
        });
    }

    // ------------------------------------------------------------------ yardım & splash
    fn draw_help(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("help-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(4, 4, 8, 170));
                if response.clicked() {
                    self.show_help = false;
                }
            });

        egui::Window::new("Yardım")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(
                    egui::RichText::new("Oyun oynarken klipleri böyle alırsın")
                        .size(13.0)
                        .strong(),
                );
                ui.add_space(8.0);
                for (key, text) in [
                    (self.draft.hotkeys.save_clip.as_str(), "son saniyeleri klip yap"),
                    (self.draft.hotkeys.toggle_record.as_str(), "tam kaydı aç / kapat"),
                    ("F1", "bu yardım penceresi"),
                    ("Boşluk", "düzenleyicide oynat / duraklat"),
                ] {
                    if key.is_empty() {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{:<10}", key))
                                .monospace()
                                .size(11.5)
                                .color(ACCENT_2),
                        );
                        ui.label(egui::RichText::new(text).size(11.5).color(MUTED));
                    });
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(
                        "Kısayollar sistem genelinde çalışır; oyun içinde de klip alabilirsin.",
                    )
                    .size(11.0)
                    .color(MUTED_DIM),
                );
                ui.add_space(8.0);
                if ui.button("Kapat").clicked() {
                    self.show_help = false;
                }
            });
    }

    fn draw_splash(&self, ctx: &egui::Context) {
        let elapsed = (now() - self.started_at) as f32;
        if elapsed >= 1.45 {
            return;
        }
        let fade = 1.0 - anim::ease_in_out(anim::progress(elapsed - 0.95, 0.45));
        let scale = anim::ease_out_back(anim::progress(elapsed, 0.7)).max(0.01);
        let rise = anim::ease_out_cubic(anim::progress(elapsed, 0.6));
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("limedal-splash"),
        ));
        painter.rect_filled(screen, 0.0, anim::with_alpha(BG_DEEP, fade));

        let center = screen.center() + egui::vec2(0.0, 26.0 * (1.0 - rise));
        let size = 74.0 * scale;
        let icon = egui::Rect::from_center_size(center - egui::vec2(0.0, 30.0), egui::vec2(size, size));
        painter.rect_filled(
            icon.expand(scale * 16.0),
            26.0 * scale,
            anim::with_alpha(ACCENT, 0.10 * fade),
        );
        painter.rect_filled(icon, 20.0 * scale, anim::with_alpha(ACCENT, fade));
        let c = icon.center();
        let s = 26.0 * scale;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x - s * 0.32, c.y - s * 0.5),
                egui::pos2(c.x - s * 0.32, c.y + s * 0.5),
                egui::pos2(c.x + s * 0.52, c.y),
            ],
            anim::with_alpha(egui::Color32::WHITE, fade),
            egui::Stroke::NONE,
        ));
        painter.text(
            center + egui::vec2(0.0, 46.0),
            egui::Align2::CENTER_CENTER,
            "LiMedal",
            egui::FontId::proportional(26.0),
            anim::with_alpha(TEXT, fade),
        );
        painter.text(
            center + egui::vec2(0.0, 72.0),
            egui::Align2::CENTER_CENTER,
            "oyun klipleri, hazır",
            egui::FontId::proportional(12.0),
            anim::with_alpha(MUTED, fade),
        );
        let bar = egui::Rect::from_center_size(
            center + egui::vec2(0.0, 98.0),
            egui::vec2(120.0, 3.0),
        );
        theme::progress(
            &painter,
            bar,
            anim::progress(elapsed, 1.0),
            anim::with_alpha(ACCENT, fade),
            anim::with_alpha(ACCENT_2, fade),
        );
        ctx.request_repaint();
    }

    fn draw_toasts(&self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    let alpha = anim::ease_out_cubic(anim::progress((now() - toast.at) as f32, 0.26));
                    let (color, icon) = match toast.kind {
                        ToastKind::Ok => (OK, "✓"),
                        ToastKind::Error => (DANGER, "!"),
                        ToastKind::Info => (ACCENT, "i"),
                    };
                    let galley = ui.painter().layout_no_wrap(
                        toast.text.clone(),
                        egui::FontId::proportional(12.0),
                        anim::with_alpha(TEXT, alpha),
                    );
                    let width = galley.size().x + 46.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 36.0), egui::Sense::hover());
                    let painter = ui.painter();
                    theme::paint_shadow(painter, rect, RADIUS_SM, alpha * 0.8);
                    theme::paint_surface(
                        painter,
                        rect,
                        anim::with_alpha(SURFACE_HI, alpha),
                        anim::with_alpha(color, 0.5 * alpha),
                        RADIUS_SM,
                    );
                    painter.circle_filled(
                        egui::pos2(rect.left() + 18.0, rect.center().y),
                        9.0,
                        anim::with_alpha(color, 0.18 * alpha),
                    );
                    painter.text(
                        egui::pos2(rect.left() + 18.0, rect.center().y),
                        egui::Align2::CENTER_CENTER,
                        icon,
                        egui::FontId::proportional(11.0),
                        anim::with_alpha(color, alpha),
                    );
                    painter.galley(
                        egui::pos2(rect.left() + 33.0, rect.center().y - galley.size().y * 0.5),
                        galley,
                        anim::with_alpha(TEXT, alpha),
                    );
                    ui.add_space(6.0);
                }
            });
    }

    fn draw_editor_screen(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .inner_margin(18.0)
                    .show(ui, |ui| {
                        if let Some(editor) = self.editor.as_mut() {
                            if editor.ui(ui) == EditorAction::Close {
                                close = true;
                            }
                        }
                    });
            });
        if close {
            self.editor = None;
            self.invalidate();
        }
    }
}

// ---------------------------------------------------------------------------
// eframe
// ---------------------------------------------------------------------------

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.core.should_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.core.take_focus_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if ctx.input(|input| input.key_pressed(egui::Key::F9)) {
            self.save_clip();
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F10)) {
            self.toggle_record();
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F1)) {
            self.show_help = !self.show_help;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.show_help {
                self.show_help = false;
            } else if self.selected.is_some() {
                self.selected = None;
                self.strip = None;
                self.confirm_delete = None;
            }
        }

        self.refresh(false);

        if self.editor.is_some() {
            self.draw_editor_screen(ctx);
            self.draw_splash(ctx);
            self.draw_toasts(ctx);
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }

        self.draw_nav(ctx);
        self.draw_top(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .inner_margin(20.0)
                    .show(ui, |ui| match self.view {
                        View::Library => self.view_library(ui),
                        View::Live => self.view_live(ui),
                        View::Games => self.view_games(ui),
                        View::Settings => self.view_settings(ui),
                    });
            });

        self.draw_drawer(ctx);
        self.draw_help(ctx);
        self.draw_splash(ctx);
        self.draw_toasts(ctx);

        let animating = self.status.recording
            || self.status.running
            || now() - self.started_at < 1.45
            || self.selected.is_some();
        ctx.request_repaint_after(Duration::from_millis(if animating { 33 } else { 400 }));
    }
}

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

pub fn load_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &rgba.as_raw()[..]);
    Some(ctx.load_texture(
        path.to_string_lossy().to_string(),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text.to_uppercase()).size(10.5).strong().color(MUTED));
    ui.add_space(8.0);
}

fn settings_row(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(label).size(12.0).color(TEXT_SOFT));
            if !hint.is_empty() {
                ui.label(egui::RichText::new(hint).size(10.0).color(MUTED_DIM));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
        });
    });
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let truncated: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", truncated)
}

fn paint_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    ratio: f32,
    color: egui::Color32,
    width: f32,
) {
    let clamped = ratio.clamp(0.0, 1.0);
    if clamped <= 0.001 {
        return;
    }
    let steps = 72;
    let start = -std::f32::consts::FRAC_PI_2;
    for index in 0..steps {
        let t0 = index as f32 / steps as f32;
        let t1 = (index + 1) as f32 / steps as f32;
        if t0 > clamped {
            break;
        }
        let a0 = start + t0 * std::f32::consts::TAU;
        let a1 = start + t1.min(clamped) * std::f32::consts::TAU;
        let p0 = center + egui::vec2(a0.cos(), a0.sin()) * radius;
        let p1 = center + egui::vec2(a1.cos(), a1.sin()) * radius;
        painter.line_segment([p0, p1], egui::Stroke::new(width, color));
    }
}

fn paint_star(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: egui::Color32) {
    // Dört uçlu parıltı: yıldız yerine temiz bir işaret.
    painter.circle_filled(center, radius * 0.32, color);
    for index in 0..4 {
        let angle = index as f32 * std::f32::consts::FRAC_PI_2 + std::f32::consts::FRAC_PI_4;
        let direction = egui::vec2(angle.cos(), angle.sin());
        painter.line_segment(
            [center + direction * radius * 0.2, center + direction * radius],
            egui::Stroke::new(1.4, color),
        );
    }
}

fn paint_icon(painter: &egui::Painter, rect: egui::Rect, view: View, color: egui::Color32) {
    match view {
        View::Library => {
            let gap = 3.0;
            let w = (rect.width() - gap) * 0.5;
            let h = (rect.height() - gap) * 0.5;
            for (dx, dy) in [(0.0, 0.0), (w + gap, 0.0), (0.0, h + gap), (w + gap, h + gap)] {
                let cell = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + dx, rect.top() + dy),
                    egui::vec2(w, h),
                );
                painter.rect_filled(cell, 2.0, color);
            }
        }
        View::Live => {
            painter.circle_stroke(
                rect.center(),
                rect.width() * 0.38,
                egui::Stroke::new(1.6, color),
            );
            painter.circle_filled(rect.center(), rect.width() * 0.15, color);
        }
        View::Games => {
            painter.rect_stroke(
                rect.shrink(rect.width() * 0.08),
                4.0,
                egui::Stroke::new(1.6, color),
            );
            painter.circle_filled(
                egui::pos2(rect.left() + rect.width() * 0.32, rect.center().y),
                1.8,
                color,
            );
            painter.circle_filled(
                egui::pos2(rect.left() + rect.width() * 0.48, rect.center().y),
                1.8,
                color,
            );
        }
        View::Settings => {
            painter.circle_stroke(
                rect.center(),
                rect.width() * 0.26,
                egui::Stroke::new(1.6, color),
            );
            for index in 0..6 {
                let angle = index as f32 / 6.0 * std::f32::consts::TAU;
                let direction = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment(
                    [
                        rect.center() + direction * (rect.width() * 0.32),
                        rect.center() + direction * (rect.width() * 0.46),
                    ],
                    egui::Stroke::new(1.6, color),
                );
            }
        }
    }
}
