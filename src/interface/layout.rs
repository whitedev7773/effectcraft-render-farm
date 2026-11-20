use super::*;

pub(crate) const GAP: f32 = 16.0;
pub(crate) const SPACE: f32 = 8.0;
pub(crate) const PADDING: i8 = 16;
pub(crate) const CONTROL: f32 = 32.0;
pub(crate) const PRIMARY_CONTROL: f32 = 36.0;
pub(crate) const COMPACT_CONTROL: f32 = 28.0;
pub(crate) const RADIUS: u8 = 8;

#[derive(Clone, Copy, Debug)]
pub(super) struct DashboardGrid {
    pub job: egui::Rect,
    pub progress: egui::Rect,
    pub preview: egui::Rect,
    pub workers: egui::Rect,
}

impl DashboardGrid {
    pub fn new(rect: egui::Rect) -> Self {
        let left = ((rect.width() - GAP) * 0.58).round();
        let hero_height = (rect.height() * 0.48).clamp(316.0, 460.0).min(rect.height() - 336.0 - GAP);
        let progress_height = (hero_height * 0.60).max(188.0);
        let preview = egui::Rect::from_min_size(rect.min, egui::vec2(left, hero_height));
        let progress = egui::Rect::from_min_max(egui::pos2(preview.right() + GAP, rect.top()), egui::pos2(rect.right(), rect.top() + progress_height));
        let job = egui::Rect::from_min_max(egui::pos2(progress.left(), progress.bottom() + GAP), egui::pos2(rect.right(), preview.bottom()));
        let workers = egui::Rect::from_min_max(egui::pos2(rect.left(), preview.bottom() + GAP), rect.max);
        Self { job, progress, preview, workers }
    }
}

pub(super) fn tile<R>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    secondary: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.painter().rect_filled(rect, RADIUS, if secondary { super::secondary(ui) } else { surface(ui) });
    ui.painter().rect_stroke(rect, RADIUS, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
    let inner = rect.shrink(PADDING as f32);
    // The caller has allocated the outer rectangle. A child must not advance
    // the parent cursor again using its smaller padded content rectangle.
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = egui::vec2(SPACE, SPACE);
    egui::ScrollArea::vertical()
        .id_salt("tile-scroll")
        .auto_shrink([false, false])
        .max_height(inner.height())
        .show(&mut child, |ui| {
            ui.set_width(ui.available_width());
            content(ui)
        })
        .inner
}

pub(super) fn flow_tile<R>(ui: &mut egui::Ui, height: f32, id: impl std::hash::Hash + std::fmt::Debug, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    tile(ui, rect, id, false, content)
}

/// A naturally sized card for pages that already have a vertical scrollbar.
pub(super) fn flow_content<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(surface(ui))
        .stroke(egui::Stroke::new(1.0, border(ui)))
        .corner_radius(RADIUS)
        .inner_margin(PADDING)
        .show(ui, |ui| {
            ui.push_id(id, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(SPACE, SPACE);
                content(ui)
            })
            .inner
        })
        .inner
}

pub(super) fn value_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        let label_width = (ui.available_width() * 0.4).max(40.0);
        ui.add_sized([label_width, 20.0], egui::Label::new(egui::RichText::new(label).size(12.0).color(muted(ui))).truncate().halign(egui::Align::Min))
            .on_hover_text(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(value).truncate()).on_hover_text(value);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_tiles_fill_bounds_and_share_edges_and_gutters() {
        for (width, height) in [(900.0, 680.0), (1120.0, 704.0), (1408.0, 784.0), (1888.0, 912.0), (2528.0, 1272.0)] {
            let bounds = egui::Rect::from_min_size(egui::pos2(16.0, 112.0), egui::vec2(width, height));
            let grid = DashboardGrid::new(bounds);
            assert_eq!(grid.job.left(), grid.progress.left());
            assert_eq!(grid.job.right(), grid.workers.right());
            assert_eq!(grid.preview.left(), grid.workers.left());
            assert_eq!(grid.job.bottom(), grid.preview.bottom());
            assert_eq!(grid.workers.bottom(), bounds.bottom());
            assert!(grid.preview.width() > grid.progress.width());
            assert!((grid.progress.left() - grid.preview.right() - GAP).abs() < 0.01);
            assert!((grid.job.top() - grid.progress.bottom() - GAP).abs() < 0.01);
            assert!((grid.workers.top() - grid.preview.bottom() - GAP).abs() < 0.01);
            let tiles = [grid.job, grid.progress, grid.preview, grid.workers];
            for (i, tile) in tiles.iter().enumerate() {
                assert!(bounds.contains_rect(*tile));
                for other in tiles.iter().skip(i + 1) {
                    assert!(!tile.intersects(*other));
                }
            }
        }
    }

    #[test]
    fn padded_tile_keeps_the_parent_cursor_on_the_outer_grid() {
        let context = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 1000.0))), ..Default::default() };
        context
            .run_ui(input, |ui| {
                ui.spacing_mut().item_spacing.y = SPACE;
                let start = ui.cursor().top();
                flow_tile(ui, 224.0, "first", |ui| {
                    ui.label("Short content");
                });
                ui.add_space(SPACE);
                assert!((ui.cursor().top() - start - 224.0 - GAP).abs() < 0.01);
                flow_tile(ui, 224.0, "second", |ui| {
                    ui.label("Second card");
                });
            })
            .drop_without_applying_deltas();
    }
}
