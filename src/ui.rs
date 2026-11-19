//! Yerel arayüz (egui).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui;

use crate::capture::CaptureStatus;
use crate::config::Config;
use crate::anim;
use crate::core::{Core, Toast, ToastKind};
use crate::editor::{Editor, EditorAction};
use crate::hyprland;
use crate::gamedetect::Game;
use crate::library::{Clip, Stats};
use crate::util::{human_bytes, human_duration, now, time_ago};

pub const BG: egui::Color32 = egui::Color32::from_rgb(11, 11, 16);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(18, 18, 25);
pub const CARD: egui::Color32 = egui::Color32::from_rgb(24, 24, 32);
pub const CARD_HOVER: egui::Color32 = egui::Color32::from_rgb(31, 31, 42);
pub const LINE: egui::Color32 = egui::Color32::from_rgb(38, 38, 50);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(236, 236, 246);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(142, 142, 165);
pub const MUTED_DIM: egui::Color32 = egui::Color32::from_rgb(96, 96, 118);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(124, 92, 255);
pub const ACCENT_2: egui::Color32 = egui::Color32::from_rgb(77, 217, 255);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 77, 109);
pub const OK: egui::Color32 = egui::Color32::from_rgb(61, 220, 151);

#[derive(PartialEq, Clone, Copy)]
enum View {
    Library,
    Live,
    Games,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum Filter {
    All,
    Favorites,
}

pub struct App {
    core: Arc<Core>,
    view: View,
    search: String,
    sort: String,
    filter: Filter,
    game_filter: Option<String>,
    clips: Vec<Clip>,
    stats: Stats,
    status: CaptureStatus,
    game: Option<String>,
    detected: Vec<Game>,
    toasts: Vec<Toast>,
    textures: HashMap<String, egui::TextureHandle>,
    failed_thumbs: Vec<String>,
    selected: Option<String>,
    strip: Option<egui::TextureHandle>,
    draft: Config,
    draft_dirty: bool,
    rename_buffer: String,
    rename_id: Option<String>,
    trim_start: f32,
    trim_end: f32,
    trim_id: Option<String>,
    share_url: Option<String>,
    confirm_delete: Option<String>,
    last_refresh: f64,
    log_tail: Vec<String>,
    doctor: Vec<(bool, String, String)>,
    doctor_at: f64,
    editor: Option<Editor>,
    started_at: f64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, core: Arc<Core>) -> App {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(14, 14, 19);
        style.visuals.override_text_color = Some(TEXT);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.animation_time = 0.16;
        style.visuals.window_rounding = egui::Rounding::same(14.0);
        style.visuals.menu_rounding = egui::Rounding::same(10.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(9.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(9.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(9.0);
        style.visuals.selection.bg_fill = egui::Color32::from_rgba_unmultiplied(124, 92, 255, 90);
        cc.egui_ctx.set_style(style);
        core.set_ctx(cc.egui_ctx.clone());

        let mut app = App {
            view: View::Library,
            search: String::new(),
            sort: "newest".to_string(),
            filter: Filter::All,
            game_filter: None,
            clips: Vec::new(),
            stats: Stats::default(),
            status: CaptureStatus::default(),
            game: None,
            detected: Vec::new(),
            toasts: Vec::new(),
            textures: HashMap::new(),
            failed_thumbs: Vec::new(),
            selected: None,
            strip: None,
            draft: core.config(),
            draft_dirty: false,
            rename_buffer: String::new(),
            rename_id: None,
            trim_start: 0.0,
            trim_end: 0.0,
            trim_id: None,
            share_url: None,
            confirm_delete: None,
            last_refresh: 0.0,
            log_tail: Vec::new(),
            doctor: Vec::new(),
            doctor_at: 0.0,
            editor: None,
            started_at: now(),
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

        let mut clips = self.core.clips(&self.search, &self.sort);
        if self.filter == Filter::Favorites {
            clips.retain(|clip| clip.meta.favorite);
        }
        if let Some(game) = &self.game_filter {
            clips.retain(|clip| &clip.meta.game == game);
        }
        self.clips = clips;

        if self.view == View::Live {
            self.log_tail = self.core.log.tail(160);
        }
    }

    fn invalidate(&mut self) {
        self.last_refresh = 0.0;
        self.refresh(true);
    }

    fn texture_for(&mut self, ctx: &egui::Context, path: &Path, key: &str) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(key) {
            return Some(texture.clone());
        }
        if self.failed_thumbs.iter().any(|item| item == key) {
            return None;
        }
        match load_texture(ctx, path) {
            Some(texture) => {
                self.textures.insert(key.to_string(), texture.clone());
                Some(texture)
            }
            None => {
                self.failed_thumbs.push(key.to_string());
                None
            }
        }
    }

    fn selected_clip(&self) -> Option<Clip> {
        let id = self.selected.clone()?;
        self.clips.iter().find(|clip| clip.meta.id == id).cloned()
    }

    // ------------------------------------------------------------------ kabuk
    fn draw_top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(56.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("LiMedal").size(19.0).strong().color(TEXT));
                    ui.add_space(10.0);

                    if hyprland::detected() {
                        ui.label(egui::RichText::new("Hyprland").size(11.0).color(ACCENT_2));
                        ui.add_space(6.0);
                    }
                    let (dot, label) = if self.status.recording {
                        (DANGER, format!("kayıt sürüyor · {}", human_duration(self.status.recording_seconds)))
                    } else if self.status.running {
                        (OK, format!("tampon {} / {}", human_duration(self.status.fill_seconds), human_duration(self.status.buffer_seconds as f64)))
                    } else {
                        (DANGER, "kayıt durdu".to_string())
                    };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
                    let pulse_t = if self.status.recording { anim::pulse(now(), 1.1) } else { 0.0 };
                    let dot_color = anim::lerp_color(dot, egui::Color32::WHITE, pulse_t * 0.45);
                    ui.painter().circle_filled(rect.center(), 4.0 + pulse_t * 1.2, dot_color);
                    ui.label(egui::RichText::new(label).size(12.0).color(MUTED));

                    if let Some(game) = &self.game {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(format!("· {}", game)).size(12.0).color(ACCENT));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        let hotkey = self.draft.hotkeys.save_clip.clone();
                        let save_text = if hotkey.is_empty() {
                            "Klip kaydet".to_string()
                        } else {
                            format!("Klip kaydet  [{}]", hotkey)
                        };
                        let save_enabled = self.status.running;
                        let button = egui::Button::new(egui::RichText::new(save_text).strong())
                            .fill(ACCENT);
                        if ui.add_enabled(save_enabled, button).clicked() {
                            self.save_clip();
                        }
                        let record_text = if self.status.recording {
                            "Kaydı bitir".to_string()
                        } else {
                            "Tam kayıt".to_string()
                        };
                        if ui.button(record_text).clicked() {
                            self.toggle_record();
                        }
                    });
                });
            });
    }

    fn draw_side(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(196.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                let items = [
                    (View::Library, format!("Kütüphane   {}", self.stats.count)),
                    (View::Live, "Canlı".to_string()),
                    (View::Games, "Oyunlar".to_string()),
                    (View::Settings, "Ayarlar".to_string()),
                ];
                for (view, label) in items {
                    let selected = self.view == view;
                    let response = ui.selectable_label(selected, egui::RichText::new(label).size(14.0));
                    let glow = ui
                        .ctx()
                        .animate_bool(egui::Id::new(("nav-glow", view as u8)), selected);
                    if glow > 0.01 {
                        let r = response.rect;
                        let bar = egui::Rect::from_min_size(
                            egui::pos2(r.left() + 2.0, r.center().y - 9.0 * glow),
                            egui::vec2(3.0, 18.0 * glow),
                        );
                        ui.painter().rect_filled(bar, 2.0, ACCENT);
                    }
                    if response.clicked() {
                        self.view = view;
                        if view == View::Settings {
                            self.draft = self.core.config();
                            self.draft_dirty = false;
                        }
                        if view == View::Live {
                            self.log_tail = self.core.log.tail(160);
                        }
                    }
                }

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} klip · {}",
                        self.stats.count,
                        human_bytes(self.stats.size)
                    ))
                    .size(11.5)
                    .color(MUTED),
                );
                ui.label(
                    egui::RichText::new(self.stats.root.clone())
                        .size(10.5)
                        .color(egui::Color32::from_rgb(104, 104, 126)),
                );

                ui.add_space(16.0);
                let hotkeys = self.core.hotkey_state();
                if hotkeys.available {
                    ui.label(egui::RichText::new("kısayollar etkin").size(11.0).color(OK));
                } else {
                    ui.label(
                        egui::RichText::new("kısayollar kapalı")
                            .size(11.0)
                            .color(DANGER),
                    );
                }
            });
    }

    fn draw_toasts(&self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    let age = anim::progress((now() - toast.at) as f32, 0.28);
                    let alpha = anim::ease_out_cubic(age);
                    let color = match toast.kind {
                        ToastKind::Ok => OK,
                        ToastKind::Error => DANGER,
                        ToastKind::Info => ACCENT,
                    };
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, anim::with_alpha(color, alpha));
                        ui.label(egui::RichText::new(toast.text.clone()).size(12.5).color(anim::with_alpha(TEXT, alpha)));
                    });
                }
            });
    }

    // ------------------------------------------------------------------ kütüphane
    fn draw_library(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Klip veya oyun ara…")
                    .desired_width(220.0),
            );
            if response.changed() {
                self.last_refresh = 0.0;
            }
            ui.add_space(6.0);
            if ui.selectable_label(self.filter == Filter::All, "Tümü").clicked() {
                self.filter = Filter::All;
            }
            if ui.selectable_label(self.filter == Filter::Favorites, "Favoriler").clicked() {
                self.filter = Filter::Favorites;
            }
            ui.add_space(10.0);
            for (value, label) in [
                ("newest", "En yeni"),
                ("oldest", "En eski"),
                ("duration", "En uzun"),
                ("size", "En büyük"),
            ] {
                if ui.selectable_label(self.sort == value, label).clicked() {
                    self.sort = value.to_string();
                    self.last_refresh = 0.0;
                }
            }
            if self.game_filter.is_some() {
                ui.add_space(10.0);
                if ui.button("filtreyi kaldır").clicked() {
                    self.game_filter = None;
                    self.last_refresh = 0.0;
                }
            }
        });

        ui.add_space(4.0);
        let games = self.stats.games.clone();
        if !games.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let total = games.iter().map(|(_, count)| *count).sum::<usize>();
                if ui
                    .selectable_label(self.game_filter.is_none(), format!("Tüm oyunlar ({})", total))
                    .clicked()
                {
                    self.game_filter = None;
                    self.last_refresh = 0.0;
                }
                for (name, count) in games.iter().take(10) {
                    let selected = self.game_filter.as_deref() == Some(name.as_str());
                    if ui.selectable_label(selected, format!("{} ({})", name, count)).clicked() {
                        self.game_filter = Some(name.clone());
                        self.last_refresh = 0.0;
                    }
                }
            });
        }
        ui.add_space(6.0);

        if self.clips.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Henüz klip yok").size(17.0).strong());
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Oyun oynarken {} tuşuna bas; son {} saniye buraya düşer.",
                        self.draft.hotkeys.save_clip,
                        self.draft.app.clip_seconds_default
                    ))
                    .size(12.5)
                    .color(MUTED),
                );
            });
            return;
        }

        let clips = self.clips.clone();
        let available = ui.available_width() - 20.0;
        let columns = ((available / 280.0).floor() as usize).max(1);
        let spacing = 14.0;
        let card_width = ((available - spacing * (columns as f32 - 1.0)) / columns as f32).max(160.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for chunk in clips.chunks(columns) {
                    ui.horizontal(|ui| {
                        for clip in chunk {
                            self.draw_card(ui, clip, card_width);
                        }
                    });
                    ui.add_space(spacing);
                }
            });
    }

    fn draw_card(&mut self, ui: &mut egui::Ui, clip: &Clip, width: f32) {
        let thumb_height = width * 9.0 / 16.0;
        let height = thumb_height + 46.0;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(width, height),
            egui::Sense::click(),
        );
        let hovered = response.hovered();
        let hover_t = ui
            .ctx()
            .animate_bool(egui::Id::new(("card-hover", clip.meta.id.as_str())), hovered);
        let is_selected = self.selected.as_deref() == Some(clip.meta.id.as_str());

        let border = if is_selected {
            ACCENT
        } else if hovered {
            egui::Color32::from_rgb(58, 50, 92)
        } else {
            LINE
        };
        let painter = ui.painter_at(rect.expand(2.0));
        painter.rect_filled(rect.expand(1.0), 9.0_f32, border);
        painter.rect_filled(rect, 8.0_f32, anim::lerp_color(CARD, CARD_HOVER, hover_t));

        let thumb_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 1.0, rect.top() + 1.0),
            egui::vec2(width - 2.0, thumb_height),
        );
        let key = format!("thumb:{}", clip.meta.id);
        match self.texture_for(ui.ctx(), &clip.thumb(), &key) {
            Some(texture) => {
                painter.image(
                    texture.id(),
                    thumb_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                painter.rect_filled(thumb_rect, 6.0_f32, egui::Color32::from_rgb(16, 16, 22));
                painter.text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "önizleme yok",
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(96, 96, 118),
                );
            }
        }

        // süre etiketi
        let duration_text = clip.meta.duration_text();
        let duration_pos = egui::pos2(thumb_rect.right() - 8.0, thumb_rect.bottom() - 8.0);
        painter.text(
            duration_pos,
            egui::Align2::RIGHT_BOTTOM,
            duration_text,
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(240, 240, 250),
        );

        if clip.meta.favorite {
            painter.circle_filled(
                egui::pos2(thumb_rect.left() + 14.0, thumb_rect.top() + 14.0),
                4.0,
                egui::Color32::from_rgb(255, 209, 102),
            );
        }

        painter.text(
            egui::pos2(rect.left() + 11.0, thumb_rect.bottom() + 8.0),
            egui::Align2::LEFT_TOP,
            truncate_chars(&clip.meta.title, 34),
            egui::FontId::proportional(13.0),
            TEXT,
        );
        let sub = format!("{} · {}", time_ago(clip.meta.created_at), clip.meta.size_text());
        painter.text(
            egui::pos2(rect.left() + 11.0, thumb_rect.bottom() + 26.0),
            egui::Align2::LEFT_TOP,
            sub,
            egui::FontId::proportional(11.0),
            MUTED,
        );

        if response.double_clicked() {
            self.open_editor(clip.clone());
        }
        if response.clicked() {
            if self.selected.as_deref() == Some(clip.meta.id.as_str()) {
                self.selected = None;
                self.strip = None;
            } else {
                self.selected = Some(clip.meta.id.clone());
                self.strip = None;
                self.rename_buffer = clip.meta.title.clone();
                self.rename_id = None;
                self.trim_id = None;
                self.share_url = None;
            }
        }
    }

    // ------------------------------------------------------------------ detay
    fn draw_detail(&mut self, ctx: &egui::Context) {
        let clip = match self.selected_clip() {
            Some(clip) => clip,
            None => return,
        };

        egui::SidePanel::right("detail")
            .resizable(false)
            .exact_width(340.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Klip").size(13.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("kapat").clicked() {
                            self.selected = None;
                            self.strip = None;
                        }
                    });
                });
                ui.add_space(6.0);

                if self.strip.is_none() {
                    if let Some(path) = self.core.library.ensure_strip(&clip) {
                        self.strip = load_texture(ui.ctx(), &path);
                    }
                }
                let preview_height = 168.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), preview_height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 8.0, egui::Color32::from_rgb(8, 8, 12));
                let strip = self.strip.clone();
                if let Some(texture) = strip {
                    painter.image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                ui.add_space(8.0);
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

                ui.add_space(4.0);
                let meta = format!(
                    "{} · {} · {}{}",
                    if clip.meta.game.trim().is_empty() { "Etiketsiz" } else { &clip.meta.game },
                    clip.meta.duration_text(),
                    clip.meta.size_text(),
                    if clip.meta.resolution_text().is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", clip.meta.resolution_text())
                    }
                );
                ui.label(egui::RichText::new(meta).size(11.5).color(MUTED));

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Düzenle").clicked() {
                        self.open_editor(clip.clone());
                    }
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
                        self.share_url = self.core.share_url(&clip);
                        if self.share_url.is_none() {
                            self.core.toast(
                                "Paylaşım kapalı — Ayarlar'da 'Ağda paylaşım'ı aç.".to_string(),
                                ToastKind::Error,
                            );
                        }
                    }
                    if ui.button(egui::RichText::new("Sil").color(DANGER)).clicked() {
                        self.confirm_delete = Some(clip.meta.id.clone());
                    }
                });

                if let Some(url) = self.share_url.clone() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Ağ bağlantısı").size(11.0).color(MUTED));
                    ui.label(egui::RichText::new(url.clone()).size(10.5).monospace().color(ACCENT));
                    if ui.button("Bağlantıyı kopyala").clicked() {
                        ui.output_mut(|out| out.copied_text = url.clone());
                        self.core
                            .toast("Bağlantı kopyalandı".to_string(), ToastKind::Ok);
                    }
                }

                if self.trim_id.as_deref() == Some(clip.meta.id.as_str()) {
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Kırpma aralığı").size(11.5).color(MUTED));
                    let duration = clip.meta.duration.max(0.5) as f32;
                    ui.add(egui::Slider::new(&mut self.trim_start, 0.0..=duration).text("başlangıç"));
                    ui.add(egui::Slider::new(&mut self.trim_end, 0.0..=duration).text("bitiş"));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} – {}",
                            human_duration(self.trim_start as f64),
                            human_duration(self.trim_end as f64)
                        ))
                        .size(11.5)
                        .color(MUTED),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("İptal").clicked() {
                            self.trim_id = None;
                        }
                        if ui.button("Kırp ve kaydet").clicked() {
                            // ffmpeg'i arayüz iş parçacığında çalıştırma: donmasın.
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
                                Err(err) => core
                                    .toast(format!("Kırpma başarısız: {}", err), ToastKind::Error),
                            });
                        }
                    });
                }

                if self.confirm_delete.as_deref() == Some(clip.meta.id.as_str()) {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("Bu klip kalıcı olarak silinsin mi?")
                            .size(12.0)
                            .color(DANGER),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Vazgeç").clicked() {
                            self.confirm_delete = None;
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Sil").color(egui::Color32::WHITE),
                            ).fill(DANGER))
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
                }

                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        clip.meta.id,
                        clip.meta.source
                    ))
                    .size(10.5)
                    .color(egui::Color32::from_rgb(98, 98, 120)),
                );
            });
    }

    // ------------------------------------------------------------------ canlı
    fn draw_live(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(320.0);
                ui.group(|ui| {
                    ui.set_width(300.0);
                    ui.add_space(4.0);
                    let state = if self.status.running {
                        if self.status.ready { "hazır" } else { "tampon doluyor" }
                    } else {
                        "durdu"
                    };
                    ui.label(egui::RichText::new(state).size(15.0).strong().color(if self.status.running { OK } else { DANGER }));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(format!("kodlayıcı   {}", self.status.encoder_label)).size(12.0).color(MUTED));
                    ui.label(egui::RichText::new(format!("çözünürlük  {}", self.status.resolution)).size(12.0).color(MUTED));
                    ui.label(egui::RichText::new(format!("fps         {}", self.status.fps)).size(12.0).color(MUTED));
                    ui.label(egui::RichText::new(format!("segment     {} dosya", self.status.segments)).size(12.0).color(MUTED));
                    ui.add_space(6.0);
                    let ratio = if self.status.buffer_seconds > 0 {
                        (self.status.fill_seconds / self.status.buffer_seconds as f64) as f32
                    } else {
                        0.0
                    };
                    ui.add(egui::ProgressBar::new(ratio).text(format!(
                        "{} / {}",
                        human_duration(self.status.fill_seconds),
                        human_duration(self.status.buffer_seconds as f64)
                    )));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Yeniden başlat").clicked() {
                            let core = self.core.clone();
                            std::thread::spawn(move || {
                                match core.engine.restart() {
                                    Ok(()) => core.toast("Kayıt yeniden başladı".to_string(), ToastKind::Ok),
                                    Err(err) => core.toast(
                                        format!("Başlatılamadı: {}", err),
                                        ToastKind::Error,
                                    ),
                                }
                            });
                        }
                        if ui.button("Durdur").clicked() {
                            let core = self.core.clone();
                            std::thread::spawn(move || {
                                if let Err(err) = core.engine.stop() {
                                    core.toast(
                                        format!("Durdurulamadı: {}", err),
                                        ToastKind::Error,
                                    );
                                }
                            });
                        }
                    });
                    if let Some(err) = &self.status.error {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(err.clone()).size(11.0).color(DANGER));
                    }
                    if !self.status.note.is_empty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.status.note.clone()).size(11.0).color(MUTED));
                    }
                    ui.add_space(4.0);
                });
            });

            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Günlük").size(12.0).strong().color(MUTED));
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in self.log_tail.iter().rev() {
                            ui.label(
                                egui::RichText::new(line.clone())
                                    .size(10.5)
                                    .monospace()
                                    .color(egui::Color32::from_rgb(150, 150, 178)),
                            );
                        }
                    });
            });
        });
    }

    // ------------------------------------------------------------------ oyunlar
    fn draw_games(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                ui.group(|ui| {
                    ui.set_width(320.0);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Şu an").size(12.0).color(MUTED));
                    ui.label(
                        egui::RichText::new(self.game.clone().unwrap_or_else(|| "Oyun algılanmadı".to_string()))
                            .size(18.0)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    let mut detection = self.draft.app.game_detection;
                    if ui.checkbox(&mut detection, "Otomatik oyun algılama").changed() {
                        self.draft.app.game_detection = detection;
                        self.draft_dirty = true;
                        self.core.apply_config(self.draft.clone());
                        self.draft_dirty = false;
                    }
                    ui.label(
                        egui::RichText::new("/proc üzerinden çalışan süreçler taranır; Steam, Proton ve Wine oyunları yollarından tanınır.")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.add_space(4.0);
                });
            });

            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Algılanan süreçler").size(12.0).strong().color(MUTED));
                if self.detected.is_empty() {
                    ui.label(egui::RichText::new("Şu an oyun süreci görünmüyor.").size(11.5).color(MUTED));
                }
                for game in self.detected.iter().take(10) {
                    ui.label(
                        egui::RichText::new(format!("{}  ·  pid {}  ·  {}", game.name, game.pid, game.source))
                            .size(11.5)
                            .color(TEXT),
                    );
                }
                ui.add_space(14.0);
                ui.label(egui::RichText::new("Oyun bazlı klipler").size(12.0).strong().color(MUTED));
                let games = self.stats.games.clone();
                if games.is_empty() {
                    ui.label(egui::RichText::new("Henüz oyun etiketli klip yok.").size(11.5).color(MUTED));
                }
                for (name, count) in games.iter().take(14) {
                    if ui
                        .selectable_label(false, format!("{}  ·  {} klip", name, count))
                        .clicked()
                    {
                        self.game_filter = Some(name.clone());
                        self.view = View::Library;
                        self.last_refresh = 0.0;
                    }
                }
            });
        });
    }

    // ------------------------------------------------------------------ ayarlar
    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let column_width = ((ui.available_width() - 28.0) / 2.0).max(320.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(column_width);
                        self.settings_capture(ui);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(column_width);
                        self.settings_audio(ui);
                    });
                });
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(column_width);
                        self.settings_hotkeys(ui);
                    });
                    ui.vertical(|ui| {
                        ui.set_width(column_width);
                        self.settings_library(ui);
                    });
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let save_btn = egui::Button::new(egui::RichText::new("Ayarları kaydet").strong()).fill(ACCENT);
                    if ui.add(save_btn).clicked() {
                        let was_running = self.status.running;
                        self.core.apply_config(self.draft.clone());
                        self.draft = self.core.config();
                        self.draft_dirty = false;
                        if was_running {
                            let core = self.core.clone();
                            std::thread::spawn(move || {
                                let _ = core.engine.restart();
                            });
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
                        ui.label(egui::RichText::new("kaydedilmemiş değişiklikler").size(11.0).color(ACCENT));
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Kayıt ayarları değişince motor kendiliğinden yeniden başlar.")
                        .size(11.0)
                        .color(MUTED),
                );
            });
    }

    fn settings_capture(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.label(egui::RichText::new("Kayıt").size(12.5).strong().color(TEXT));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Kalite");
                for (value, label) in [
                    ("low", "Düşük"),
                    ("medium", "Orta"),
                    ("high", "Yüksek"),
                    ("ultra", "Ultra"),
                ] {
                    if ui
                        .selectable_label(self.draft.capture.quality == value, label)
                        .clicked()
                    {
                        self.draft.capture.quality = value.to_string();
                        self.draft_dirty = true;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("FPS");
                for value in [30u32, 60, 120, 144] {
                    if ui
                        .selectable_label(self.draft.capture.fps == value, value.to_string())
                        .clicked()
                    {
                        self.draft.capture.fps = value;
                        self.draft_dirty = true;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Çözünürlük");
                let options: [(&str, Option<u32>, Option<u32>); 4] = [
                    ("Otomatik", None, None),
                    ("720p", Some(1280), Some(720)),
                    ("1080p", Some(1920), Some(1080)),
                    ("1440p", Some(2560), Some(1440)),
                ];
                for (label, width, height) in options {
                    let active =
                        self.draft.capture.width == width && self.draft.capture.height == height;
                    if ui.selectable_label(active, label).clicked() {
                        self.draft.capture.width = width;
                        self.draft.capture.height = height;
                        self.draft_dirty = true;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Tampon");
                let mut seconds = self.draft.capture.buffer_seconds;
                if ui
                    .add(egui::Slider::new(&mut seconds, 5..=600).suffix(" sn"))
                    .changed()
                {
                    self.draft.capture.buffer_seconds = seconds;
                    self.draft_dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Segment");
                let mut segment = self.draft.capture.segment_seconds;
                if ui
                    .add(egui::Slider::new(&mut segment, 1..=10).suffix(" sn"))
                    .changed()
                {
                    self.draft.capture.segment_seconds = segment;
                    self.draft_dirty = true;
                }
            });

            let mut cursor = self.draft.capture.draw_cursor;
            if ui.checkbox(&mut cursor, "Fare imlecini kaydet").changed() {
                self.draft.capture.draw_cursor = cursor;
                self.draft_dirty = true;
            }

            ui.add_space(6.0);
            ui.label(egui::RichText::new("Kodlayıcı").size(11.5).color(MUTED));
            let current_encoder = self.draft.capture.encoder.clone();
            for (value, label) in [
                ("auto", "Otomatik"),
                ("h264_nvenc", "NVIDIA NVENC"),
                ("h264_vaapi", "VA-API"),
                ("h264_qsv", "Quick Sync"),
                ("libx264", "CPU"),
            ] {
                if ui
                    .selectable_label(current_encoder == value, label)
                    .clicked()
                {
                    self.draft.capture.encoder = value.to_string();
                    self.draft_dirty = true;
                }
            }
            ui.label(
                egui::RichText::new(format!("şu an: {}", self.status.encoder_label))
                    .size(10.5)
                    .color(MUTED),
            );
        });
    }

    fn settings_audio(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.label(egui::RichText::new("Ses").size(12.5).strong().color(TEXT));
            ui.add_space(4.0);

            let audio = self.core.audio.status();
            if let Some(err) = &audio.error {
                ui.label(egui::RichText::new(err.clone()).size(10.5).color(DANGER));
            }

            ui.label(egui::RichText::new("Oyun sesi").size(11.5).color(MUTED));
            let current = self.draft.capture.audio.game_source.clone();
            if ui.selectable_label(current == "auto", "Otomatik (sistem sesi)").clicked() {
                self.draft.capture.audio.game_source = "auto".to_string();
                self.draft_dirty = true;
            }
            if ui.selectable_label(current == "none", "Sessiz").clicked() {
                self.draft.capture.audio.game_source = "none".to_string();
                self.draft_dirty = true;
            }
            for source in audio.sources.iter().filter(|s| s.kind == crate::audio::SourceKind::Monitor).take(6) {
                if ui.selectable_label(current == source.name, source.label.clone()).clicked() {
                    self.draft.capture.audio.game_source = source.name.clone();
                    self.draft_dirty = true;
                }
            }

            ui.add_space(6.0);
            ui.label(egui::RichText::new("Mikrofon").size(11.5).color(MUTED));
            let mic_current = self.draft.capture.audio.mic_source.clone();
            if ui.selectable_label(mic_current.is_empty(), "Kapalı").clicked() {
                self.draft.capture.audio.mic_source = String::new();
                self.draft_dirty = true;
            }
            if ui.selectable_label(mic_current == "auto", "Otomatik (ilk mikrofon)").clicked() {
                self.draft.capture.audio.mic_source = "auto".to_string();
                self.draft_dirty = true;
            }
            for source in audio.sources.iter().filter(|s| s.kind == crate::audio::SourceKind::Input).take(6) {
                if ui.selectable_label(mic_current == source.name, source.label.clone()).clicked() {
                    self.draft.capture.audio.mic_source = source.name.clone();
                    self.draft_dirty = true;
                }
            }

            ui.add_space(6.0);
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
        ui.group(|ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.label(egui::RichText::new("Kısayollar").size(12.5).strong().color(TEXT));
            ui.add_space(4.0);

            let state = self.core.hotkey_state();
            if state.available {
                ui.label(
                    egui::RichText::new(format!("{} klavye dinleniyor", state.devices))
                        .size(11.0)
                        .color(OK),
                );
            } else {
                ui.label(egui::RichText::new("kısayollar etkin değil").size(11.0).color(DANGER));
                ui.label(
                    egui::RichText::new("sudo usermod -aG input $USER  (oturumu yenileyin)")
                        .size(10.5)
                        .monospace()
                        .color(MUTED),
                );
            }

            ui.add_space(4.0);
            let mut enabled = self.draft.hotkeys.enabled;
            if ui.checkbox(&mut enabled, "Kısayolları etkinleştir").changed() {
                self.draft.hotkeys.enabled = enabled;
                self.draft_dirty = true;
            }

            ui.horizontal(|ui| {
                ui.label("Klip kaydet");
                if ui
                    .add(egui::TextEdit::singleline(&mut self.draft.hotkeys.save_clip).desired_width(140.0))
                    .changed()
                {
                    self.draft_dirty = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Kayıt aç/kapa");
                if ui
                    .add(egui::TextEdit::singleline(&mut self.draft.hotkeys.toggle_record).desired_width(140.0))
                    .changed()
                {
                    self.draft_dirty = true;
                }
            });
            ui.label(
                egui::RichText::new("Örnek: F9, ctrl+shift+k")
                    .size(10.5)
                    .color(MUTED),
            );
        });
    }

    fn settings_library(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.label(egui::RichText::new("Kitaplık").size(12.5).strong().color(TEXT));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Klasör");
                if ui
                    .add(egui::TextEdit::singleline(&mut self.draft.app.library_dir).desired_width(200.0))
                    .changed()
                {
                    self.draft_dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Klip uzunluğu");
                let mut seconds = self.draft.app.clip_seconds_default;
                if ui
                    .add(egui::Slider::new(&mut seconds, 5..=300).suffix(" sn"))
                    .changed()
                {
                    self.draft.app.clip_seconds_default = seconds;
                    self.draft_dirty = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Depolama sınırı");
                let mut gb = self.draft.app.max_storage_gb;
                if ui
                    .add(egui::Slider::new(&mut gb, 0..=500).suffix(" GB"))
                    .changed()
                {
                    self.draft.app.max_storage_gb = gb;
                    self.draft_dirty = true;
                }
            });

            let mut autostart = self.draft.app.auto_start_capture;
            if ui.checkbox(&mut autostart, "Açılışta kaydı başlat").changed() {
                self.draft.app.auto_start_capture = autostart;
                self.draft_dirty = true;
            }
            let mut notifications = self.draft.app.notifications;
            if ui.checkbox(&mut notifications, "Masaüstü bildirimleri").changed() {
                self.draft.app.notifications = notifications;
                self.draft_dirty = true;
            }
            let mut share = self.draft.app.share_enabled;
            if ui.checkbox(&mut share, "Ağda paylaşım").changed() {
                self.draft.app.share_enabled = share;
                self.draft_dirty = true;
            }
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.set_width(ui.available_width() - 8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Sistem kontrolü").size(12.5).strong());
                if ui.button("Çalıştır").clicked() {
                    self.doctor = self.core.doctor();
                    self.doctor_at = now();
                }
            });
            if !self.doctor.is_empty() {
                ui.add_space(4.0);
                for (ok, label, detail) in self.doctor.clone() {
                    ui.label(
                        egui::RichText::new(format!(
                            "[{}] {:<16}{}",
                            if ok { "ok" } else { "!!" },
                            label,
                            detail
                        ))
                        .size(10.5)
                        .monospace()
                        .color(if ok { MUTED } else { DANGER }),
                    );
                }
            }
        });
    }

    // ------------------------------------------------------------------ eylem
    fn save_clip(&mut self) {
        let seconds = self.draft.app.clip_seconds_default;
        let core = self.core.clone();
        std::thread::spawn(move || {
            let _ = core.save_clip_now(Some(seconds), None);
        });
    }

    fn open_editor(&mut self, clip: Clip) {
        self.editor = Some(Editor::open(self.core.clone(), clip));
        self.selected = None;
        self.strip = None;
        self.trim_id = None;
    }

    fn draw_splash(&self, ctx: &egui::Context) {
        let elapsed = (now() - self.started_at) as f32;
        if elapsed >= 1.4 {
            return;
        }
        let fade = 1.0 - anim::ease_in_out(anim::progress(elapsed - 0.95, 0.42));
        let scale = anim::ease_out_back(anim::progress(elapsed, 0.75)).max(0.01);
        let rise = anim::ease_out_cubic(anim::progress(elapsed, 0.65));
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("limedal-splash"),
        ));
        painter.rect_filled(screen, 0.0, anim::with_alpha(BG, fade));

        let center = screen.center() + egui::vec2(0.0, 22.0 * (1.0 - rise));
        let size = 66.0 * scale;
        let icon = egui::Rect::from_center_size(
            center - egui::vec2(0.0, 26.0),
            egui::vec2(size, size),
        );
        painter.rect_filled(
            icon.expand(scale * 14.0),
            22.0 * scale,
            anim::with_alpha(ACCENT, 0.12 * fade),
        );
        painter.rect_filled(icon, 18.0 * scale, anim::with_alpha(ACCENT, fade));

        let c = icon.center();
        let s = 24.0 * scale;
        let triangle = vec![
            egui::pos2(c.x - s * 0.35, c.y - s * 0.5),
            egui::pos2(c.x - s * 0.35, c.y + s * 0.5),
            egui::pos2(c.x + s * 0.55, c.y),
        ];
        painter.add(egui::Shape::convex_polygon(
            triangle,
            anim::with_alpha(TEXT, fade),
            egui::Stroke::NONE,
        ));

        painter.text(
            center + egui::vec2(0.0, 40.0),
            egui::Align2::CENTER_CENTER,
            "LiMedal",
            egui::FontId::proportional(24.0),
            anim::with_alpha(TEXT, fade),
        );
        painter.text(
            center + egui::vec2(0.0, 64.0),
            egui::Align2::CENTER_CENTER,
            "oyun klipleri",
            egui::FontId::proportional(12.0),
            anim::with_alpha(MUTED, fade),
        );
        ctx.request_repaint();
    }

    fn toggle_record(&mut self) {
        let core = self.core.clone();
        std::thread::spawn(move || {
            let _ = core.toggle_record();
        });
    }
}

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

        self.refresh(false);

        self.draw_top(ctx);
        if self.editor.is_none() {
            self.draw_side(ctx);
        }
        if self.editor.is_none() {
            self.draw_detail(ctx);
        }

        let mut close_editor = false;
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(editor) = self.editor.as_mut() {
                if editor.ui(ui) == EditorAction::Close {
                    close_editor = true;
                }
                return;
            }
            match self.view {
                View::Library => self.draw_library(ui),
                View::Live => self.draw_live(ui),
                View::Games => self.draw_games(ui),
                View::Settings => self.draw_settings(ui),
            }
        });

        if close_editor {
            self.editor = None;
            self.invalidate();
        }
        self.draw_splash(ctx);
        self.draw_toasts(ctx);
        let animating = self.editor.is_some() || self.status.recording || now() - self.started_at < 1.4;
        ctx.request_repaint_after(Duration::from_millis(if animating { 16 } else { 400 }));
    }
}

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

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let truncated: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", truncated)
}
