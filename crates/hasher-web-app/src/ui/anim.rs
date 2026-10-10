//! Helper per le animazioni: pulsazione, shimmer, dissolvenze, contatori, spinner ad arco.
//!
//! Tutte le animazioni diventano istantanee/statiche quando l'utente le disattiva
//! (`Settings::animations`): lo stato è pubblicato a inizio frame con [`set_enabled`].

use egui::{Color32, Context, Id, Pos2, Rect, Shape, Stroke, Ui, Vec2};
use std::time::Duration;

/// Intervallo di repaint usato dalle animazioni continue (~30 fps: fluido ma parsimonioso).
const FRAME: Duration = Duration::from_millis(33);

fn enabled_id() -> Id {
    Id::new("hasher.anim.enabled")
}

/// Pubblica nel contesto se le animazioni sono attive (chiamare a inizio frame).
pub fn set_enabled(ctx: &Context, enabled: bool) {
    ctx.data_mut(|d| d.insert_temp(enabled_id(), enabled));
}

/// `true` se le animazioni sono attive.
pub fn enabled(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(enabled_id())).unwrap_or(true)
}

/// Tempo corrente del contesto in secondi.
pub fn now(ctx: &Context) -> f64 {
    ctx.input(|i| i.time)
}

/// Onda sinusoidale 0..1 con il periodo dato (secondi). Richiede il repaint.
/// Con animazioni disattivate restituisce sempre 0.
pub fn pulse(ctx: &Context, id: Id, period: f32) -> f32 {
    if !enabled(ctx) {
        return 0.0;
    }
    // Fase iniziale legata all'id, così più elementi non pulsano all'unisono.
    let phase = (id.value() % 1000) as f64 / 1000.0;
    let t = now(ctx) / period.max(0.05) as f64 + phase;
    ctx.request_repaint_after(FRAME);
    (0.5 - 0.5 * (t * std::f64::consts::TAU).cos()) as f32
}

/// Dissolvenza in entrata/uscita (0..1) legata a un booleano.
pub fn fade_in(ctx: &Context, id: Id, visible: bool) -> f32 {
    if !enabled(ctx) {
        return if visible { 1.0 } else { 0.0 };
    }
    ctx.animate_bool_with_time_and_easing(id, visible, 0.22, emath_ease_out)
}

fn emath_ease_out(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t)
}

/// Dissolvenza temporale: 0 → 1 nei `duration` secondi successivi a `since` (tempo del contesto).
pub fn appear(ctx: &Context, since: f64, duration: f32) -> f32 {
    if !enabled(ctx) {
        return 1.0;
    }
    let t = ((now(ctx) - since) as f32 / duration.max(0.01)).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    egui::emath::easing::cubic_out(t)
}

/// Valore numerico che si avvicina dolcemente al bersaglio (contatori animati).
pub fn animated_number(ctx: &Context, id: Id, target: f64) -> f64 {
    if !enabled(ctx) {
        return target;
    }
    let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
    let current = ctx.data(|d| d.get_temp::<f64>(id));
    let value = match current {
        None => target,
        Some(v) => {
            let k = 1.0 - (-dt * 10.0).exp();
            let next = v + (target - v) * k;
            if (target - next).abs() < 0.5 {
                target
            } else {
                next
            }
        }
    };
    ctx.data_mut(|d| d.insert_temp(id, value));
    if value != target {
        ctx.request_repaint();
    }
    value
}

/// Barra "scheletro" con riflesso in movimento (stato di attesa).
/// `t` è il tempo in secondi (di solito [`now`]).
pub fn shimmer_rect(ui: &Ui, rect: Rect, t: f64) {
    let p = super::theme::pal(ui);
    let radius = (rect.height() / 2.0).min(6.0);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, p.skeleton);
    if !enabled(ui.ctx()) {
        return;
    }
    paint_sweep(ui, rect, t, p.shimmer, radius);
    ui.ctx().request_repaint_after(FRAME);
}

/// Disegna un riflesso che scorre orizzontalmente dentro `rect` (usato da scheletri e barre).
pub fn paint_sweep(ui: &Ui, rect: Rect, t: f64, color: Color32, radius: f32) {
    let inner = rect.shrink2(Vec2::new(radius.min(rect.width() / 2.0), 0.0));
    if inner.width() <= 2.0 {
        return;
    }
    let period = 1.4_f64;
    let band = (rect.width() * 0.35).clamp(40.0, 220.0);
    let travel = inner.width() + band * 2.0;
    let phase = (t % period / period) as f32;
    let x = inner.left() - band + phase * travel;
    let painter = ui.painter().with_clip_rect(inner.intersect(ui.clip_rect()));
    let transparent = Color32::TRANSPARENT;
    let left = Rect::from_min_max(Pos2::new(x - band / 2.0, rect.top()), Pos2::new(x, rect.bottom()));
    let right = Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new(x + band / 2.0, rect.bottom()));
    painter.add(Shape::gradient_rect(
        left,
        egui::Direction::LeftToRight,
        [transparent, color],
    ));
    painter.add(Shape::gradient_rect(
        right,
        egui::Direction::LeftToRight,
        [color, transparent],
    ));
}

/// Spinner ad arco disegnato a mano. Restituisce la risposta dell'area allocata.
pub fn spinner_ring(ui: &mut Ui, size: f32, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint_spinner(ui, rect, color);
    response
}

/// Disegna lo spinner nel rettangolo dato.
pub fn paint_spinner(ui: &Ui, rect: Rect, color: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5 - 1.5;
    let stroke_w = (radius * 0.28).clamp(1.5, 3.0);
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(stroke_w, color.gamma_multiply(0.18)));

    let animated = enabled(ui.ctx());
    let t = if animated { now(ui.ctx()) } else { 0.0 };
    let start = t * 4.2;
    let sweep = if animated {
        1.2 + 0.9 * (t * 1.7).sin().abs()
    } else {
        1.6
    };
    let n = 24;
    let points: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * i as f64 / n as f64;
            center + radius * Vec2::new(a.cos() as f32, a.sin() as f32)
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(stroke_w, color)));
    if animated {
        ui.ctx().request_repaint_after(FRAME);
    }
}
