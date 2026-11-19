//! Tasarım sistemi: renkler, tipografi ve ortak çizim yardımcıları.
//!
//! Arayüzün tamamı buradaki token'lardan beslenir; tema değiştirmek için
//! yalnızca bu dosyayı düzenlemek yeterlidir.

use eframe::egui;

use crate::anim::{lerp, lerp_color, with_alpha};

// ---------------------------------------------------------------------------
// Renkler
// ---------------------------------------------------------------------------

pub const BG: egui::Color32 = egui::Color32::from_rgb(10, 10, 15);
pub const BG_DEEP: egui::Color32 = egui::Color32::from_rgb(6, 6, 10);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(19, 19, 29);
pub const SURFACE_HI: egui::Color32 = egui::Color32::from_rgb(27, 27, 41);
pub const SURFACE_LO: egui::Color32 = egui::Color32::from_rgb(14, 14, 21);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(38, 38, 56);
pub const BORDER_HI: egui::Color32 = egui::Color32::from_rgb(62, 62, 90);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(238, 238, 248);
pub const TEXT_SOFT: egui::Color32 = egui::Color32::from_rgb(188, 188, 212);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(140, 140, 170);
pub const MUTED_DIM: egui::Color32 = egui::Color32::from_rgb(96, 96, 122);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(124, 92, 255);
pub const ACCENT_HI: egui::Color32 = egui::Color32::from_rgb(154, 126, 255);
pub const ACCENT_2: egui::Color32 = egui::Color32::from_rgb(56, 214, 255);
pub const PINK: egui::Color32 = egui::Color32::from_rgb(255, 92, 138);
pub const GOLD: egui::Color32 = egui::Color32::from_rgb(255, 209, 102);
pub const OK: egui::Color32 = egui::Color32::from_rgb(61, 220, 151);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 86, 112);

/// editor.rs ile uyumluluk için ince çizgi rengi.
pub const LINE: egui::Color32 = BORDER;

// ---------------------------------------------------------------------------
// Ölçüler
// ---------------------------------------------------------------------------

pub const RADIUS: f32 = 14.0;
pub const RADIUS_SM: f32 = 10.0;
pub const NAV_WIDTH: f32 = 238.0;
pub const TOP_HEIGHT: f32 = 66.0;

// ---------------------------------------------------------------------------
// Stil
// ---------------------------------------------------------------------------

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = SURFACE_LO;
    visuals.override_text_color = Some(TEXT);
    visuals.window_rounding = egui::Rounding::same(RADIUS);
    visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.menu_rounding = egui::Rounding::same(RADIUS_SM);
    visuals.selection.bg_fill = with_alpha(ACCENT, 0.55);
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_HI);

    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_SOFT);
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(RADIUS_SM);

    visuals.widgets.inactive.bg_fill = SURFACE_HI;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_HI;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_SOFT);
    visuals.widgets.inactive.rounding = egui::Rounding::same(RADIUS_SM);

    visuals.widgets.hovered.bg_fill = BORDER;
    visuals.widgets.hovered.weak_bg_fill = BORDER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, BORDER_HI);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);
    visuals.widgets.hovered.rounding = egui::Rounding::same(RADIUS_SM);

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_HI);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.rounding = egui::Rounding::same(RADIUS_SM);

    visuals.widgets.open.bg_fill = SURFACE_HI;
    visuals.widgets.open.weak_bg_fill = SURFACE_HI;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, BORDER_HI);
    visuals.widgets.open.rounding = egui::Rounding::same(RADIUS_SM);

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(9.0, 9.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size = egui::vec2(38.0, 30.0);
    style.spacing.slider_width = 150.0;
    style.animation_time = 0.15;

    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(11.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(11.5));

    ctx.set_style(style);
}

// ---------------------------------------------------------------------------
// Çizim yardımcıları
// ---------------------------------------------------------------------------

/// Yuvarlatılmış, çerçeveli yüzey.
pub fn paint_surface(
    painter: &egui::Painter,
    rect: egui::Rect,
    fill: egui::Color32,
    border: egui::Color32,
    radius: f32,
) {
    painter.rect_filled(rect, radius, fill);
    if border.a() > 0 {
        painter.rect_stroke(rect, radius, egui::Stroke::new(1.0, border));
    }
}

/// Yumuşak gölge taklidi: birkaç saydam katman.
pub fn paint_shadow(painter: &egui::Painter, rect: egui::Rect, radius: f32, strength: f32) {
    if strength <= 0.01 {
        return;
    }
    for step in 0..4 {
        let grow = 2.0 + step as f32 * 3.0;
        let alpha = strength * (0.10 - step as f32 * 0.02).max(0.01);
        painter.rect_filled(rect.expand(grow), radius + grow, with_alpha(BG_DEEP, alpha));
    }
}

/// Soldan sağa renk geçişli ince şerit (ilerleme çubuğu, vurgu çizgisi).
pub fn paint_gradient(
    painter: &egui::Painter,
    rect: egui::Rect,
    from: egui::Color32,
    to: egui::Color32,
    radius: f32,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let steps = 40;
    for index in 0..steps {
        let t0 = index as f32 / steps as f32;
        let t1 = (index + 1) as f32 / steps as f32;
        let piece = egui::Rect::from_min_max(
            egui::pos2(rect.left() + rect.width() * t0, rect.top()),
            egui::pos2(rect.left() + rect.width() * t1 + 0.6, rect.bottom()),
        );
        let color = lerp_color(from, to, (t0 + t1) * 0.5);
        painter.rect_filled(piece, if index == 0 || index == steps - 1 { radius } else { 0.0 }, color);
    }
}

/// Kart yüzeyi: yüzey + çerçeve + iç boşluk.
pub fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(RADIUS))
        .inner_margin(16.0)
}

/// Daha koyu, iç içe panel.
pub fn inner_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(SURFACE_LO)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .rounding(egui::Rounding::same(RADIUS_SM))
        .inner_margin(12.0)
}

/// Dolgu (kart içermeyen) blok.
pub fn plain_frame() -> egui::Frame {
    egui::Frame::none().inner_margin(0.0)
}

// ---------------------------------------------------------------------------
// Küçük bileşenler
// ---------------------------------------------------------------------------

/// Segment düğmesi: seçiliyse vurgulu.
pub fn segment(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let painter_style = (SURFACE_HI, BORDER, MUTED);
    let fill = if active { with_alpha(ACCENT, 0.22) } else { painter_style.0 };
    let border = if active { ACCENT } else { painter_style.1 };
    let color = if active { egui::Color32::WHITE } else { painter_style.2 };

    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::FontId::proportional(12.0),
        color,
    );
    let size = egui::vec2(galley.size().x + 22.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hover = ui
        .ctx()
        .animate_bool(egui::Id::new(("seg-hover", text, rect.left() as i32)), response.hovered());
    let fill = if active {
        fill
    } else {
        lerp_color(fill, SURFACE_HI, hover)
    };
    let painter = ui.painter();
    paint_surface(painter, rect, fill, border, 999.0);
    let text_color = if active { egui::Color32::WHITE } else { color };
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Ana eylem düğmesi (dolgulu).
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), egui::FontId::proportional(13.5), egui::Color32::WHITE);
    let size = egui::vec2(galley.size().x + 34.0, 36.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hover = ui.ctx().animate_bool(
        egui::Id::new(("primary", text, rect.left() as i32, rect.top() as i32)),
        response.hovered(),
    );
    let painter = ui.painter();
    paint_shadow(painter, rect, 999.0, 0.55 + hover * 0.35);
    let fill = lerp_color(ACCENT, ACCENT_HI, hover);
    paint_surface(painter, rect, fill, egui::Color32::TRANSPARENT, 999.0);
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::WHITE,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Açık anahtar (switch).
pub fn switch(ui: &mut egui::Ui, id: &str, value: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(42.0, 24.0), egui::Sense::click());
    let t = ui.ctx().animate_bool(egui::Id::new(("switch", id)), *value);
    let painter = ui.painter();
    let fill = lerp_color(SURFACE_HI, ACCENT, t);
    paint_surface(painter, rect, fill, if *value { ACCENT_HI } else { BORDER }, 999.0);
    let knob_x = lerp(rect.left() + 12.0, rect.right() - 12.0, t);
    painter.circle_filled(
        egui::pos2(knob_x, rect.center().y),
        8.5,
        lerp_color(MUTED, egui::Color32::WHITE, t),
    );
    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}

/// Küçük durum noktası.
pub fn dot(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32, glow: f32) {
    if glow > 0.01 {
        painter.circle_filled(center, 7.0 + glow * 3.0, with_alpha(color, 0.14 * glow));
    }
    painter.circle_filled(center, 4.0, color);
}

/// Yatay ilerleme çubuğu.
pub fn progress(painter: &egui::Painter, rect: egui::Rect, ratio: f32, from: egui::Color32, to: egui::Color32) {
    painter.rect_filled(rect, 999.0, SURFACE_HI);
    let clamped = ratio.clamp(0.0, 1.0);
    if clamped <= 0.0 {
        return;
    }
    let mut filled = rect;
    let width = rect.width() * clamped;
    filled = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + width, rect.bottom()));
    paint_gradient(painter, filled, from, to, 999.0);
}
