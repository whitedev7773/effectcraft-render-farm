use super::*;

pub(crate) const GAP: f32 = 16.0;
pub(crate) const SPACE: f32 = 8.0;
pub(crate) const PADDING: i8 = 16;
pub(crate) const CONTROL: f32 = 32.0;
pub(crate) const PRIMARY_CONTROL: f32 = 36.0;
pub(crate) const COMPACT_CONTROL: f32 = 28.0;
pub(crate) const RADIUS: u8 = 12;

#[derive(Clone, Copy, Debug)]
pub(super) struct DashboardGrid {
    pub job: egui::Rect,
    pub progress: egui::Rect,
    pub preview: egui::Rect,
    pub activity: egui::Rect,
    pub workers: egui::Rect,
}

impl DashboardGrid {
    pub fn new(rect: egui::Rect) -> Self {
        let column = (rect.width() - GAP * 2.0) / 3.0;
        let top = ((rect.height() * 0.28).clamp(224.0, 256.0) / SPACE).round() * SPACE;
        let lower_top = rect.top() + top + GAP;
        let preview_height = (column * 9.0 / 16.0 + 96.0).clamp(240.0, (rect.bottom() - lower_top - GAP - 144.0).max(240.0));
        let job = egui::Rect::from_min_size(rect.min, egui::vec2(column, top));
        let progress = egui::Rect::from_min_max(egui::pos2(job.right() + GAP, rect.top()), egui::pos2(rect.right(), job.bottom()));
        let preview = egui::Rect::from_min_size(egui::pos2(rect.left(), lower_top), egui::vec2(column, preview_height));
        let activity = egui::Rect::from_min_max(egui::pos2(rect.left(), preview.bottom() + GAP), egui::pos2(job.right(), rect.bottom()));
        let workers = egui::Rect::from_min_max(egui::pos2(progress.left(), lower_top), rect.max);
        Self { job, progress, preview, activity, workers }
    }
}

/// Distribute the final row too, so every row uses its entire available width.
pub(super) fn grid_cells(rect: egui::Rect, count: usize, max_columns: usize, min_height: f32) -> Vec<egui::Rect> {
    if count == 0 {
        return Vec::new();
    }
    let columns = max_columns.max(1).min(count);
    let rows = count.div_ceil(columns);
    let height = ((rect.height() - GAP * (rows - 1) as f32) / rows as f32).max(min_height);
    let mut cells = Vec::with_capacity(count);
    let mut remaining = count;
    for row in 0..rows {
        let in_row = remaining.div_ceil(rows - row).min(columns);
        let width = (rect.width() - GAP * (in_row - 1) as f32) / in_row as f32;
        for column in 0..in_row {
            let min = rect.min + egui::vec2(column as f32 * (width + GAP), row as f32 * (height + GAP));
            cells.push(egui::Rect::from_min_size(min, egui::vec2(width, height)));
        }
        remaining -= in_row;
    }
    cells
}

pub(super) fn tile<R>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    secondary: bool,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.painter().rect_filled(rect, RADIUS, if secondary { SURFACE_SECONDARY } else { SURFACE });
    ui.painter().rect_stroke(rect, RADIUS, egui::Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
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

pub(super) fn value_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        caption(ui, label);
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
        for (width, height) in [(1120.0, 704.0), (1408.0, 784.0), (1888.0, 912.0), (2528.0, 1272.0)] {
            let bounds = egui::Rect::from_min_size(egui::pos2(16.0, 112.0), egui::vec2(width, height));
            let grid = DashboardGrid::new(bounds);
            assert_eq!(grid.job.left(), grid.preview.left());
            assert_eq!(grid.job.right(), grid.activity.right());
            assert_eq!(grid.progress.left(), grid.workers.left());
            assert_eq!(grid.job.bottom(), grid.progress.bottom());
            assert_eq!(grid.preview.top(), grid.workers.top());
            assert_eq!(grid.activity.bottom(), bounds.bottom());
            assert_eq!(grid.workers.bottom(), bounds.bottom());
            assert!((grid.progress.left() - grid.job.right() - GAP).abs() < 0.01);
            assert!((grid.activity.top() - grid.preview.bottom() - GAP).abs() < 0.01);
            let tiles = [grid.job, grid.progress, grid.preview, grid.activity, grid.workers];
            for (i, tile) in tiles.iter().enumerate() {
                assert!(bounds.contains_rect(*tile));
                for other in tiles.iter().skip(i + 1) {
                    assert!(!tile.intersects(*other));
                }
            }
        }
    }

    #[test]
    fn pc_grid_fills_every_row_for_uneven_counts() {
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 640.0));
        for count in 1..=12 {
            let cells = grid_cells(bounds, count, 3, 180.0);
            assert_eq!(cells.len(), count);
            for row in cells.chunk_by(|a, b| (a.top() - b.top()).abs() < 0.01) {
                assert_eq!(row[0].left(), bounds.left());
                assert!((row.last().unwrap().right() - bounds.right()).abs() < 0.01);
                for pair in row.windows(2) {
                    assert!((pair[1].left() - pair[0].right() - GAP).abs() < 0.01);
                    assert!((pair[0].height() - pair[1].height()).abs() < 0.01);
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
