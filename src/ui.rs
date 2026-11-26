//! Aspetto della finestra: colori, dimensioni del testo e i pochi controlli che egui non disegna come serve.
//! Palette e forme vengono da Camallo e Stiva, così i programmi si riconoscono come una famiglia.
//! Le icone sono disegnate con linee e forme, quindi non dipendono dai glifi del font.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Margin, Painter, PointerButton, Pos2, Rect,
    ResizeDirection, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui,
    ViewportCommand, pos2, vec2,
};

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1c, 0x1e);
pub const PANEL: Color32 = Color32::from_rgb(0x22, 0x25, 0x27);
pub const CARD: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x30);
pub const FIELD: Color32 = Color32::from_rgb(0x34, 0x38, 0x3b);
pub const LINE: Color32 = Color32::from_rgb(0x3b, 0x40, 0x44);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xec);
pub const MUTED: Color32 = Color32::from_rgb(0x9b, 0xa1, 0xa7);
pub const TITLE: Color32 = Color32::from_rgb(0x0f, 0x10, 0x11);
/// Accento: barra di avanzamento e contorni.
pub const ACCENT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);
/// Riempimento dei pulsanti principali: più scuro dell'accento, così il bianco sopra si legge a 5:1.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x7c, 0x4d, 0xee);
pub const ACCENT_FILL_HOVER: Color32 = Color32::from_rgb(0x85, 0x57, 0xf2);
/// L'accento come testo su fondo scuro: più chiaro, così resta leggibile.
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xb9, 0xa3, 0xfb);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2b, 0x23, 0x48);
pub const ON_ACCENT: Color32 = Color32::WHITE;
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x6e, 0x6e);
pub const SUCCESS: Color32 = Color32::from_rgb(0x4e, 0xc3, 0x8b);
/// Qualcosa da guardare che non è un errore: un file che esiste già, un nome doppio.
pub const WARN: Color32 = Color32::from_rgb(0xe3, 0xa6, 0x3c);

const HOVER: Color32 = Color32::from_rgb(0x42, 0x47, 0x4c);
const PRESSED: Color32 = Color32::from_rgb(0x4c, 0x52, 0x58);

/// Tema scuro con un solo colore d'accento, forme arrotondate e un po' più d'aria di quella di egui.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(14);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = FIELD;
        v.faint_bg_color = CARD;
        v.hyperlink_color = ACCENT_TEXT;
        v.selection.bg_fill = ACCENT_DIM;
        v.selection.stroke = Stroke::new(1.0, ACCENT_TEXT);
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT_TEXT);

        let r = CornerRadius::same(9);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = PANEL;
        w.noninteractive.weak_bg_fill = PANEL;
        w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = FIELD;
        w.inactive.weak_bg_fill = FIELD;
        // Un contorno leggero, così caselle e campi si vedono anche sul fondo delle card.
        w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4f, 0x54));
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, MUTED);
        w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.active.bg_fill = PRESSED;
        w.active.weak_bg_fill = PRESSED;
        w.active.bg_stroke = Stroke::new(1.0, ACCENT_TEXT);
        w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        w.active.corner_radius = r;
        w.open.bg_fill = FIELD;
        w.open.weak_bg_fill = FIELD;
        w.open.bg_stroke = Stroke::new(1.0, MUTED);
        w.open.fg_stroke = Stroke::new(1.0, TEXT);
        w.open.corner_radius = r;

        s.spacing.item_spacing = vec2(8.0, 6.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.combo_width = 90.0;

        use egui::FontFamily::{Monospace, Proportional};
        s.text_styles = [
            (TextStyle::Heading, FontId::new(20.0, Proportional)),
            (TextStyle::Body, FontId::new(14.5, Proportional)),
            (TextStyle::Button, FontId::new(14.5, Proportional)),
            (TextStyle::Small, FontId::new(12.0, Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, Monospace)),
        ]
        .into();
    });
}

/// Su Windows c'è Segoe UI, che si legge meglio del font incluso; quelli inclusi restano di riserva.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn fonts(ctx: &egui::Context) {
    #[cfg(windows)]
    {
        use std::sync::Arc;
        let dir = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into())
            .join("Fonts");
        let mut defs = egui::FontDefinitions::default();
        let mut found = false;
        if let Ok(bytes) = std::fs::read(dir.join("segoeui.ttf")) {
            defs.font_data.insert(
                "segoeui".into(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            defs.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoeui".into());
            found = true;
        }
        if let Ok(bytes) = std::fs::read(dir.join("seguisym.ttf")) {
            defs.font_data.insert(
                "seguisym".into(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            defs.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("seguisym".into());
            found = true;
        }
        if found {
            ctx.set_fonts(defs);
        }
    }
}

pub fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t.into()).color(MUTED).size(13.0)
}

/// Il trascinamento dei file arriva alla finestra solo fuori da Wayland (limite della libreria delle finestre).
pub fn drop_works() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// L'unica azione del momento.
    Primary,
    Soft,
    /// Senza sfondo finché il puntatore non ci passa sopra.
    Ghost,
    /// Una scelta attiva.
    Selected,
}

impl Kind {
    /// Testo, riempimento, riempimento sotto il puntatore, contorno.
    fn colors(self) -> (Color32, Color32, Color32, Stroke) {
        match self {
            Kind::Primary => (ON_ACCENT, ACCENT_FILL, ACCENT_FILL_HOVER, Stroke::NONE),
            Kind::Soft => (TEXT, FIELD, HOVER, Stroke::NONE),
            Kind::Ghost => (MUTED, Color32::TRANSPARENT, FIELD, Stroke::NONE),
            Kind::Selected => (
                ACCENT_TEXT,
                ACCENT_DIM,
                Color32::from_rgb(0x38, 0x2d, 0x5e),
                Stroke::new(1.0, ACCENT),
            ),
        }
    }
}

/// Pulsante arrotondato. `size` è la dimensione del testo, così l'azione principale può essere più grande.
pub fn pill(ui: &mut Ui, text: &str, kind: Kind, size: f32) -> Response {
    let (fg, fill, hover, stroke) = kind.colors();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(size), fg);
    let pad = vec2(size * 0.95, size * 0.5);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let bg = if !enabled {
            fill.gamma_multiply(0.45)
        } else if resp.is_pointer_button_down_on() {
            hover.gamma_multiply(0.85)
        } else if resp.hovered() {
            hover
        } else {
            fill
        };
        let text_color = if kind == Kind::Ghost && resp.hovered() {
            TEXT
        } else {
            fg
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(255),
            bg,
            stroke,
            StrokeKind::Inside,
        );
        let at = rect.center() - galley.size() / 2.0;
        let text_color = if enabled {
            text_color
        } else {
            text_color.gamma_multiply(0.5)
        };
        ui.painter()
            .galley_with_override_text_color(at, galley, text_color);
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Pulsante per il testo di dimensione normale.
pub fn button(ui: &mut Ui, text: &str, kind: Kind) -> Response {
    pill(ui, text, kind, 14.0)
}

/// Un riquadro arrotondato per un blocco della finestra.
pub fn card<R>(ui: &mut Ui, outline: Stroke, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(outline)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Titolo di una card.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.5).strong().color(TEXT));
}

/// Una breve etichetta su una capsula colorata: lo stato di un file, un conteggio.
/// Disegnata a mano, così ha sempre la stessa altezza in qualunque riga.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let mix = |a: u8, b: u8| (a as f32 * 0.82 + b as f32 * 0.18) as u8;
    let fill = Color32::from_rgb(
        mix(BG.r(), color.r()),
        mix(BG.g(), color.g()),
        mix(BG.b(), color.b()),
    );
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.5), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(18.0, 6.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(255), fill);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    resp
}

fn cross_lines(p: &Painter, c: Pos2, d: f32, stroke: Stroke) {
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
    p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
}

/// Una "x" tonda per chiudere o togliere qualcosa.
pub fn close(ui: &mut Ui) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered();
        if hot {
            ui.painter().circle_filled(rect.center(), 13.0, FIELD);
        }
        cross_lines(
            ui.painter(),
            rect.center(),
            4.5,
            Stroke::new(1.6, if hot { TEXT } else { MUTED }),
        );
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    resp
}

/// Barra di avanzamento sottile. Senza frazione un segmento corto va avanti e indietro.
pub fn bar(ui: &mut Ui, fraction: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let r = CornerRadius::same(3);
    ui.painter().rect_filled(rect, r, LINE);
    match fraction {
        Some(f) => {
            let w = rect.width() * f.clamp(0.0, 1.0);
            if w > 0.5 {
                ui.painter().rect_filled(
                    Rect::from_min_size(rect.min, vec2(w.max(6.0), rect.height())),
                    r,
                    ACCENT,
                );
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let phase = (t * 0.8).fract();
            let pos = if phase < 0.5 {
                phase * 2.0
            } else {
                (1.0 - phase) * 2.0
            };
            let w = rect.width() * 0.2;
            let x = rect.left() + (rect.width() - w) * pos;
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())),
                r,
                ACCENT,
            );
            ui.ctx().request_repaint();
        }
    }
}

/// La bobina dell'icona, disegnata con forme: disco, sei fori e mozzo.
fn reel(p: &Painter, c: Pos2, r: f32, disk: Color32, holes: Color32) {
    p.circle_filled(c, r, disk);
    for k in 0..6 {
        let a = std::f32::consts::TAU * k as f32 / 6.0 - std::f32::consts::FRAC_PI_2;
        p.circle_filled(c + vec2(a.cos(), a.sin()) * r * 0.6, r * 0.25, holes);
    }
    p.circle_filled(c, r * 0.11, holes);
}

/// Bobina in contorno, per l'elenco vuoto; si accende quando ci passa sopra un file trascinato.
pub fn empty_icon(ui: &mut Ui, hot: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
    let ink = if hot { ACCENT_TEXT } else { LINE };
    let p = ui.painter();
    let c = rect.center();
    p.circle_stroke(c, 34.0, Stroke::new(2.5, ink));
    for k in 0..6 {
        let a = std::f32::consts::TAU * k as f32 / 6.0 - std::f32::consts::FRAC_PI_2;
        p.circle_stroke(
            c + vec2(a.cos(), a.sin()) * 20.0,
            7.5,
            Stroke::new(2.0, ink),
        );
    }
    p.circle_filled(c, 3.5, ink);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Folder,
    Pencil,
    Close,
    Minimize,
    Maximize,
    Restore,
}

/// Un'icona larga circa 16 px attorno a `c`, disegnata con linee.
pub fn draw_icon(p: &Painter, c: Pos2, icon: Icon, color: Color32) {
    let st = |w: f32| Stroke::new(w, color);
    let at = |x: f32, y: f32| c + vec2(x, y);
    match icon {
        Icon::Close => cross_lines(p, c, 5.0, st(1.6)),
        Icon::Minimize => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], st(1.3));
        }
        Icon::Maximize => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(10.0, 10.0)),
                CornerRadius::ZERO,
                st(1.3),
                StrokeKind::Middle,
            );
        }
        Icon::Restore => {
            p.rect_stroke(
                Rect::from_center_size(at(-1.0, 1.0), vec2(8.0, 8.0)),
                CornerRadius::ZERO,
                st(1.3),
                StrokeKind::Middle,
            );
            p.add(Shape::line(
                vec![at(-2.0, -5.0), at(5.0, -5.0), at(5.0, 2.0)],
                st(1.3),
            ));
        }
        Icon::Pencil => {
            // Corpo inclinato, punta in basso a sinistra, cappuccio in alto a destra.
            p.line_segment([at(-5.0, 5.0), at(4.0, -4.0)], st(3.2));
            p.line_segment([at(3.0, -7.0), at(7.0, -3.0)], st(1.6));
            p.add(Shape::convex_polygon(
                vec![at(-7.0, 7.0), at(-6.2, 3.6), at(-3.6, 6.2)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Folder => {
            p.add(Shape::closed_line(
                vec![
                    at(-8.0, -5.5),
                    at(-3.0, -5.5),
                    at(-1.0, -3.0),
                    at(8.0, -3.0),
                    at(8.0, 6.0),
                    at(-8.0, 6.0),
                ],
                st(1.7),
            ));
        }
    }
}

/// Pulsante tondo con un'icona; `on` la colora d'accento (per esempio: dati corretti a mano).
pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = resp.hovered() && enabled;
        if on {
            ui.painter()
                .circle_filled(rect.center(), size / 2.0, ACCENT_DIM);
        } else if hot {
            ui.painter().circle_filled(rect.center(), size / 2.0, FIELD);
        }
        let color = match (on, hot) {
            (true, _) => ACCENT_TEXT,
            (false, true) => TEXT,
            _ => MUTED,
        };
        draw_icon(
            ui.painter(),
            rect.center(),
            icon,
            if enabled {
                color
            } else {
                color.gamma_multiply(0.4)
            },
        );
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Pulsante della barra del titolo. `danger` diventa rosso sotto il puntatore, come la chiusura di Windows.
fn window_button(ui: &mut Ui, icon: Icon, danger: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(46.0, ui.available_height()), Sense::click());
    if resp.hovered() {
        let fill = if danger {
            Color32::from_rgb(0xe8, 0x11, 0x23)
        } else {
            Color32::from_rgb(0x2a, 0x2c, 0x2f)
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    }
    let color = if resp.hovered() {
        Color32::WHITE
    } else {
        MUTED
    };
    draw_icon(ui.painter(), rect.center(), icon, color);
    resp
}

pub const TITLE_HEIGHT: f32 = 34.0;

/// La finestra non ha la cornice del sistema, quindi questa barra fa ciò che faceva la cornice:
/// nome, spostamento, riduzione, ingrandimento, chiusura.
pub fn title_bar(ui: &mut Ui, name: &str, minimize: &str, maximize: &str, restore: &str) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let bar = ui.max_rect();
    // Aggiunta per prima, così i pulsanti sopra vincono il puntatore dove si sovrappongono.
    let drag = ui.interact(
        bar,
        ui.scope_id().with("title_drag"),
        Sense::click_and_drag(),
    );
    if drag.drag_started_by(PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }
    let p = ui.painter();
    // L'icona del programma in piccolo: tessera viola con la bobina bianca.
    let mid = pos2(bar.left() + 25.0, bar.center().y);
    p.rect_filled(
        Rect::from_center_size(mid, vec2(16.0, 16.0)),
        CornerRadius::same(5),
        ACCENT,
    );
    reel(p, mid, 5.8, ON_ACCENT, ACCENT_FILL);
    p.text(
        pos2(bar.left() + 40.0, bar.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        MUTED,
    );

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        if window_button(ui, Icon::Close, true).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let (icon, tip) = if maximized {
            (Icon::Restore, restore)
        } else {
            (Icon::Maximize, maximize)
        };
        if window_button(ui, icon, false).on_hover_text(tip).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if window_button(ui, Icon::Minimize, false)
            .on_hover_text(minimize)
            .clicked()
        {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
    });
}

/// Su quale bordo o angolo della finestra cade un punto, entro `m` pixel dal bordo.
pub fn edge_at(p: Pos2, r: Rect, m: f32) -> Option<ResizeDirection> {
    use ResizeDirection::*;
    let (l, rt, t, b) = (
        p.x - r.left() < m,
        r.right() - p.x < m,
        p.y - r.top() < m,
        r.bottom() - p.y < m,
    );
    match (l, rt, t, b) {
        (true, _, true, _) => Some(NorthWest),
        (_, true, true, _) => Some(NorthEast),
        (true, _, _, true) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, ..) => Some(West),
        (_, true, ..) => Some(East),
        (_, _, true, _) => Some(North),
        (_, _, _, true) => Some(South),
        _ => None,
    }
}

fn resize_cursor(d: ResizeDirection) -> egui::CursorIcon {
    use ResizeDirection::*;
    use egui::CursorIcon as C;
    match d {
        North => C::ResizeNorth,
        South => C::ResizeSouth,
        East => C::ResizeEast,
        West => C::ResizeWest,
        NorthEast => C::ResizeNorthEast,
        NorthWest => C::ResizeNorthWest,
        SouthEast => C::ResizeSouthEast,
        SouthWest => C::ResizeSouthWest,
    }
}

/// Senza la cornice del sistema la finestra si ridimensiona dal bordo solo se lo fa questa funzione.
pub fn resize_edges(ctx: &egui::Context) {
    let (maximized, pos) = ctx.input(|i| {
        (
            i.viewport().maximized.unwrap_or(false),
            i.pointer.hover_pos(),
        )
    });
    let Some(pos) = pos.filter(|_| !maximized) else {
        return;
    };
    if let Some(dir) = edge_at(pos, ctx.content_rect(), 6.0) {
        ctx.set_cursor_icon(resize_cursor(dir));
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

/// Una linea sottile attorno alla finestra, visto che non c'è la cornice a mostrare dove finisce.
pub fn border(ctx: &egui::Context) {
    ctx.layer_painter(egui::LayerId::debug()).rect_stroke(
        ctx.content_rect(),
        CornerRadius::ZERO,
        Stroke::new(1.0, LINE),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_bordi_indicano_il_lato_o_l_angolo() {
        use ResizeDirection::*;
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
        assert_eq!(edge_at(pos2(50.0, 40.0), r, 6.0), None);
        assert_eq!(edge_at(pos2(2.0, 40.0), r, 6.0), Some(West));
        assert_eq!(edge_at(pos2(98.0, 40.0), r, 6.0), Some(East));
        assert_eq!(edge_at(pos2(50.0, 1.0), r, 6.0), Some(North));
        assert_eq!(edge_at(pos2(50.0, 79.0), r, 6.0), Some(South));
        assert_eq!(edge_at(pos2(1.0, 1.0), r, 6.0), Some(NorthWest));
        assert_eq!(edge_at(pos2(99.0, 79.0), r, 6.0), Some(SouthEast));
        assert_eq!(
            edge_at(pos2(7.0, 40.0), r, 6.0),
            None,
            "appena dentro la zona"
        );
    }
}
