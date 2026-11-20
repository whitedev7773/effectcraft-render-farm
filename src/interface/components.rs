//! Native component patterns adapted from shadcn/ui, Blueprint, Fluent 2 and Ant Design.
//! See docs/ui-component-references.md for the design references.
use super::*;

pub(super) fn segmented(ui: &mut egui::Ui, id: &str, selected: usize, labels: &[&str]) -> Option<usize> {
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| ui.painter().layout_no_wrap((*label).into(), egui::FontId::proportional(13.0), ui.visuals().text_color()).size().x + 24.0)
        .collect();
    let natural = widths.iter().sum::<f32>() + 6.0;
    let width = natural.min(ui.available_width());
    let (bounds, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    ui.painter().rect_filled(bounds, 6, secondary(ui));
    ui.painter().rect_stroke(bounds, 6, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
    let scale = (width - 6.0).max(1.0) / (natural - 6.0).max(1.0);
    let mut left = bounds.left() + 3.0;
    let mut clicked = None;
    for (index, (label, width)) in labels.iter().zip(widths).enumerate() {
        let rect = egui::Rect::from_min_size(egui::pos2(left, bounds.top() + 3.0), egui::vec2(width * scale, 28.0));
        let response = ui.interact(rect, ui.id().with((id, index)), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), index == selected, label));
        if selected == index || response.hovered() {
            ui.painter().rect_filled(rect, 4, if selected == index { surface(ui) } else { ui.visuals().widgets.hovered.bg_fill });
        }
        if response.has_focus() {
            ui.painter().rect_stroke(rect, 4, egui::Stroke::new(1.0, accent(ui)), egui::StrokeKind::Inside);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(13.0),
            if selected == index { ui.visuals().text_color() } else { muted(ui) },
        );
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clicked = Some(index);
        }
        left = rect.right();
    }
    clicked
}

pub(super) fn status(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    let font = egui::FontId::proportional(11.0);
    let width = ui.painter().layout_no_wrap(label.into(), font.clone(), color).size().x + 28.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4, color.gamma_multiply(if ui.visuals().dark_mode { 0.12 } else { 0.09 }));
    ui.painter().circle_filled(rect.left_center() + egui::vec2(9.0, 0.0), 2.5, color);
    ui.painter().text(rect.left_center() + egui::vec2(17.0, 0.0), egui::Align2::LEFT_CENTER, label, font, color);
}

pub(super) fn section_heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(14.0).strong());
        if !description.is_empty() {
            caption(ui, description);
        }
    });
}
