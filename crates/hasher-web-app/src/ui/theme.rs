//! Identità visiva: palette (PIANO §3), font embeddati, stili del testo e `Visuals` chiaro/scuro.
//!
//! Nessun colore è definito fuori da questo file: le viste leggono i token con [`pal`].

use crate::backend::settings::ThemeMode;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, Style,
    TextStyle, Theme, ThemePreference, Visuals,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Raggio degli angoli dei controlli (pulsanti, campi, menu).
pub const RADIUS_WIDGET: u8 = 8;
/// Raggio degli angoli di pannelli, card e finestre.
pub const RADIUS_CARD: u8 = 12;

// Nomi delle famiglie di font registrate.
const F_INTER: &str = "Inter-Regular";
const F_INTER_MEDIUM: &str = "Inter-Medium";
const F_INTER_SEMIBOLD: &str = "Inter-SemiBold";
const F_INTER_BOLD: &str = "Inter-Bold";
const F_MONO: &str = "JetBrainsMono-Regular";
const F_MONO_MEDIUM: &str = "JetBrainsMono-Medium";
const F_ICONS: &str = "icons";

/// Famiglia "medium" (pulsanti, etichette di sezione).
pub fn family_medium() -> FontFamily {
    FontFamily::Name(F_INTER_MEDIUM.into())
}
/// Famiglia "semibold" (titoli).
pub fn family_semibold() -> FontFamily {
    FontFamily::Name(F_INTER_SEMIBOLD.into())
}
/// Famiglia "bold" (titolo principale).
pub fn family_bold() -> FontFamily {
    FontFamily::Name(F_INTER_BOLD.into())
}
/// Famiglia monospazio "medium" (hash evidenziati).
pub fn family_mono_medium() -> FontFamily {
    FontFamily::Name(F_MONO_MEDIUM.into())
}

pub fn font_regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn font_medium(size: f32) -> FontId {
    FontId::new(size, family_medium())
}
pub fn font_semibold(size: f32) -> FontId {
    FontId::new(size, family_semibold())
}
pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, family_bold())
}
pub fn font_mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn font_mono_medium(size: f32) -> FontId {
    FontId::new(size, family_mono_medium())
}
/// Font per le icone Phosphor: famiglia dedicata con Phosphor al primo posto (Inter contiene
/// alcuni glifi nell'area d'uso privato che altrimenti coprirebbero le icone).
pub fn font_icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(F_ICONS.into()))
}

/// Stile di testo personalizzato per i titoli grandi.
pub fn title_style() -> TextStyle {
    TextStyle::Name("Title".into())
}

/// Token di colore per un tema.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Sfondo della finestra.
    pub bg: Color32,
    /// Superficie delle card.
    pub surface: Color32,
    /// Superficie elevata (intestazioni, controlli a riposo).
    pub elevated: Color32,
    /// Variante più marcata della superficie elevata (hover).
    pub elevated_hover: Color32,
    /// Variante per lo stato premuto.
    pub elevated_active: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    /// Testo ancora più tenue (segnaposti, separatori testuali).
    pub text_faint: Color32,
    pub primary: Color32,
    /// Riempimento dei pulsanti primari (contrasto AA con testo bianco).
    pub primary_fill: Color32,
    pub primary_fill_hover: Color32,
    /// Testo sopra `primary_fill`.
    pub on_primary: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub error: Color32,
    pub warning: Color32,
    /// Bordi sottili di card e separatori.
    pub border: Color32,
    /// Bordo più visibile (campi, controlli).
    pub border_strong: Color32,
    /// Sfondo dei campi di testo.
    pub field: Color32,
    /// Colore base degli scheletri di caricamento.
    pub skeleton: Color32,
    /// Riflesso dell'effetto shimmer.
    pub shimmer: Color32,
    /// Ombra delle card.
    pub shadow: Color32,
    /// Ombra delle finestre e dei popup.
    pub shadow_strong: Color32,
    /// Tessera chiara dietro il marchio (il navy del logo deve restare leggibile).
    pub brand_tile: Color32,
}

impl Palette {
    /// Versione traslucida del primario (selezioni, evidenziazioni, overlay).
    pub fn primary_soft(&self) -> Color32 {
        self.primary.gamma_multiply(if self.dark { 0.22 } else { 0.12 })
    }
    pub fn success_soft(&self) -> Color32 {
        self.success.gamma_multiply(if self.dark { 0.16 } else { 0.10 })
    }
    pub fn error_soft(&self) -> Color32 {
        self.error.gamma_multiply(if self.dark { 0.16 } else { 0.10 })
    }
    pub fn warning_soft(&self) -> Color32 {
        self.warning.gamma_multiply(if self.dark { 0.16 } else { 0.12 })
    }
}

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xF4, 0xF7, 0xFB),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    elevated: Color32::from_rgb(0xEA, 0xF0, 0xF7),
    elevated_hover: Color32::from_rgb(0xDF, 0xE8, 0xF2),
    elevated_active: Color32::from_rgb(0xD2, 0xDE, 0xEC),
    text: Color32::from_rgb(0x15, 0x1F, 0x35),
    text_muted: Color32::from_rgb(0x5B, 0x6B, 0x82),
    text_faint: Color32::from_rgb(0x8E, 0x9B, 0xAE),
    primary: Color32::from_rgb(0x14, 0x47, 0x81),
    primary_fill: Color32::from_rgb(0x14, 0x47, 0x81),
    primary_fill_hover: Color32::from_rgb(0x1B, 0x56, 0x99),
    on_primary: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    accent: Color32::from_rgb(0x5B, 0x85, 0xA1),
    success: Color32::from_rgb(0x1F, 0x8A, 0x5B),
    error: Color32::from_rgb(0xC8, 0x43, 0x3F),
    warning: Color32::from_rgb(0xB9, 0x78, 0x0A),
    border: Color32::from_rgb(0xE0, 0xE7, 0xF0),
    border_strong: Color32::from_rgb(0xC9, 0xD4, 0xE2),
    field: Color32::from_rgb(0xF8, 0xFA, 0xFD),
    skeleton: Color32::from_rgb(0xE4, 0xEA, 0xF2),
    shimmer: Color32::from_rgba_premultiplied(0xB8, 0xB8, 0xB8, 0xB8),
    shadow: Color32::from_rgba_unmultiplied_const(0x0B, 0x1A, 0x33, 0x16),
    shadow_strong: Color32::from_rgba_unmultiplied_const(0x0B, 0x1A, 0x33, 0x30),
    brand_tile: Color32::from_rgb(0xFF, 0xFF, 0xFF),
};

pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x0E, 0x15, 0x26),
    surface: Color32::from_rgb(0x17, 0x22, 0x38),
    elevated: Color32::from_rgb(0x1F, 0x2C, 0x47),
    elevated_hover: Color32::from_rgb(0x27, 0x37, 0x57),
    elevated_active: Color32::from_rgb(0x2F, 0x42, 0x68),
    text: Color32::from_rgb(0xE6, 0xEC, 0xF4),
    text_muted: Color32::from_rgb(0x9A, 0xAA, 0xC2),
    text_faint: Color32::from_rgb(0x6B, 0x7B, 0x95),
    primary: Color32::from_rgb(0x4F, 0x86, 0xC6),
    primary_fill: Color32::from_rgb(0x2F, 0x64, 0xA6),
    primary_fill_hover: Color32::from_rgb(0x3A, 0x73, 0xB8),
    on_primary: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    accent: Color32::from_rgb(0x7F, 0xA6, 0xC0),
    success: Color32::from_rgb(0x4C, 0xC3, 0x8A),
    error: Color32::from_rgb(0xE5, 0x63, 0x5F),
    warning: Color32::from_rgb(0xF2, 0xB0, 0x35),
    border: Color32::from_rgb(0x26, 0x34, 0x52),
    border_strong: Color32::from_rgb(0x34, 0x46, 0x69),
    field: Color32::from_rgb(0x11, 0x1A, 0x2E),
    skeleton: Color32::from_rgb(0x22, 0x30, 0x4C),
    shimmer: Color32::from_rgba_premultiplied(0x1C, 0x1C, 0x1C, 0x1C),
    shadow: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x40),
    shadow_strong: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x80),
    brand_tile: Color32::from_rgb(0xEA, 0xF0, 0xF7),
};

/// Palette per il tema corrente della `Ui`.
pub fn pal(ui: &egui::Ui) -> &'static Palette {
    palette(ui.visuals().dark_mode)
}

/// Palette per il tema corrente del contesto.
pub fn pal_ctx(ctx: &Context) -> &'static Palette {
    palette(ctx.global_style().visuals.dark_mode)
}

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// Installa i font embeddati (Inter, JetBrains Mono, Phosphor). Da chiamare una volta all'avvio.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(F_INTER, include_bytes!("../../assets/fonts/Inter-Regular.ttf"));
    add(F_INTER_MEDIUM, include_bytes!("../../assets/fonts/Inter-Medium.ttf"));
    add(
        F_INTER_SEMIBOLD,
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(F_INTER_BOLD, include_bytes!("../../assets/fonts/Inter-Bold.ttf"));
    add(F_MONO, include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"));
    add(
        F_MONO_MEDIUM,
        include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf"),
    );

    // Fallback predefiniti di egui (simboli, emoji) dopo i nostri font.
    let default_prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let default_mono = fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

    let mut prop = vec![F_INTER.to_owned()];
    prop.extend(default_prop.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, prop);

    let mut mono = vec![F_MONO.to_owned()];
    mono.extend(default_mono.iter().cloned());
    fonts.families.insert(FontFamily::Monospace, mono);

    // Phosphor: inserito al secondo posto della famiglia proporzionale (icone mescolabili al testo).
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Famiglie con peso specifico: [peso, phosphor, fallback…].
    for (family, primary) in [
        (family_medium(), F_INTER_MEDIUM),
        (family_semibold(), F_INTER_SEMIBOLD),
        (family_bold(), F_INTER_BOLD),
        (family_mono_medium(), F_MONO_MEDIUM),
    ] {
        let mut list = vec![primary.to_owned(), "phosphor".to_owned()];
        if primary == F_MONO_MEDIUM {
            list.extend(default_mono.iter().cloned());
        } else {
            list.extend(default_prop.iter().cloned());
        }
        fonts.families.insert(family, list);
    }

    let mut icon_list = vec!["phosphor".to_owned(), F_INTER.to_owned()];
    icon_list.extend(default_prop.iter().cloned());
    fonts.families.insert(FontFamily::Name(F_ICONS.into()), icon_list);

    ctx.set_fonts(fonts);
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    [
        (TextStyle::Small, font_regular(12.0)),
        (TextStyle::Body, font_regular(15.0)),
        (TextStyle::Button, font_medium(15.0)),
        (TextStyle::Heading, font_semibold(22.0)),
        (TextStyle::Monospace, font_mono(14.0)),
        (title_style(), font_bold(28.0)),
    ]
    .into()
}

fn shadow(offset_y: i8, blur: u8, color: Color32) -> Shadow {
    Shadow {
        offset: [0, offset_y],
        blur,
        spread: 0,
        color,
    }
}

/// Costruisce i `Visuals` per una palette.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    let r = CornerRadius::same(RADIUS_WIDGET);

    v.override_text_color = None;
    v.weak_text_color = Some(p.text_muted);
    v.hyperlink_color = p.primary;
    v.faint_bg_color = if p.dark {
        p.surface.lerp_to_gamma(p.elevated, 0.35)
    } else {
        p.field
    };
    v.extreme_bg_color = p.field;
    v.text_edit_bg_color = Some(p.field);
    v.code_bg_color = p.elevated;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;

    v.window_corner_radius = CornerRadius::same(RADIUS_CARD);
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_shadow = shadow(10, 28, p.shadow_strong);
    v.window_highlight_topmost = false;
    v.menu_corner_radius = CornerRadius::same(10);
    v.popup_shadow = shadow(6, 18, p.shadow_strong);
    v.panel_fill = p.bg;

    v.selection.bg_fill = p.primary.gamma_multiply(if p.dark { 0.45 } else { 0.25 });
    v.selection.stroke = Stroke::new(
        1.0,
        if p.dark {
            Color32::from_rgb(0xDC, 0xE8, 0xF7)
        } else {
            p.primary
        },
    );
    v.text_cursor.stroke = Stroke::new(2.0, p.primary);

    v.button_frame = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    v.image_loading_spinners = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;
    w.noninteractive.expansion = 0.0;

    w.inactive.bg_fill = p.elevated;
    w.inactive.weak_bg_fill = p.elevated;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border_strong.gamma_multiply(0.0));
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = p.elevated_hover;
    w.hovered.weak_bg_fill = p.elevated_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.elevated_active;
    w.active.weak_bg_fill = p.elevated_active;
    w.active.bg_stroke = Stroke::new(1.0, p.primary);
    w.active.fg_stroke = Stroke::new(2.0, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.elevated_hover;
    w.open.weak_bg_fill = p.elevated_hover;
    w.open.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = r;
    w.open.expansion = 0.0;

    v
}

fn style_tweaks(style: &mut Style, animations: bool) {
    style.text_styles = text_styles();
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(12.0, 5.0);
    s.interact_size = egui::vec2(36.0, 28.0);
    s.window_margin = Margin::same(18);
    s.menu_margin = Margin::same(6);
    s.icon_width = 18.0;
    s.icon_width_inner = 10.0;
    s.icon_spacing = 8.0;
    s.combo_height = 320.0;
    s.menu_width = 260.0;
    s.tooltip_width = 360.0;
    s.scroll = egui::style::ScrollStyle::floating();
    s.scroll.bar_width = 8.0;
    style.animation_time = if animations { 0.18 } else { 0.0 };
    style.interaction.selectable_labels = true;
    style.url_in_tooltip = true;
}

/// Applica tema (chiaro/scuro/automatico) e stili. Chiamare all'avvio e a ogni cambio di tema
/// o dell'impostazione delle animazioni.
pub fn apply(ctx: &Context, mode: ThemeMode, animations: bool) {
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK));
    ctx.all_styles_mut(|style| style_tweaks(style, animations));
    let pref = match mode {
        ThemeMode::System => ThemePreference::System,
        ThemeMode::Light => ThemePreference::Light,
        ThemeMode::Dark => ThemePreference::Dark,
    };
    ctx.options_mut(|o| {
        o.theme_preference = pref;
        o.fallback_theme = Theme::Light;
    });
}

/// Mescola due colori nello spazio gamma (0 = `a`, 1 = `b`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}
