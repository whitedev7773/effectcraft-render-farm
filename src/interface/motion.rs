use super::*;

const CONTROL_TIME: f32 = 0.14;
const DIALOG_TIME: f32 = 0.20;
const PROGRESS_TIME: f32 = 0.22;
const DIALOG_OFFSET: f32 = 6.0;
const BACKDROP_ALPHA: u8 = 160;

pub(super) fn apply_style(ctx: &egui::Context, reduced: bool) {
    let duration = if reduced { 0.0 } else { CONTROL_TIME };
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        if ctx.style_of(theme).animation_time != duration {
            ctx.style_mut_of(theme, |style| style.animation_time = duration);
        }
    }
}

fn time(ui: &egui::Ui, seconds: f32) -> f32 {
    if ui.ctx().global_style().animation_time == 0.0 { 0.0 } else { seconds }
}

pub(super) fn highlight(ui: &egui::Ui, id: egui::Id, active: bool) -> f32 {
    let value = ui.ctx().animate_bool_with_time(id, active, time(ui, CONTROL_TIME));
    value * value * (3.0 - 2.0 * value)
}

pub(super) fn progress(ui: &egui::Ui, key: impl egui::AsIdSalt, fraction: f32) -> f32 {
    let target = fraction.clamp(0.0, 1.0);
    let seconds = if target == 0.0 { 0.0 } else { time(ui, PROGRESS_TIME) };
    ui.ctx().animate_value_with_time(ui.id().with(("progress", key)), target, seconds)
}

pub(super) fn busy_indicator(ui: &mut egui::Ui) {
    if time(ui, CONTROL_TIME) != 0.0 {
        ui.spinner();
        return;
    }
    let size = ui.spacing().interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator));
    for offset in [-5.0, 0.0, 5.0] {
        ui.painter().circle_filled(rect.center() + egui::vec2(offset, 0.0), 1.5, ui.visuals().strong_text_color());
    }
}

#[derive(Clone, Copy)]
struct DialogOpening {
    frame: u64,
    started: f64,
}

pub(super) fn animated_modal(ctx: &egui::Context, id: &str, open: bool, reduced: bool, frame: egui::Frame) -> Option<(egui::Modal, f32)> {
    let id = egui::Id::new(id);
    if !open {
        ctx.data_mut(|data| data.remove::<DialogOpening>(id.with("opening")));
        return None;
    }
    Some(opening_modal(ctx, id, reduced, frame))
}

pub(super) fn opening_modal(ctx: &egui::Context, id: egui::Id, reduced: bool, frame: egui::Frame) -> (egui::Modal, f32) {
    let opening_id = id.with("opening");
    let now = ctx.input(|input| input.time);
    let frame_nr = ctx.cumulative_frame_nr();
    let reveal = ctx.data_mut(|data| {
        let mut opening = data
            .get_temp::<DialogOpening>(opening_id)
            .filter(|opening| frame_nr.saturating_sub(opening.frame) <= 1)
            .unwrap_or(DialogOpening { frame: frame_nr, started: now });
        opening.frame = frame_nr;
        if reduced {
            opening.started = now - f64::from(DIALOG_TIME);
        }
        data.insert_temp(opening_id, opening);
        let elapsed = ((now - opening.started) as f32 / DIALOG_TIME).clamp(0.0, 1.0);
        egui::emath::easing::cubic_out(elapsed)
    });
    if reveal < 1.0 {
        ctx.request_repaint();
    }
    let area = egui::Modal::default_area(id).fade_in(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, (1.0 - reveal) * DIALOG_OFFSET));
    let backdrop = egui::Color32::from_black_alpha(BACKDROP_ALPHA).gamma_multiply(reveal);
    (egui::Modal::new(id).area(area).frame(frame.multiply_with_opacity(reveal)).backdrop_color(backdrop), reveal)
}
