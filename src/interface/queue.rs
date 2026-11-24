use super::*;
use crate::queue::{Status, Task};

fn status_color(ui: &egui::Ui, status: Status) -> egui::Color32 {
    match status {
        Status::Completed => success(ui),
        Status::Failed => danger(ui),
        Status::Preparing | Status::Waiting | Status::Rendering | Status::Encoding => accent(ui),
        _ => muted(ui),
    }
}

fn queue_filter_label(value: u8) -> &'static str {
    match value {
        1 => "대기 작업",
        2 => "진행 중",
        3 => "완료",
        4 => "확인 필요",
        _ => "모든 상태",
    }
}

fn queue_matches(task: &Task, query: &str, filter: u8) -> bool {
    let status_matches = match filter {
        1 => task.status == Status::Pending,
        2 => task.status.in_progress(),
        3 => task.status == Status::Completed,
        4 => matches!(task.status, Status::Failed | Status::Interrupted),
        _ => true,
    };
    let query = query.trim().to_lowercase();
    status_matches
        && (query.is_empty()
            || task.composition.name.to_lowercase().contains(&query)
            || task.project.to_lowercase().contains(&query)
            || task.final_video.to_lowercase().contains(&query))
}

fn queue_text(ui: &mut egui::Ui, rect: egui::Rect, text: &str, size: f32, color: egui::Color32) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).truncate().halign(egui::Align::Min));
}

fn queue_checkbox(ui: &mut egui::Ui, rect: egui::Rect, id: &str, checked: bool) -> egui::Response {
    let rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0));
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), checked, "선택"));
    ui.painter().rect_filled(rect, 4, if checked { layout::PRIMARY_FILL } else { secondary(ui) });
    ui.painter().rect_stroke(rect, 4, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
    if checked {
        let center = rect.center();
        let points = vec![center + egui::vec2(-4.0, 0.0), center + egui::vec2(-1.0, 3.0), center + egui::vec2(4.0, -3.0)];
        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.5, layout::ON_PRIMARY)));
    }
    response
}

fn queue_path(ui: &mut egui::Ui, rect: egui::Rect, id: &str, path: &str) {
    let width = ui.painter().layout_no_wrap(path.to_owned(), egui::FontId::proportional(12.0), accent(ui)).size().x;
    let label_width = width.min((rect.width() - 16.0).max(0.0));
    let label = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + label_width, rect.bottom()));
    queue_text(ui, label, path, 12.0, accent(ui));
    let icon = egui::Rect::from_center_size(egui::pos2(label.right() + 10.0, rect.center().y), egui::vec2(12.0, 12.0));
    if let Some(image) = figma_svg(ui, "queue-external-link", include_bytes!("../../assets/ui/figma/queue-external-link.svg"), icon.size()) {
        image.paint_at(ui, icon);
    }
    let response = ui.interact(rect, ui.id().with(id), egui::Sense::click()).on_hover_text(path).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked()
        && let Some(parent) = std::path::Path::new(path).parent()
    {
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer").arg(parent).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(parent).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

struct QueueColumns {
    handle: egui::Rect,
    check: egui::Rect,
    project: egui::Rect,
    project_path: egui::Rect,
    output: egui::Rect,
    status: egui::Rect,
}

impl QueueColumns {
    fn new(rect: egui::Rect) -> Self {
        let inner = rect.shrink2(egui::vec2(16.0, 0.0));
        let handle = egui::Rect::from_min_size(inner.min, egui::vec2(12.0, inner.height()));
        let check = egui::Rect::from_min_size(egui::pos2(handle.right() + 8.0, inner.top()), egui::vec2(16.0, inner.height()));
        let start = check.right() + 16.0;
        let width = (inner.right() - start).max(0.0);
        let edge = |fraction: f32| start + width * fraction;
        let column = |left: f32, right: f32| egui::Rect::from_min_max(egui::pos2(edge(left), inner.top()), egui::pos2(edge(right) - 12.0, inner.bottom()));
        let project = column(0.0, 0.32);
        let project_path = column(0.32, 0.54);
        let output = column(0.54, 0.78);
        let status = egui::Rect::from_min_max(egui::pos2(edge(0.78), inner.top()), inner.max);
        Self { handle, check, project, project_path, output, status }
    }
}

impl App {
    fn open_queue_task(&mut self, id: u64) {
        self.queue_detail = Some(id);
        self.queue_edit = self.settings.queue.iter().find(|task| task.id == id).cloned();
    }

    pub(super) fn queue_panel(&mut self, ui: &mut egui::Ui, height: f32) {
        let pending = self.settings.queue.iter().filter(|task| task.status == Status::Pending).count();
        let heading = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(egui::RichText::new("렌더링 목록").heading());
            caption(ui, format!("현재 {pending}개의 컴포지션이 대기중입니다."));
        };
        if ui.available_width() >= 650.0 {
            ui.horizontal(|ui| {
                ui.vertical(heading);
                self.queue_actions(ui);
            });
        } else {
            ui.scope(heading);
            self.queue_actions(ui);
        }
        ui.add_space(layout::SPACE);
        if self.settings.queue.is_empty() {
            self.empty_queue(ui, height);
            return;
        }
        let query_id = egui::Id::new("queue-search");
        let filter_id = egui::Id::new("queue-status-filter");
        let mut query = ui.ctx().data(|data| data.get_temp::<String>(query_id).unwrap_or_default());
        let mut filter = ui.ctx().data(|data| data.get_temp::<u8>(filter_id).unwrap_or_default());
        let compact = ui.available_width() < 680.0;
        if compact {
            ui.add_sized([ui.available_width(), layout::CONTROL], egui::TextEdit::singleline(&mut query).hint_text("프로젝트, 컴포지션, 출력 파일 검색…"));
        }
        ui.horizontal(|ui| {
            if !compact {
                let width = (ui.available_width() - 300.0).clamp(180.0, 360.0);
                ui.add_sized([width, layout::CONTROL], egui::TextEdit::singleline(&mut query).hint_text("프로젝트, 컴포지션, 출력 파일 검색…"));
            }
            egui::ComboBox::from_id_salt("queue-status").width(124.0).selected_text(queue_filter_label(filter)).show_ui(ui, |ui| {
                for value in 0..=4 {
                    ui.selectable_value(&mut filter, value, queue_filter_label(value));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(view) = components::segmented(ui, "queue-view", usize::from(self.settings.queue_cards), &["목록", "카드"]) {
                    self.settings.queue_cards = view == 1;
                }
            });
        });
        let selection_id = egui::Id::new("queue-selected-tasks");
        let selected = ui.ctx().data(|data| data.get_temp::<Vec<u64>>(selection_id).unwrap_or_default());
        let removable = selected
            .iter()
            .filter(|id| self.settings.queue.iter().any(|task| task.id == **id && !task.status.in_progress() && self.queue_active != Some(task.id)))
            .count();
        if removable > 0 && ui.button(format!("선택 항목 {removable}개 제거")).on_hover_text("큐에서만 제거하며 출력 파일은 삭제하지 않습니다.").clicked()
        {
            self.settings.queue.retain(|task| !selected.contains(&task.id) || task.status.in_progress() || self.queue_active == Some(task.id));
            self.queue_previews.retain(|id, _| self.settings.queue.iter().any(|task| task.id == *id));
            ui.ctx().data_mut(|data| data.insert_temp(selection_id, Vec::<u64>::new()));
        }
        let tasks: Vec<_> =
            self.settings.queue.iter().enumerate().filter(|(_, task)| queue_matches(task, &query, filter)).map(|(index, task)| (index, task.clone())).collect();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            caption(ui, format!("{} / {}개 표시", tasks.len(), self.settings.queue.len()));
            if (!query.is_empty() || filter != 0) && ui.small_button("필터 초기화").clicked() {
                query.clear();
                filter = 0;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| caption(ui, "등록 순서대로 실행"));
        });
        ui.ctx().data_mut(|data| {
            data.insert_temp(query_id, query);
            data.insert_temp(filter_id, filter);
        });
        ui.add_space(layout::SPACE);
        if tasks.is_empty() {
            egui::Frame::new().fill(surface(ui)).stroke(egui::Stroke::new(1.0, border(ui))).corner_radius(layout::RADIUS).inner_margin(32).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(32.0);
                    ui.label(egui::RichText::new("조건에 맞는 작업이 없습니다").size(18.0).strong());
                    caption(ui, "검색어 또는 상태 필터를 바꿔 보세요.");
                    if ui.button("전체 작업 보기").clicked() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(query_id, String::new());
                            data.insert_temp(filter_id, 0_u8);
                        });
                    }
                    ui.add_space(32.0);
                });
            });
        } else if self.settings.queue_cards {
            let columns = ((ui.available_width() + layout::GAP) / 350.0).floor().clamp(1.0, 4.0) as usize;
            let rows = tasks.len().div_ceil(columns);
            let gap = layout::GAP;
            let cell_width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
            let preview_height = (cell_width - f32::from(layout::PADDING) * 2.0) * 9.0 / 16.0;
            let preview_height = if compact { preview_height.min(192.0) } else { preview_height };
            let item_height = preview_height + 178.0;
            let grid_height = rows as f32 * item_height + rows.saturating_sub(1) as f32 * gap;
            let (bounds, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), grid_height), egui::Sense::hover());
            for (display_index, (index, task)) in tasks.iter().enumerate() {
                let rect = egui::Rect::from_min_size(
                    bounds.min + egui::vec2((display_index % columns) as f32 * (cell_width + gap), (display_index / columns) as f32 * (item_height + gap)),
                    egui::vec2(cell_width, item_height),
                );
                ui.push_id(("queue-item", task.id), |ui| self.queue_item(ui, task, *index, true, rect));
            }
        } else if ui.available_width() >= 900.0 {
            self.queue_table(ui, &tasks, (height - 160.0).max(200.0));
        } else {
            let item_height = 106.0;
            for (index, task) in &tasks {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), item_height), egui::Sense::hover());
                ui.push_id(("queue-item", task.id), |ui| self.queue_item(ui, task, *index, false, rect));
            }
        }
    }

    fn queue_actions(&mut self, ui: &mut egui::Ui) {
        let size = egui::vec2(ui.available_width(), layout::PRIMARY_CONTROL);
        ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let dispatch = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
            let can_start =
                self.running && !dispatch && !self.stopping && !self.connected_render_workers().is_empty() && self.engine_updater.required.is_none();
            let start_label = egui::RichText::new("렌더링 시작").font(figma_font(14.0, false)).extra_letter_spacing(-0.4).color(layout::ON_PRIMARY);
            let start_button = match figma_svg(ui, "queue-play", include_bytes!("../../assets/ui/figma/queue-play.svg"), egui::Vec2::splat(16.0)) {
                Some(icon) => egui::Button::image_and_text(icon.tint(layout::ON_PRIMARY), start_label),
                None => egui::Button::new(start_label),
            }
            .fill(layout::PRIMARY_FILL)
            .stroke(egui::Stroke::NONE)
            .corner_radius(layout::CONTROL_RADIUS)
            .min_size(egui::vec2(0.0, layout::PRIMARY_CONTROL));
            if ui.add_enabled(can_start, start_button).on_hover_text("호스트를 열고 사용할 PC를 연결한 뒤 렌더링을 시작합니다.").clicked()
            {
                self.begin_current_render();
            }
            let add_label = egui::RichText::new("컴포지션 추가").font(figma_font(14.0, false)).extra_letter_spacing(-0.4);
            let add_button = match figma_svg(ui, "queue-add", include_bytes!("../../assets/ui/figma/queue-add.svg"), egui::Vec2::splat(16.0)) {
                Some(icon) => egui::Button::image_and_text(icon, add_label),
                None => egui::Button::new(add_label),
            }
            .corner_radius(layout::CONTROL_RADIUS)
            .min_size(egui::vec2(0.0, layout::PRIMARY_CONTROL));
            if ui.add_enabled(!self.running && self.pending_start.is_none(), add_button).clicked() {
                self.choose_project_for_queue();
            }
        });
    }

    fn queue_table(&mut self, ui: &mut egui::Ui, tasks: &[(usize, Task)], min_height: f32) {
        let header_height = 40.0;
        let row_height = 56.0;
        let height = (header_height + tasks.len() as f32 * row_height).max(min_height);
        let (bounds, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        ui.painter().rect_filled(bounds, layout::RADIUS, surface(ui));
        let header = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), header_height));
        ui.painter().rect_filled(header, egui::CornerRadius { nw: layout::RADIUS, ne: layout::RADIUS, sw: 0, se: 0 }, secondary(ui));
        let columns = QueueColumns::new(header);
        for (rect, label) in
            [(columns.project, "컴포지션"), (columns.project_path, "프로젝트 경로"), (columns.output, "출력물 저장 위치"), (columns.status, "상태")]
        {
            queue_text(ui, rect, label, 12.0, muted(ui));
        }
        // The selection controls are kept in transient UI state, so they never change render order.
        let selection_id = egui::Id::new("queue-selected-tasks");
        let mut selected = ui.ctx().data(|data| data.get_temp::<Vec<u64>>(selection_id).unwrap_or_default());
        selected.retain(|id| self.settings.queue.iter().any(|task| task.id == *id));
        let all_selected = !tasks.is_empty() && tasks.iter().all(|(_, task)| selected.contains(&task.id));
        if queue_checkbox(ui, columns.check, "queue-select-all", all_selected).clicked() {
            if all_selected {
                selected.retain(|id| !tasks.iter().any(|(_, task)| task.id == *id));
            } else {
                for (_, task) in tasks {
                    if !selected.contains(&task.id) {
                        selected.push(task.id);
                    }
                }
            }
        }
        for (row, (index, task)) in tasks.iter().enumerate() {
            let rect = egui::Rect::from_min_size(bounds.min + egui::vec2(0.0, header_height + row as f32 * row_height), egui::vec2(bounds.width(), row_height));
            ui.push_id(("queue-table-row", task.id), |ui| self.queue_table_row(ui, task, *index, rect, &mut selected));
        }
        ui.ctx().data_mut(|data| data.insert_temp(selection_id, selected));
        ui.painter().rect_stroke(bounds, layout::RADIUS, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
    }

    fn queue_table_row(&mut self, ui: &mut egui::Ui, task: &Task, index: usize, rect: egui::Rect, selected: &mut Vec<u64>) {
        let columns = QueueColumns::new(rect);
        let active = self.queue_active == Some(task.id);
        let hovered = ui.rect_contains_pointer(rect.intersect(ui.clip_rect()));
        let hover = motion::highlight(ui, ui.id().with(("queue-row-hover", task.id)), hovered);
        ui.painter().rect_filled(rect, 0, secondary(ui).gamma_multiply(hover));
        ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, border(ui)));
        // The two-column dot grip is a real drag handle for pending entries.
        let grip = ui.interact(columns.handle, ui.id().with("queue-drag"), egui::Sense::drag());
        for x in [0.0, 3.0] {
            for y in [-3.0, 0.0, 3.0] {
                ui.painter().circle_filled(columns.handle.center() + egui::vec2(x - 1.5, y), 0.8, muted(ui).gamma_multiply(0.55));
            }
        }
        let drag_id = ui.id().with("queue-drag-distance");
        if grip.dragged() {
            let delta = grip.drag_delta().y;
            ui.ctx().data_mut(|data| {
                let distance = data.get_temp::<f32>(drag_id).unwrap_or_default() + delta;
                data.insert_temp(drag_id, distance);
            });
        }
        if grip.drag_stopped() && task.status == Status::Pending && !active {
            let direction = ui.ctx().data_mut(|data| data.remove_temp::<f32>(drag_id).unwrap_or_default());
            if direction.abs() >= 20.0 {
                let next = if direction < 0.0 { index.checked_sub(1) } else { Some(index + 1).filter(|next| *next < self.settings.queue.len()) };
                if let Some(next) = next.filter(|next| !self.settings.queue[*next].status.in_progress()) {
                    self.settings.queue.swap(index, next);
                }
            }
        }
        if queue_checkbox(ui, columns.check, "queue-select", selected.contains(&task.id)).clicked() {
            if selected.contains(&task.id) {
                selected.retain(|id| *id != task.id);
            } else {
                selected.push(task.id);
            }
        }
        let name = std::path::Path::new(&task.project).file_name().unwrap_or_default().to_string_lossy();
        let title = egui::Rect::from_min_max(columns.project.min, egui::pos2(columns.project.right(), rect.center().y));
        let subtitle = egui::Rect::from_min_max(egui::pos2(columns.project.left(), rect.center().y), columns.project.max);
        queue_text(ui, title, &task.composition.name, 12.0, ui.visuals().text_color());
        queue_text(ui, subtitle, &name, 12.0, muted(ui));
        let response = ui.interact(columns.project, ui.id().with("queue-project"), egui::Sense::hover());
        response.on_hover_text(format!("{}\n{}", task.project, task.detail));
        queue_path(ui, columns.project_path, "project-path", &task.project);
        queue_path(ui, columns.output, "output-path", &task.final_video);
        let total = (task.range_end - task.range_start).max(1);
        let fraction = (task.done as f32 / total as f32).clamp(0.0, 1.0);
        if task.status.in_progress() {
            let progress =
                egui::Rect::from_min_size(egui::pos2(columns.status.left(), rect.center().y - 2.0), egui::vec2((columns.status.width() - 48.0).max(0.0), 4.0));
            Self::queue_progress(ui, progress, task.id, fraction, status_color(ui, task.status));
            let label = egui::Rect::from_min_max(egui::pos2(progress.right() + 8.0, rect.top()), columns.status.max);
            queue_text(ui, label, &format!("{:.0}%", fraction * 100.0), 12.0, muted(ui));
        } else {
            let label = match task.status {
                Status::Pending => "대기중",
                Status::Interrupted => "작업 중단",
                Status::Failed => "오류 발생",
                _ => task.status.label(),
            };
            let color = if task.status == Status::Pending { ui.visuals().text_color() } else { status_color(ui, task.status) };
            queue_text(ui, columns.status, label, 14.0, color);
        }
        if hovered {
            let footer = egui::Rect::from_min_size(egui::pos2(rect.right() - 112.0, rect.center().y - 16.0), egui::vec2(112.0, layout::CONTROL));
            ui.painter().rect_filled(footer, layout::CONTROL_RADIUS, secondary(ui));
            self.queue_controls(ui, task, footer);
        }
        let gear = egui::Rect::from_min_size(egui::pos2(rect.right() - layout::CONTROL - 16.0, rect.center().y - 16.0), egui::Vec2::splat(layout::CONTROL));
        self.queue_settings_gear(ui, task, rect, gear);
    }

    fn empty_queue(&mut self, ui: &mut egui::Ui, height: f32) {
        let body_height = (height - 100.0).clamp(260.0, 460.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), body_height), egui::Sense::hover());
        ui.painter().rect_filled(rect, layout::RADIUS, surface(ui));
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 40.0));
        ui.painter().rect_filled(header, egui::CornerRadius { nw: layout::RADIUS, ne: layout::RADIUS, sw: 0, se: 0 }, secondary(ui));
        let columns = QueueColumns::new(header);
        for (column, label) in
            [(columns.project, "컴포지션"), (columns.project_path, "프로젝트 경로"), (columns.output, "출력물 저장 위치"), (columns.status, "상태")]
        {
            queue_text(ui, column, label, 12.0, muted(ui));
        }
        ui.add_enabled_ui(false, |ui| {
            queue_checkbox(ui, columns.check, "empty-select-all", false);
        });
        ui.painter().rect_stroke(rect, layout::RADIUS, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
        let body = egui::Rect::from_min_max(egui::pos2(rect.left() + 16.0, header.bottom()), rect.max - egui::vec2(16.0, 0.0));
        let mut content = ui.new_child(egui::UiBuilder::new().id_salt("empty-queue").max_rect(body).layout(egui::Layout::top_down(egui::Align::Center)));
        content.set_clip_rect(body.intersect(ui.clip_rect()));
        content.add_space(((body.height() - 80.0) / 2.0).max(0.0));
        let (icon, _) = content.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
        if let Some(image) = figma_svg(&content, "queue-empty-shapes", include_bytes!("../../assets/ui/figma/queue-empty-shapes.svg"), icon.size()) {
            image.paint_at(&content, icon);
        }
        content.add_space(8.0);
        content.label(egui::RichText::new("대기중인 컴포지션 없음").font(figma_font(16.0, false)));
        content.label(egui::RichText::new("우측 버튼으로 렌더링할 컴포지션을 추가합니다.").size(12.0).color(muted(&content)));
    }

    fn queue_item(&mut self, ui: &mut egui::Ui, task: &Task, index: usize, cards: bool, rect: egui::Rect) {
        let active = self.queue_active == Some(task.id);
        let color = status_color(ui, task.status);
        ui.painter().rect_filled(rect, layout::RADIUS, surface(ui));
        ui.painter().rect_stroke(
            rect,
            layout::RADIUS,
            egui::Stroke::new(1.0, if active { accent(ui).gamma_multiply(0.35) } else { border(ui) }),
            egui::StrokeKind::Inside,
        );
        if active {
            let marker = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 14.0), egui::vec2(3.0, rect.height() - 28.0));
            ui.painter().rect_filled(marker, 2, accent(ui));
        }
        let inner = rect.shrink(layout::PADDING as f32);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(inner.intersect(ui.clip_rect()));
        let total = (task.range_end - task.range_start).max(1);
        let fraction = (task.done as f32 / total as f32).clamp(0.0, 1.0);
        let name = std::path::Path::new(&task.project).file_name().unwrap_or_default().to_string_lossy();
        if cards {
            let image_height = inner.height() - 146.0;
            let preview = self.queue_previews.get(&task.id).cloned();
            self.dashboard_image(&mut child, u64::MAX / 2 + task.id, preview.as_ref(), image_height.max(80.0));
            let number = egui::Rect::from_min_size(inner.min + egui::vec2(8.0, 8.0), egui::vec2(32.0, 22.0));
            child.painter().rect_filled(number, 4, egui::Color32::from_black_alpha(170));
            child.painter().text(
                number.center(),
                egui::Align2::CENTER_CENTER,
                format!("{:02}", index + 1),
                egui::FontId::monospace(11.0),
                egui::Color32::WHITE,
            );
            if let Some(preview) = preview {
                let label = format!("FRAME {}", preview.frame);
                let frame = egui::Rect::from_min_size(egui::pos2(inner.right() - 100.0, inner.top() + image_height - 30.0), egui::vec2(92.0, 22.0));
                child.painter().rect_filled(frame, 4, egui::Color32::from_black_alpha(170));
                child.painter().text(frame.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::monospace(10.0), egui::Color32::WHITE);
            }
            child.add_space(4.0);
            let (header, _) = child.allocate_exact_size(egui::vec2(child.available_width(), 28.0), egui::Sense::hover());
            self.queue_title(&mut child, task, index, header, color);
            child.add(egui::Label::new(egui::RichText::new(&*name).size(12.0).color(muted(&child))).truncate()).on_hover_text(&task.project);
            let progress = egui::Rect::from_min_size(egui::pos2(inner.left(), inner.bottom() - 60.0), egui::vec2(inner.width(), 4.0));
            Self::queue_progress(&child, progress, task.id, fraction, color);
            child.painter().text(
                egui::pos2(inner.left(), progress.top() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{} / {total} 프레임", task.done),
                egui::FontId::proportional(11.0),
                muted(&child),
            );
            let footer = egui::Rect::from_min_size(egui::pos2(inner.left(), inner.bottom() - layout::CONTROL), egui::vec2(inner.width(), layout::CONTROL));
            self.queue_controls(&mut child, task, footer);
            let gear = egui::Rect::from_min_size(egui::pos2(inner.right() - layout::CONTROL - 8.0, inner.top() + 8.0), egui::Vec2::splat(layout::CONTROL));
            self.queue_settings_gear(ui, task, rect, gear);
        } else {
            let ordinal_width = 32.0;
            let content_left = inner.left() + ordinal_width + 8.0;
            child.painter().text(
                egui::pos2(inner.left(), inner.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{:02}", index + 1),
                egui::FontId::monospace(12.0),
                if active { accent(&child) } else { muted(&child) },
            );
            let header = egui::Rect::from_min_max(egui::pos2(content_left, inner.top()), egui::pos2(inner.right(), inner.top() + 26.0));
            self.queue_title(&mut child, task, index, header, color);
            let meta = format!("{name}  ·  {} × {}  ·  {}–{}f", task.composition.width, task.composition.height, task.range_start, task.range_end);
            let meta_rect = egui::Rect::from_min_max(egui::pos2(content_left, inner.top() + 29.0), egui::pos2(inner.right(), inner.top() + 46.0));
            let mut meta_ui = child.new_child(egui::UiBuilder::new().max_rect(meta_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
            meta_ui.set_clip_rect(meta_rect.intersect(child.clip_rect()));
            meta_ui.add(egui::Label::new(egui::RichText::new(&meta).size(11.0).color(muted(&child))).truncate()).on_hover_text(&task.project);
            let controls_width = 112.0;
            let footer = egui::Rect::from_min_size(
                egui::pos2(inner.right() - controls_width, inner.bottom() - layout::CONTROL),
                egui::vec2(controls_width, layout::CONTROL),
            );
            self.queue_controls(&mut child, task, footer);
            self.queue_settings_gear(
                ui,
                task,
                rect,
                egui::Rect::from_min_size(egui::pos2(footer.right() - layout::CONTROL, footer.top()), egui::Vec2::splat(layout::CONTROL)),
            );
            let progress_rect =
                egui::Rect::from_min_max(egui::pos2(content_left, inner.bottom() - 8.0), egui::pos2(footer.left() - 16.0, inner.bottom() - 4.0));
            Self::queue_progress(&child, progress_rect, task.id, fraction, color);
            child.painter().text(
                progress_rect.left_top() - egui::vec2(0.0, 6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.0}%  ·  {} / {total} 프레임", fraction * 100.0, task.done),
                egui::FontId::proportional(11.0),
                muted(&child),
            );
        }
    }

    fn queue_title(&mut self, ui: &mut egui::Ui, task: &Task, index: usize, rect: egui::Rect, color: egui::Color32) {
        let status_width = ui.painter().layout_no_wrap(task.status.label().into(), egui::FontId::proportional(11.0), color).size().x + 20.0;
        let status = egui::Rect::from_min_size(egui::pos2(rect.right() - status_width, rect.center().y - 11.0), egui::vec2(status_width, 22.0));
        ui.painter().rect_filled(status, layout::CONTROL_RADIUS, soft(color));
        ui.painter().text(status.center(), egui::Align2::CENTER_CENTER, task.status.label(), egui::FontId::proportional(11.0), color);
        let title_rect = egui::Rect::from_min_max(rect.min, egui::pos2(status.left() - 12.0, rect.bottom()));
        let response = ui.interact(title_rect, ui.id().with("queue-title"), egui::Sense::hover());
        let mut title = ui.new_child(egui::UiBuilder::new().max_rect(title_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        title.set_clip_rect(title_rect.intersect(ui.clip_rect()));
        title.add(egui::Label::new(egui::RichText::new(&task.composition.name).size(14.0).strong()).truncate());
        response.on_hover_text(format!("{:02} · {}\n{}", index + 1, task.composition.name, task.detail));
    }

    fn queue_settings_gear(&mut self, ui: &mut egui::Ui, task: &Task, item: egui::Rect, rect: egui::Rect) {
        let visible_item = item.intersect(ui.clip_rect());
        let hovered = ui.rect_contains_pointer(visible_item);
        let hitbox = rect.intersect(ui.clip_rect());
        if !hitbox.is_positive() {
            return;
        }
        let id = egui::Id::new(("queue-settings", task.id));
        let response = ui.interact(hitbox, id, egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), format!("렌더 설정 · {}", task.composition.name)));
        let icon = icon_image(ui, UiIcon::Settings);
        let visible = hovered || response.has_focus();
        let opacity = motion::highlight(ui, id.with("reveal"), visible);
        if opacity > 0.0 {
            let mut layer = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
            layer.set_clip_rect(hitbox);
            layer.multiply_opacity(opacity);
            let painter = layer.painter();
            painter.rect_filled(rect, layout::CONTROL_RADIUS, surface(ui));
            painter.rect_stroke(
                rect,
                layout::CONTROL_RADIUS,
                egui::Stroke::new(1.0, if response.hovered() || response.has_focus() { accent(ui) } else { border(ui) }),
                egui::StrokeKind::Inside,
            );
            icon.paint_at(&layer, rect.shrink(7.0));
        }
        if visible && response.clicked() {
            self.open_queue_task(task.id);
        }
        response.on_hover_text("렌더 설정").on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    fn queue_progress(ui: &egui::Ui, rect: egui::Rect, task_id: u64, fraction: f32, color: egui::Color32) {
        let fraction = motion::progress(ui, task_id, fraction);
        ui.painter().rect_filled(rect, 2, border(ui));
        if fraction > 0.0 {
            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height())), 2, color);
        }
    }

    fn queue_controls(&mut self, ui: &mut egui::Ui, task: &Task, footer: egui::Rect) {
        let movable = task.status == Status::Pending && self.queue_active != Some(task.id);
        let index = self.settings.queue.iter().position(|entry| entry.id == task.id).unwrap_or(0);
        let up = egui::Rect::from_min_size(footer.min, egui::Vec2::splat(layout::CONTROL));
        let down = up.translate(egui::vec2(layout::CONTROL + 4.0, 0.0));
        if button_at(ui, up, "queue-up", ButtonSpec::new("↑", ButtonKind::Quiet, movable && index > 0 && !self.settings.queue[index - 1].status.in_progress()))
            .on_hover_text("순서 올리기")
            .clicked()
        {
            self.settings.queue.swap(index, index - 1);
        }
        let index = self.settings.queue.iter().position(|entry| entry.id == task.id).unwrap_or(0);
        if button_at(
            ui,
            down,
            "queue-down",
            ButtonSpec::new(
                "↓",
                ButtonKind::Quiet,
                movable && index + 1 < self.settings.queue.len() && !self.settings.queue.get(index + 1).is_some_and(|entry| entry.status.in_progress()),
            ),
        )
        .on_hover_text("순서 내리기")
        .clicked()
        {
            self.settings.queue.swap(index, index + 1);
        }
    }

    pub(super) fn queue_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.queue_detail else { return };
        let Some(live) = self.settings.queue.iter().find(|task| task.id == id).cloned() else {
            self.queue_detail = None;
            self.queue_edit = None;
            return;
        };
        let mut edit = self.queue_edit.clone().filter(|task| task.id == id).unwrap_or_else(|| live.clone());
        let editable = !live.status.in_progress() && live.status != Status::Completed && self.queue_active != Some(id);
        let mut save = false;
        let mut retry = false;
        let mut remove = false;
        let mut close = false;
        let mut saved = false;
        let (modal, reveal) = motion::opening_modal(ctx, egui::Id::new("queue-detail"), self.settings.reduced_motion, card(ctx));
        let response = modal.show(ctx, |ui| {
            ui.multiply_opacity(reveal);
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(280.0, 650.0));
            close = dialog_header(ui, &live.composition.name, "닫기");
            let color = status_color(ui, live.status);
            badge(ui, live.status.label(), color);
            egui::ScrollArea::vertical().id_salt("queue-detail-scroll").max_height((ctx.content_rect().height() - 240.0).max(140.0)).show(ui, |ui| {
                ui.add(egui::Label::new(&live.project).wrap());
                caption(
                    ui,
                    format!(
                        "{} × {} · {:.3} fps · {}프레임",
                        live.composition.width,
                        live.composition.height,
                        live.composition.rate.as_f64(),
                        live.composition.frames
                    ),
                );
                if let Some(preview) = self.queue_previews.get(&id).cloned() {
                    self.show_preview_sized(ui, u64::MAX / 2 + id, &preview, 240.0);
                }
                let total = (live.range_end - live.range_start).max(1);
                caption(ui, format!("{} / {total} 프레임", live.done));
                ui.add(
                    egui::ProgressBar::new(motion::progress(ui, id, live.done as f32 / total as f32))
                        .fill(accent(ui))
                        .desired_width(ui.available_width())
                        .desired_height(8.0),
                );
                if !live.detail.is_empty() {
                    ui.label(&live.detail);
                }
                ui.separator();
                ui.add_enabled_ui(editable, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("렌더 범위");
                        ui.add(egui::DragValue::new(&mut edit.range_start).range(0..=edit.range_end.saturating_sub(1)));
                        ui.label("→");
                        ui.add(egui::DragValue::new(&mut edit.range_end).range(edit.range_start + 1..=edit.composition.frames));
                    });
                    caption(ui, "끝 프레임 미포함");
                    range_bar(
                        ui,
                        &mut edit.range_start,
                        &mut edit.range_end,
                        edit.composition.frames,
                        (edit.composition.work_start, edit.composition.work_end),
                        &mut self.range_handle,
                    );
                    if path_row(ui, "최종 MP4", &mut edit.final_video)
                        && let Some(path) = rfd::FileDialog::new().add_filter("MP4", &["mp4"]).save_file()
                    {
                        edit.final_video = path.display().to_string();
                    }
                    caption(ui, "프레임 저장 폴더 · 프로젝트 및 렌더 작업별로 자동 관리");
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut edit.render.automatic, "자동 최적화");
                        ui.checkbox(&mut edit.render.draft, "Draft 품질");
                        ui.checkbox(&mut edit.render.audio, "오디오 포함");
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("비트레이트");
                        ui.add(egui::DragValue::new(&mut edit.render.video_bitrate).range(100..=200_000).suffix(" kbps"));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut edit.render.previews, "현재 프레임 미리보기");
                        ui.add_enabled(edit.render.previews, egui::DragValue::new(&mut edit.render.preview_seconds).range(1..=30).suffix("초 간격"));
                    });
                    ui.collapsing("작업 분배 · 고급 설정", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("구간 분배");
                            ui.selectable_value(&mut edit.distribution, Distribution::Auto, "자동");
                            ui.selectable_value(&mut edit.distribution, Distribution::Frames, "프레임");
                            ui.selectable_value(&mut edit.distribution, Distribution::Percent, "비율");
                        });
                        match edit.distribution {
                            Distribution::Frames => {
                                ui.add(egui::DragValue::new(&mut edit.chunk_frames).range(1..=100_000).suffix("프레임"));
                            }
                            Distribution::Percent => {
                                ui.add(egui::DragValue::new(&mut edit.chunk_percent).range(0.01..=100.0).suffix("%"));
                            }
                            Distribution::Auto => {}
                        }
                        ui.add_enabled(!edit.render.automatic, egui::DragValue::new(&mut edit.max_workers).range(1..=64).suffix("대 동시 사용"));
                        ui.add_enabled(!edit.render.automatic, egui::Checkbox::new(&mut edit.gpu, "GPU 렌더링"));
                        ui.checkbox(&mut edit.render.adaptive, "처리 속도에 따라 구간 나누기");
                        ui.add(egui::DragValue::new(&mut edit.render.target_seconds).range(5..=300).suffix("초 목표 구간"));
                        ui.add(egui::DragValue::new(&mut edit.render.retries).range(1..=10).suffix("회 최대 시도"));
                        ui.add(egui::DragValue::new(&mut edit.render.poll_ms).range(50..=2000).suffix("ms 프레임 감지"));
                        ui.checkbox(&mut edit.render.durable_writes, "수신 프레임마다 디스크 동기화");
                        ui.checkbox(&mut edit.render.nvenc, "하드웨어 인코딩 사용");
                        text_row(ui, "FFmpeg 경로 · 비우면 자동 감지", &mut edit.render.ffmpeg, false);
                    });
                });
                if !editable {
                    caption(ui, "진행 중이거나 완료된 작업의 설정은 잠겨 있습니다.");
                }
                if !self.last_error.is_empty() {
                    ui.colored_label(danger(ui), &self.last_error);
                }
            });
            ui.separator();
            let can_retry = editable && matches!(live.status, Status::Failed | Status::Interrupted);
            let leading =
                (!live.status.in_progress() && self.queue_active != Some(id)).then_some(ButtonSpec::new("큐에서 제거", ButtonKind::Destructive, true));
            let primary = editable.then_some(ButtonSpec::new(if can_retry { "저장 후 재대기" } else { "설정 저장" }, ButtonKind::Primary, true));
            let clicks =
                footer_buttons(ui, leading, primary, Some(ButtonSpec::new(if editable { "취소" } else { "닫기" }, ButtonKind::Normal, true)), layout::CONTROL);
            save = clicks.primary && !can_retry;
            retry = clicks.primary && can_retry;
            remove = clicks.leading;
            close |= clicks.dismiss;
        });
        if save || retry {
            match crate::queue::validate_unique(&edit, &self.settings.queue) {
                Ok(()) => {
                    edit.status = if retry { Status::Pending } else { live.status };
                    edit.done = if retry { 0 } else { live.done };
                    edit.detail = if retry { String::new() } else { live.detail };
                    if let Some(task) = self.settings.queue.iter_mut().find(|task| task.id == id) {
                        *task = edit.clone();
                    }
                    self.last_error.clear();
                    saved = true;
                }
                Err(error) => self.last_error = error,
            }
        }
        self.queue_edit = Some(edit);
        if remove {
            self.settings.queue.retain(|task| task.id != id);
            self.queue_previews.remove(&id);
        }
        if response.should_close() || close || remove || saved {
            self.queue_detail = None;
            self.queue_edit = None;
        }
    }
}
