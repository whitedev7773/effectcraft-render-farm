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

struct QueueColumns {
    order: egui::Rect,
    project: egui::Rect,
    status: egui::Rect,
    progress: egui::Rect,
    output: egui::Rect,
    actions: egui::Rect,
}

impl QueueColumns {
    fn new(rect: egui::Rect) -> Self {
        let inner = rect.shrink2(egui::vec2(16.0, 8.0));
        let actions = egui::Rect::from_min_max(egui::pos2(inner.right() - 112.0, inner.top()), inner.max);
        let output = egui::Rect::from_min_max(egui::pos2(actions.left() - 152.0, inner.top()), egui::pos2(actions.left() - 12.0, inner.bottom()));
        let progress = egui::Rect::from_min_max(egui::pos2(output.left() - 142.0, inner.top()), egui::pos2(output.left() - 12.0, inner.bottom()));
        let status = egui::Rect::from_min_max(egui::pos2(progress.left() - 104.0, inner.top()), egui::pos2(progress.left() - 12.0, inner.bottom()));
        let order = egui::Rect::from_min_size(inner.min, egui::vec2(40.0, inner.height()));
        let project = egui::Rect::from_min_max(egui::pos2(order.right() + 8.0, inner.top()), egui::pos2(status.left() - 12.0, inner.bottom()));
        Self { order, project, status, progress, output, actions }
    }
}

impl App {
    fn open_queue_task(&mut self, id: u64) {
        self.queue_detail = Some(id);
        self.queue_edit = self.settings.queue.iter().find(|task| task.id == id).cloned();
    }

    pub(super) fn queue_panel(&mut self, ui: &mut egui::Ui, height: f32) {
        let pending = self.settings.queue.iter().filter(|task| task.status == Status::Pending).count();
        let completed = self.settings.queue.iter().filter(|task| task.status == Status::Completed).count();
        ui.horizontal(|ui| {
            let title_width = (ui.available_width() - 152.0).max(1.0);
            ui.allocate_ui_with_layout(egui::vec2(title_width, layout::CONTROL), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                components::section_heading(ui, "작업 목록", &format!("전체 {}  ·  대기 {pending}  ·  완료 {completed}", self.settings.queue.len()));
            });
            if ui.add_enabled(!self.running && self.pending_start.is_none(), primary_button("+ 프로젝트 추가")).clicked() {
                self.choose_project_for_queue();
            }
        });
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
            let item_height = (cell_width - 32.0) * 9.0 / 16.0 + 178.0;
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
            self.queue_table(ui, &tasks);
        } else {
            let item_height = 106.0;
            for (index, task) in &tasks {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), item_height), egui::Sense::hover());
                ui.push_id(("queue-item", task.id), |ui| self.queue_item(ui, task, *index, false, rect));
            }
        }
    }

    fn queue_table(&mut self, ui: &mut egui::Ui, tasks: &[(usize, Task)]) {
        let (bounds, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0 + tasks.len() as f32 * 72.0), egui::Sense::hover());
        ui.painter().rect_filled(bounds, layout::RADIUS, surface(ui));
        let header = egui::Rect::from_min_size(bounds.min, egui::vec2(bounds.width(), 40.0));
        ui.painter().rect_filled(header, egui::CornerRadius { nw: layout::RADIUS, ne: layout::RADIUS, sw: 0, se: 0 }, secondary(ui));
        let columns = QueueColumns::new(header);
        for (rect, label) in [
            (columns.order, "순서"),
            (columns.project, "프로젝트 / 컴포지션"),
            (columns.status, "상태"),
            (columns.progress, "진행률"),
            (columns.output, "출력 파일"),
            (columns.actions, "작업"),
        ] {
            queue_text(ui, rect, label, 11.0, muted(ui));
        }
        for (row, (index, task)) in tasks.iter().enumerate() {
            let rect = egui::Rect::from_min_size(bounds.min + egui::vec2(0.0, 40.0 + row as f32 * 72.0), egui::vec2(bounds.width(), 72.0));
            ui.push_id(("queue-table-row", task.id), |ui| self.queue_table_row(ui, task, *index, rect));
        }
        ui.painter().rect_stroke(bounds, layout::RADIUS, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
    }

    fn queue_table_row(&mut self, ui: &mut egui::Ui, task: &Task, index: usize, rect: egui::Rect) {
        let columns = QueueColumns::new(rect);
        let active = self.queue_active == Some(task.id);
        let color = status_color(ui, task.status);
        let response = ui.interact(columns.project, ui.id().with("queue-project"), egui::Sense::hover());
        if active || response.hovered() {
            ui.painter().rect_filled(rect, 0, if active { accent(ui).gamma_multiply(0.07) } else { secondary(ui) });
        }
        ui.painter().hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, border(ui)));
        queue_text(ui, columns.order, &format!("{:02}", index + 1), 12.0, if active { accent(ui) } else { muted(ui) });
        let name = std::path::Path::new(&task.project).file_name().unwrap_or_default().to_string_lossy();
        let title = egui::Rect::from_min_max(columns.project.min, egui::pos2(columns.project.right(), rect.center().y + 1.0));
        queue_text(ui, title, &task.composition.name, 13.0, ui.visuals().text_color());
        let subtitle = egui::Rect::from_min_max(egui::pos2(columns.project.left(), rect.center().y + 1.0), columns.project.max);
        queue_text(ui, subtitle, &format!("{name}  ·  {} × {}", task.composition.width, task.composition.height), 11.0, muted(ui));
        response.on_hover_text(format!("{}\n{}", task.project, task.detail));
        let status = egui::Rect::from_min_size(egui::pos2(columns.status.left(), rect.center().y - 10.0), egui::vec2(columns.status.width(), 20.0));
        ui.painter().circle_filled(status.left_center() + egui::vec2(3.0, 0.0), 3.0, color);
        queue_text(ui, status.translate(egui::vec2(12.0, 0.0)), task.status.label(), 11.0, color);
        let total = (task.range_end - task.range_start).max(1);
        let fraction = (task.done as f32 / total as f32).clamp(0.0, 1.0);
        let progress_label = egui::Rect::from_min_max(columns.progress.min, egui::pos2(columns.progress.right(), rect.center().y + 4.0));
        queue_text(ui, progress_label, &format!("{:.0}% · {} / {total}", fraction * 100.0, task.done), 11.0, muted(ui));
        Self::queue_progress(
            ui,
            egui::Rect::from_min_size(egui::pos2(columns.progress.left(), rect.center().y + 10.0), egui::vec2(columns.progress.width() - 8.0, 4.0)),
            fraction,
            if task.status == Status::Completed { success(ui) } else { accent(ui) },
        );
        let output = std::path::Path::new(&task.final_video).file_name().unwrap_or_default().to_string_lossy();
        queue_text(ui, columns.output, &output, 12.0, muted(ui));
        let _ = ui.interact(columns.output, ui.id().with("output-path"), egui::Sense::hover()).on_hover_text(&task.final_video);
        let footer = egui::Rect::from_center_size(columns.actions.center(), egui::vec2(columns.actions.width(), layout::CONTROL));
        self.queue_controls(ui, task, footer);
        self.queue_settings_gear(
            ui,
            task,
            rect,
            egui::Rect::from_min_size(egui::pos2(footer.right() - layout::CONTROL, footer.top()), egui::Vec2::splat(layout::CONTROL)),
        );
    }

    fn empty_queue(&mut self, ui: &mut egui::Ui, height: f32) {
        let body_height = (height - 100.0).clamp(300.0, 460.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), body_height), egui::Sense::hover());
        ui.painter().rect_filled(rect, layout::RADIUS, secondary(ui));
        let mut content =
            ui.new_child(egui::UiBuilder::new().id_salt("empty-queue").max_rect(rect.shrink(24.0)).layout(egui::Layout::top_down(egui::Align::Center)));
        content.set_clip_rect(rect.intersect(ui.clip_rect()));
        content.add_space((body_height - 280.0).max(0.0) / 2.0);
        let (icon, _) = content.allocate_exact_size(egui::vec2(64.0, 64.0), egui::Sense::hover());
        let stroke = egui::Stroke::new(1.5, accent(&content));
        for offset in [0.0, 7.0, 14.0] {
            let card = egui::Rect::from_min_size(icon.min + egui::vec2(8.0 + offset * 0.35, 5.0 + offset), egui::vec2(42.0, 30.0));
            content.painter().rect_filled(card, 5, secondary(&content));
            content.painter().rect_stroke(card, 5, stroke, egui::StrokeKind::Inside);
        }
        content.add_space(layout::SPACE);
        content.label(egui::RichText::new("첫 렌더 작업을 추가하세요").size(20.0).strong());
        content.add(egui::Label::new(egui::RichText::new("프로젝트를 불러오고 렌더할 컴포지션을 선택하세요.").size(13.0).color(muted(&content))).wrap());
        content.add_space(layout::GAP);
        caption(&mut content, "상세 속성은 카드 위에 마우스를 올린 뒤 설정 아이콘에서 바꿀 수 있습니다.");
        content.add_space(layout::GAP);
        if content.add_enabled(!self.running && self.pending_start.is_none(), primary_button("프로젝트 불러오기")).clicked() {
            self.choose_project_for_queue();
        }
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
        let inner = rect.shrink(16.0);
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
            Self::queue_progress(&child, progress, fraction, color);
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
            Self::queue_progress(&child, progress_rect, fraction, color);
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
        ui.painter().rect_filled(status, 4, color.gamma_multiply(0.12));
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
        if hovered || response.has_focus() {
            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            painter.rect_filled(rect, 6, surface(ui));
            painter.rect_stroke(
                rect,
                6,
                egui::Stroke::new(1.0, if response.hovered() || response.has_focus() { accent(ui) } else { border(ui) }),
                egui::StrokeKind::Inside,
            );
            icon_image(ui, UiIcon::Settings).paint_at(ui, rect.shrink(7.0));
            if response.clicked() {
                self.open_queue_task(task.id);
            }
        }
        response.on_hover_text("렌더 설정").on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    fn queue_progress(ui: &egui::Ui, rect: egui::Rect, fraction: f32, color: egui::Color32) {
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
        let response = egui::Modal::new(egui::Id::new("queue-detail")).frame(card(ctx)).show(ctx, |ui| {
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
                    egui::ProgressBar::new((live.done as f32 / total as f32).clamp(0.0, 1.0))
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
                    caption(ui, format!("프레임 저장 폴더 · 자동: {}", settings::frame_output(&edit.project).display()));
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
