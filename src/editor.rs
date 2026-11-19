//! Klip düzenleyici: zaman çizgisi, önizleme, işaretler ve dışa aktarma.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;

use crate::anim::ease_out_cubic;
use crate::core::{Core, ToastKind};
use crate::library::Clip;
use crate::player::Player;
use crate::ui::{load_texture, ACCENT, DANGER, LINE, MUTED, MUTED_DIM, OK, TEXT};
use crate::util::{human_bytes, id_stamp, now};

const MAX_GIF_SECONDS: f64 = 20.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum OutputAspect {
    Source,
    Vertical,
    Square,
}

#[derive(Clone, Copy, Debug)]
struct Options {
    speed: f32,
    aspect: OutputAspect,
    sharpen: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            speed: 1.0,
            aspect: OutputAspect::Source,
            sharpen: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditorAction {
    None,
    Close,
    Changed,
}

#[derive(Debug)]
enum EditorCommand {
    Toggle,
    Step(f64),
    Seek(f64),
    MarkIn,
    MarkOut,
    ClearMarks,
    ToggleLoop,
    ToggleMute,
    Cut,
    ExportGif,
    ExportMp4,
    Share,
    OpenFolder,
    AskDelete,
    CancelDelete,
    Delete,
    ToggleHelp,
    Back,
}

pub struct Editor {
    core: Arc<Core>,
    clip: Clip,
    player: Player,
    frame_texture: Option<egui::TextureHandle>,
    strip_texture: Option<egui::TextureHandle>,
    strip_tried: bool,
    title: String,
    mark_in: Option<f64>,
    mark_out: Option<f64>,
    looping: bool,
    confirm_delete: bool,
    show_help: bool,
    share_url: Option<String>,
    busy: Arc<AtomicBool>,
    opened_at: f64,
    options: Options,
}

impl Editor {
    pub fn open(core: Arc<Core>, clip: Clip) -> Editor {
        let width = clip.meta.width.unwrap_or(1280);
        let height = clip.meta.height.unwrap_or(720);
        let duration = if clip.meta.duration > 0.1 {
            clip.meta.duration
        } else {
            1.0
        };
        let player = Player::new(core.ff.path(), clip.video(), duration, 30.0, width, height);
        Editor {
            core,
            clip: clip.clone(),
            player,
            frame_texture: None,
            strip_texture: None,
            strip_tried: false,
            title: clip.meta.title.clone(),
            mark_in: None,
            mark_out: None,
            looping: true,
            confirm_delete: false,
            show_help: false,
            share_url: None,
            busy: Arc::new(AtomicBool::new(false)),
            opened_at: now(),
            options: Options::default(),
        }
    }

    pub fn clip_id(&self) -> String {
        self.clip.meta.id.clone()
    }

    // ------------------------------------------------------------------ yardımcı
    fn duration(&self) -> f64 {
        self.player.duration.max(0.05)
    }

    fn aspect(&self) -> f32 {
        if let Some(texture) = &self.frame_texture {
            let size = texture.size();
            if size[0] > 0 && size[1] > 0 {
                return size[0] as f32 / size[1] as f32;
            }
        }
        let width = self.clip.meta.width.unwrap_or(1280) as f32;
        let height = self.clip.meta.height.unwrap_or(720) as f32;
        (width / height).clamp(0.2, 5.0)
    }

    /// Geçerli seçim aralığı: (başlangıç, bitiş)
    fn range(&self) -> Option<(f64, f64)> {
        let start = self.mark_in.unwrap_or(0.0).max(0.0);
        let end = self.mark_out.unwrap_or(self.duration());
        if end - start >= 0.3 {
            Some((start, end))
        } else {
            None
        }
    }

    fn selection_text(&self) -> String {
        match self.range() {
            Some((start, end)) => format!(
                "{}  -  {}   ({} sn)",
                time_text(start),
                time_text(end),
                format!("{:.1}", end - start)
            ),
            None => "tüm klip".to_string(),
        }
    }

    // ------------------------------------------------------------------ veri
    fn pump(&mut self, ctx: &egui::Context) {
        self.player.tick();
        if let Some((image, _at)) = self.player.poll() {
            if let Some(texture) = self.frame_texture.as_mut() {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.frame_texture = Some(ctx.load_texture(
                    "editor-frame",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            ctx.request_repaint();
        }

        if self.looping && self.player.playing {
            if let (Some(_), Some(end)) = (self.mark_in, self.mark_out) {
                if self.player.clock >= end - 0.06 {
                    let start = self.mark_in.unwrap_or(0.0);
                    self.player.play();
                    self.player.seek(start);
                    ctx.request_repaint();
                }
            }
        }

        if !self.strip_tried {
            self.strip_tried = true;
            if let Some(path) = self.core.library.ensure_strip(&self.clip) {
                self.strip_texture = load_texture(ctx, &path);
            }
        }

    }

    // ------------------------------------------------------------------ eylemler
    fn mark_in_now(&mut self) {
        let at = self.player.clock;
        self.mark_in = Some(at);
        if let Some(end) = self.mark_out {
            if end <= at + 0.2 {
                self.mark_out = None;
            }
        }
    }

    fn mark_out_now(&mut self) {
        let at = self.player.clock;
        self.mark_out = Some(at.min(self.duration()));
        if let Some(start) = self.mark_in {
            if at <= start + 0.2 {
                self.mark_in = None;
            }
        }
    }

    fn rename(&mut self) {
        let title = self.title.trim().to_string();
        if title.is_empty() || title == self.clip.meta.title {
            return;
        }
        if let Some(clip) = self.core.library.set_title(&self.clip.meta.id, &title) {
            self.clip.meta.title = clip.meta.title;
            self.core
                .toast("Başlık güncellendi".to_string(), ToastKind::Ok);
        }
    }

    fn toggle_favorite(&mut self) {
        if let Some(clip) = self.core.library.toggle_favorite(&self.clip.meta.id) {
            self.clip.meta.favorite = clip.meta.favorite;
            let message = if clip.meta.favorite {
                "Favorilere eklendi"
            } else {
                "Favoriden çıkarıldı"
            };
            self.core.toast(message.to_string(), ToastKind::Ok);
        }
    }

    fn share(&mut self) {
        match self.core.share_url(&self.clip) {
            Some(url) => {
                self.share_url = Some(url);
            }
            None => {
                self.share_url = None;
                self.core.toast(
                    "Paylaşım kapalı — Ayarlar'da 'Ağda paylaşım'ı aç.".to_string(),
                    ToastKind::Error,
                );
            }
        }
    }

    fn cut(&mut self) {
        let (start, end) = match self.range() {
            Some(range) => range,
            None => {
                self.core
                    .toast("Önce giriş/çıkış işaretlerini koy.".to_string(), ToastKind::Error);
                return;
            }
        };
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let core = self.core.clone();
        let busy = self.busy.clone();
        let id = self.clip.meta.id.clone();
        self.player.pause();
        std::thread::spawn(move || {
            let result = core.library.trim(&id, start, end);
            busy.store(false, Ordering::SeqCst);
            match result {
                Ok(clip) => core.toast(
                    format!("Yeni klip: {}", clip.meta.title),
                    ToastKind::Ok,
                ),
                Err(err) => core.toast(format!("Kırpma başarısız: {}", err), ToastKind::Error),
            }
            core.repaint();
        });
    }

    fn export(&mut self, gif: bool) {
        let (start, end) = match self.range() {
            Some(range) => range,
            None => {
                self.core
                    .toast("Önce giriş/çıkış işaretlerini koy.".to_string(), ToastKind::Error);
                return;
            }
        };
        if gif && end - start > MAX_GIF_SECONDS {
            self.core.toast(
                format!("GIF için en fazla {:.0} saniye seç.", MAX_GIF_SECONDS),
                ToastKind::Error,
            );
            return;
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.player.pause();

        let ffmpeg = self.core.ff.path();
        let src = self.clip.video().to_string_lossy().to_string();
        let dir = self.core.library.root().join("exports");
        let _ = std::fs::create_dir_all(&dir);
        let stem = if self.clip.meta.slug.is_empty() {
            self.clip.meta.id.clone()
        } else {
            self.clip.meta.slug.clone()
        };
        let extension = if gif { "gif" } else { "mp4" };
        let target: PathBuf = dir.join(format!("{}-{}.{}", stem, id_stamp(), extension));
        let args = export_args(
            &ffmpeg,
            &src,
            &target.to_string_lossy(),
            start,
            end,
            gif,
            self.clip.meta.width.unwrap_or(1280),
            self.clip.meta.height.unwrap_or(720),
            self.options,
        );

        let core = self.core.clone();
        let busy = self.busy.clone();
        let label = target.to_string_lossy().to_string();
        std::thread::spawn(move || {
            let (code, _, err) = crate::ffmpeg::run(&args);
            busy.store(false, Ordering::SeqCst);
            if code == 0 && std::fs::metadata(&label).map(|m| m.len()).unwrap_or(0) > 1024 {
                core.toast(format!("Dışa aktarıldı: {}", label), ToastKind::Ok);
                let _ = std::fs::metadata(&label);
            } else {
                let reason = err.trim().lines().last().unwrap_or("bilinmeyen hata");
                core.toast(
                    format!("Dışa aktarma başarısız: {}", reason),
                    ToastKind::Error,
                );
            }
            core.repaint();
        });
    }

    fn act(&mut self, command: EditorCommand, ctx: &egui::Context) -> EditorAction {
        match command {
            EditorCommand::Toggle => self.player.toggle(),
            EditorCommand::Step(frames) => {
                self.player.pause();
                self.player.step(frames);
            }
            EditorCommand::Seek(at) => self.player.seek(at),
            EditorCommand::MarkIn => self.mark_in_now(),
            EditorCommand::MarkOut => self.mark_out_now(),
            EditorCommand::ClearMarks => {
                self.mark_in = None;
                self.mark_out = None;
            }
            EditorCommand::ToggleLoop => self.looping = !self.looping,
            EditorCommand::ToggleMute => {
                let next = !self.player.audio_enabled;
                self.player.set_audio(next);
            }
            EditorCommand::Cut => self.cut(),
            EditorCommand::ExportGif => self.export(true),
            EditorCommand::ExportMp4 => self.export(false),
            EditorCommand::Share => self.share(),
            EditorCommand::OpenFolder => self.core.open_path(&self.clip.dir),
            EditorCommand::AskDelete => self.confirm_delete = true,
            EditorCommand::CancelDelete => self.confirm_delete = false,
            EditorCommand::Delete => {
                self.confirm_delete = false;
                self.player.pause();
                if self.core.library.delete(&self.clip.meta.id) {
                    self.core.toast("Klip silindi".to_string(), ToastKind::Ok);
                    return EditorAction::Close;
                }
                self.core
                    .toast("Klip silinemedi".to_string(), ToastKind::Error);
            }
            EditorCommand::ToggleHelp => self.show_help = !self.show_help,
            EditorCommand::Back => {
                if self.confirm_delete {
                    self.confirm_delete = false;
                } else if self.show_help {
                    self.show_help = false;
                } else {
                    return EditorAction::Close;
                }
            }
        }
        self.core.repaint();
        let _ = ctx;
        EditorAction::None
    }

    // ------------------------------------------------------------------ çizim
    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorAction {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);

        let mut commands: Vec<EditorCommand> = Vec::new();
        let mut action = EditorAction::None;
        let appear = ease_out_cubic(crate::anim::progress(
            (now() - self.opened_at) as f32,
            0.35,
        ));

        // ---- üst satır
        ui.horizontal(|ui| {
            if ui.button("< Kütüphane").clicked() {
                action = EditorAction::Close;
            }
            ui.add_space(8.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.title)
                    .desired_width(320.0)
                    .hint_text("Klip adı"),
            );
            if response.lost_focus() {
                self.rename();
            }
            ui.add_space(6.0);
            let star = if self.clip.meta.favorite { "Favoride" } else { "Favori" };
            if ui.button(star).clicked() {
                self.toggle_favorite();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new("Sil").color(DANGER)).clicked() {
                    commands.push(EditorCommand::AskDelete);
                }
                if ui.button("Klasör").clicked() {
                    commands.push(EditorCommand::OpenFolder);
                }
                if self.busy.load(Ordering::SeqCst) {
                    ui.spinner();
                    ui.label(egui::RichText::new("işleniyor…").size(11.0).color(MUTED));
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {} · {}",
                        self.clip.meta.game,
                        human_bytes(self.clip.meta.size),
                        self.clip
                            .meta
                            .resolution_text()
                    ))
                    .size(11.5)
                    .color(MUTED),
                );
            });
        });

        ui.add_space(8.0);

        // ---- önizleme
        let total_width = ui.available_width();
        let max_height = (ui.available_height() * 0.5).max(180.0);
        let aspect = self.aspect();
        let mut video_width = total_width.min(1180.0);
        let mut video_height = video_width / aspect;
        if video_height > max_height {
            video_height = max_height;
            video_width = video_height * aspect;
        }
        let video_width = video_width.max(160.0);
        let video_height = video_height.max(90.0);
        let side_pad = ((total_width - video_width) * 0.5).max(0.0);

        ui.horizontal(|ui| {
            ui.add_space(side_pad);
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(video_width, video_height),
                egui::Sense::click(),
            );
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 10.0, egui::Color32::from_rgb(6, 6, 10));

            if let Some(texture) = &self.frame_texture {
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, (255.0 * appear) as u8),
                );
            } else {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "önizleme hazırlanıyor…",
                    egui::FontId::proportional(12.5),
                    MUTED_DIM,
                );
            }

            if !self.player.playing {
                let center = rect.center();
                painter.circle_filled(
                    center,
                    26.0,
                    egui::Color32::from_rgba_unmultiplied(8, 8, 14, 190),
                );
                painter.circle_stroke(center, 26.0, egui::Stroke::new(1.0, ACCENT));
                for step in 0..9 {
                    let t = step as f32 / 8.0;
                    let x = center.x - 4.0 + t * 12.0;
                    let half = 9.0 * (1.0 - t.abs());
                    painter.line_segment(
                        [egui::pos2(x, center.y - half), egui::pos2(x, center.y + half)],
                        egui::Stroke::new(2.0, TEXT),
                    );
                }
            }

            if response.clicked() {
                commands.push(EditorCommand::Toggle);
            }
            if let Some(err) = &self.player.error {
                painter.text(
                    egui::pos2(rect.left() + 10.0, rect.bottom() - 10.0),
                    egui::Align2::LEFT_BOTTOM,
                    err.clone(),
                    egui::FontId::proportional(11.0),
                    DANGER,
                );
            }
        });

        ui.add_space(10.0);

        // ---- zaman çizgisi
        commands.extend(self.timeline(ui));

        ui.add_space(8.0);

        // ---- taşıma düğmeleri
        ui.horizontal(|ui| {
            if ui.button("Başa").on_hover_text("Başa dön").clicked() {
                commands.push(EditorCommand::Seek(0.0));
            }
            if ui.button("-1 kare").on_hover_text("Bir kare geri").clicked() {
                commands.push(EditorCommand::Step(-1.0));
            }
            let play_label = if self.player.playing { "Duraklat" } else { "Oynat" };
            if ui
                .add(egui::Button::new(egui::RichText::new(play_label).strong()).fill(ACCENT))
                .clicked()
            {
                commands.push(EditorCommand::Toggle);
            }
            if ui.button("+1 kare").on_hover_text("Bir kare ileri").clicked() {
                commands.push(EditorCommand::Step(1.0));
            }
            if ui.button("Sona").on_hover_text("Sona git").clicked() {
                let end = self.duration();
                commands.push(EditorCommand::Seek(end));
            }
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} / {}",
                    time_text(self.player.clock),
                    time_text(self.duration())
                ))
                .size(13.0)
                .monospace()
                .color(TEXT),
            );
            ui.add_space(12.0);
            if ui.button("Giriş [I]").clicked() {
                commands.push(EditorCommand::MarkIn);
            }
            if ui.button("Çıkış [O]").clicked() {
                commands.push(EditorCommand::MarkOut);
            }
            if ui.button("Temizle").clicked() {
                commands.push(EditorCommand::ClearMarks);
            }
            let loop_label = if self.looping { "Döngü açık" } else { "Döngü kapalı" };
            if ui.button(loop_label).clicked() {
                commands.push(EditorCommand::ToggleLoop);
            }
            let audio_label = if self.player.audio_enabled { "Ses açık" } else { "Ses kapalı" };
            if ui
                .add_enabled(self.player.audio_available, egui::Button::new(audio_label))
                .clicked()
            {
                commands.push(EditorCommand::ToggleMute);
            }
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(self.selection_text())
                    .size(11.5)
                    .color(if self.range().is_some() { OK } else { MUTED }),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Yardım [H]").clicked() {
                    commands.push(EditorCommand::ToggleHelp);
                }
            });
        });

        ui.add_space(10.0);

        // ---- eylemler
        let has_range = self.range().is_some();
        let busy = self.busy.load(Ordering::SeqCst);

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Hız").size(11.0).color(MUTED));
            for (value, label) in [(0.5f32, "0.5x"), (1.0, "1x"), (1.5, "1.5x"), (2.0, "2x")] {
                if ui
                    .selectable_label((self.options.speed - value).abs() < 0.01, label)
                    .clicked()
                {
                    self.options.speed = value;
                }
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Format").size(11.0).color(MUTED));
            for (value, label) in [
                (OutputAspect::Source, "Orijinal"),
                (OutputAspect::Vertical, "9:16"),
                (OutputAspect::Square, "1:1"),
            ] {
                if ui
                    .selectable_label(self.options.aspect == value, label)
                    .clicked()
                {
                    self.options.aspect = value;
                }
            }
            ui.add_space(10.0);
            if ui
                .selectable_label(self.options.sharpen, "Keskinleştir")
                .clicked()
            {
                self.options.sharpen = !self.options.sharpen;
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    has_range && !busy,
                    egui::Button::new(egui::RichText::new("Kırp ve kütüphaneye ekle").strong())
                        .fill(ACCENT),
                )
                .clicked()
            {
                commands.push(EditorCommand::Cut);
            }
            if ui
                .add_enabled(has_range && !busy, egui::Button::new("GIF olarak aktar"))
                .clicked()
            {
                commands.push(EditorCommand::ExportGif);
            }
            if ui
                .add_enabled(has_range && !busy, egui::Button::new("MP4 (720p) aktar"))
                .clicked()
            {
                commands.push(EditorCommand::ExportMp4);
            }
            if ui.button("Paylaş").clicked() {
                commands.push(EditorCommand::Share);
            }
            if ui.button("Klasörü aç").clicked() {
                commands.push(EditorCommand::OpenFolder);
            }
        });

        if let Some(url) = self.share_url.clone() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(url.clone())
                        .size(11.0)
                        .monospace()
                        .color(ACCENT),
                );
                if ui.button("Kopyala").clicked() {
                    ui.ctx().copy_text(url.clone());
                    self.core
                        .toast("Bağlantı kopyalandı".to_string(), ToastKind::Ok);
                }
            });
        }

        // ---- klavye
        self.handle_keys(&ctx, &mut commands);

        // ---- pencereler
        if self.confirm_delete {
            egui::Window::new("Klibi sil")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!("\"{}\" kalıcı olarak silinsin mi?", self.clip.meta.title))
                            .size(12.5),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Vazgeç").clicked() {
                            commands.push(EditorCommand::CancelDelete);
                        }
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Sil").color(egui::Color32::WHITE),
                            ).fill(DANGER))
                            .clicked()
                        {
                            commands.push(EditorCommand::Delete);
                        }
                    });
                });
        }

        if self.show_help {
            egui::Window::new("Düzenleyici kısayolları")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    for (key, text) in [
                        ("Boşluk", "oynat / duraklat"),
                        ("← →", "bir kare geri / ileri"),
                        ("Shift + ← →", "bir saniye geri / ileri"),
                        ("I / O", "giriş ve çıkış işareti"),
                        ("X", "işaretleri temizle"),
                        ("L", "döngü"),
                        ("M", "ses"),
                        ("Ctrl + S", "seçimi kırp ve kaydet"),
                        ("Esc", "kütüphaneye dön"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:<14}", key))
                                    .monospace()
                                    .size(11.5)
                                    .color(ACCENT),
                            );
                            ui.label(egui::RichText::new(text).size(11.5).color(MUTED));
                        });
                    }
                });
        }

        for command in commands {
            let outcome = self.act(command, &ctx);
            if outcome != EditorAction::None {
                action = outcome;
            }
        }

        if self.player.playing {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }

        action
    }

    // ------------------------------------------------------------------ zaman çizgisi
    fn timeline(&mut self, ui: &mut egui::Ui) -> Vec<EditorCommand> {
        let mut commands = Vec::new();
        let width = ui.available_width().max(120.0);
        let height = 84.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let duration = self.duration();

        painter.rect_filled(rect, 8.0, egui::Color32::from_rgb(9, 9, 14));

        let strip_rect = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), height - 20.0),
        );
        if let Some(texture) = &self.strip_texture {
            painter.image(
                texture.id(),
                strip_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::from_rgb(150, 150, 168),
            );
        } else {
            painter.rect_filled(strip_rect, 6.0, egui::Color32::from_rgb(14, 14, 20));
            painter.text(
                strip_rect.center(),
                egui::Align2::CENTER_CENTER,
                "film şeridi hazırlanıyor…",
                egui::FontId::proportional(11.0),
                MUTED_DIM,
            );
        }

        let range = self.range();
        if let Some((start, end)) = range {
            let start_x = rect.left() + (start / duration) as f32 * rect.width();
            let end_x = rect.left() + (end / duration) as f32 * rect.width();
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), strip_rect.top()),
                    egui::pos2(start_x, strip_rect.bottom()),
                ),
                0.0,
                egui::Color32::from_rgba_unmultiplied(6, 6, 10, 165),
            );
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(end_x, strip_rect.top()),
                    egui::pos2(rect.right(), strip_rect.bottom()),
                ),
                0.0,
                egui::Color32::from_rgba_unmultiplied(6, 6, 10, 165),
            );
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(start_x, strip_rect.top()),
                    egui::pos2(end_x, strip_rect.bottom()),
                ),
                3.0,
                egui::Stroke::new(1.5, ACCENT),
            );
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(start_x - 1.5, strip_rect.top()), egui::vec2(3.0, strip_rect.height())),
                1.5,
                ACCENT,
            );
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(end_x - 1.5, strip_rect.top()), egui::vec2(3.0, strip_rect.height())),
                1.5,
                ACCENT,
            );
        }

        for step in 0..=10 {
            let ratio = step as f32 / 10.0;
            let x = rect.left() + ratio * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.bottom() - 18.0), egui::pos2(x, rect.bottom() - 12.0)],
                egui::Stroke::new(1.0, LINE),
            );
            if step % 2 == 0 {
                painter.text(
                    egui::pos2(x, rect.bottom() - 10.0),
                    egui::Align2::CENTER_TOP,
                    time_text(duration * ratio as f64),
                    egui::FontId::proportional(9.5),
                    MUTED_DIM,
                );
            }
        }

        let play_ratio = if duration > 0.0 {
            (self.player.clock / duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let play_x = rect.left() + play_ratio * rect.width();
        painter.line_segment(
            [egui::pos2(play_x, rect.top() - 3.0), egui::pos2(play_x, rect.bottom())],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 96, 128)),
        );
        painter.circle_filled(egui::pos2(play_x, rect.top() - 3.0), 4.5, egui::Color32::from_rgb(255, 96, 128));

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let pointer = response.interact_pointer_pos();
        if (response.clicked() || response.dragged()) && pointer.is_some() {
            if let Some(position) = pointer {
                let ratio = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let target = duration * ratio as f64;
                commands.push(EditorCommand::Seek(target));
            }
        }

        commands
    }

    // ------------------------------------------------------------------ klavye
    fn handle_keys(&mut self, ctx: &egui::Context, commands: &mut Vec<EditorCommand>) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let shift = ctx.input(|input| input.modifiers.shift);
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Space) {
                commands.push(EditorCommand::Toggle);
            }
            if input.key_pressed(egui::Key::ArrowLeft) {
                commands.push(EditorCommand::Step(if shift { -30.0 } else { -1.0 }));
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                commands.push(EditorCommand::Step(if shift { 30.0 } else { 1.0 }));
            }
            if input.key_pressed(egui::Key::Home) {
                commands.push(EditorCommand::Seek(0.0));
            }
            if input.key_pressed(egui::Key::End) {
                let end = self.duration();
                commands.push(EditorCommand::Seek(end));
            }
            if input.key_pressed(egui::Key::I) {
                commands.push(EditorCommand::MarkIn);
            }
            if input.key_pressed(egui::Key::O) {
                commands.push(EditorCommand::MarkOut);
            }
            if input.key_pressed(egui::Key::X) {
                commands.push(EditorCommand::ClearMarks);
            }
            if input.key_pressed(egui::Key::L) {
                commands.push(EditorCommand::ToggleLoop);
            }
            if input.key_pressed(egui::Key::M) {
                commands.push(EditorCommand::ToggleMute);
            }
            if input.key_pressed(egui::Key::H) {
                commands.push(EditorCommand::ToggleHelp);
            }
            if input.key_pressed(egui::Key::Escape) {
                commands.push(EditorCommand::Back);
            }
            if input.modifiers.command && input.key_pressed(egui::Key::S) {
                commands.push(EditorCommand::Cut);
            }
        });
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.player.pause();
    }
}

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

fn time_text(seconds: f64) -> String {
    let value = if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    };
    let minutes = (value / 60.0).floor() as u64;
    let rest = value - minutes as f64 * 60.0;
    format!("{}:{:04.1}", minutes, rest)
}

fn export_args(
    ffmpeg: &str,
    source: &str,
    target: &str,
    start: f64,
    end: f64,
    gif: bool,
    width: u32,
    height: u32,
    options: Options,
) -> Vec<String> {
    let speed = if options.speed > 0.05 {
        options.speed
    } else {
        1.0
    };
    let span = ((end - start).max(0.1) / speed as f64).max(0.1);

    let mut filters: Vec<String> = Vec::new();
    if (speed - 1.0).abs() > 0.01 {
        filters.push(format!("setpts=PTS/{:.4}", speed));
    }
    match options.aspect {
        OutputAspect::Vertical => {
            filters.push("crop=ih*9/16:ih".to_string());
            filters.push("scale=-2:1280".to_string());
        }
        OutputAspect::Square => {
            filters.push("crop=ih:ih".to_string());
            filters.push("scale=-2:1080".to_string());
        }
        OutputAspect::Source => {}
    }
    if options.sharpen {
        filters.push("unsharp=5:5:0.8".to_string());
    }

    let mut args: Vec<String> = vec![
        ffmpeg.to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-nostdin".to_string(),
        "-y".to_string(),
        "-ss".to_string(),
        format!("{:.3}", start.max(0.0)),
        "-i".to_string(),
        source.to_string(),
        "-t".to_string(),
        format!("{:.3}", span),
    ];

    if gif {
        let target_width = width.min(720).max(240);
        let fps = if height >= 900 { 20 } else { 24 };
        filters.push(format!("fps={}", fps));
        filters.push(format!("scale={}:-1:flags=lanczos", target_width));
        let chain = filters.join(",");
        args.extend([
            "-an".to_string(),
            "-filter_complex".to_string(),
            format!(
                "{},split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
                chain
            ),
            "-loop".to_string(),
            "0".to_string(),
            target.to_string(),
        ]);
        return args;
    }

    filters.push("scale=-2:min(720\\,ih)".to_string());
    args.extend([
        "-vf".to_string(),
        filters.join(","),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "veryfast".to_string(),
        "-crf".to_string(),
        "22".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "160k".to_string(),
    ]);
    if (speed - 1.0).abs() > 0.01 {
        args.push("-af".to_string());
        args.push(format!("atempo={:.3}", speed));
    }
    args.push(target.to_string());
    args
}


