use super::*;
use layout::{CONTROL, DashboardGrid, GAP, PADDING, SPACE, flow_content, flow_tile, tile, value_row};

fn fleet_columns(rect: egui::Rect, wide: bool) -> Vec<egui::Rect> {
    let widths =
        if wide { vec![(rect.width() - 506.0).max(160.0), 92.0, 190.0, 92.0, 62.0, 70.0] } else { vec![(rect.width() - 236.0).max(100.0), 132.0, 48.0, 56.0] };
    let mut left = rect.left();
    widths
        .into_iter()
        .map(|width| {
            let column = egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(width, rect.height()));
            left += width;
            column
        })
        .collect()
}

fn resize_handle(ui: &mut egui::Ui, bounds: egui::Rect, id: &'static str, horizontal: bool) -> egui::Response {
    let response = ui.interact(bounds, ui.id().with(id), egui::Sense::click_and_drag());
    let reveal = motion::highlight(ui, response.id.with("hover"), response.hovered() || response.dragged());
    let (name, bytes, size): (&str, &[u8], egui::Vec2) = match horizontal {
        true => ("resize-horizontal-active", include_bytes!("../../assets/ui/figma/resize-horizontal-active.svg"), egui::vec2(60.0, 10.0)),
        false => ("resize-vertical-active", include_bytes!("../../assets/ui/figma/resize-vertical-active.svg"), egui::vec2(10.0, 100.0)),
    };
    if reveal > 0.0
        && let Some(icon) = figma_svg(ui, name, bytes, size)
    {
        icon.tint(egui::Color32::WHITE.gamma_multiply(reveal)).paint_at(ui, egui::Rect::from_center_size(bounds.center(), size));
    }
    response.on_hover_cursor(if horizontal { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal })
}

fn overview_chip(ui: &mut egui::Ui, label: &str, color: egui::Color32) {
    let font = figma_font(12.0, false);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(galley.size().x + 16.0, 24.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, layout::CONTROL_RADIUS, soft(color));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

fn elapsed_clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", seconds / 3600, seconds % 3600 / 60, seconds % 60)
}

impl App {
    pub(super) fn workspace_grid(&mut self, ui: &mut egui::Ui, viewport_height: f32) {
        if self.monitor && self.queue_view && self.settings.host {
            if ui.available_width() >= 900.0 && viewport_height >= 500.0 {
                // The queue scrolls independently so the live status cards remain
                // visible beneath it, as in the host window design.
                let size_id = egui::Id::new("queue-overview-height");
                let gaps = ui.spacing().item_spacing.y * 2.0;
                let max_height = (viewport_height - 240.0 - gaps).max(200.0);
                let card_height = ui.ctx().data(|data| data.get_temp::<f32>(size_id).unwrap_or(260.0)).clamp(200.0, max_height);
                let queue_height = (viewport_height - card_height - layout::GAP - gaps).max(230.0);
                egui::ScrollArea::vertical()
                    .id_salt("queue-overview-scroll")
                    .auto_shrink([false, false])
                    .max_height(queue_height)
                    .show(ui, |ui| self.queue_panel(ui, queue_height));
                let (handle, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), layout::GAP), egui::Sense::hover());
                let response = resize_handle(ui, handle, "queue-height-resize", true).on_hover_text("위아래로 끌어 패널 높이 조절 · 두 번 클릭해 초기화");
                if response.double_clicked() {
                    ui.ctx().data_mut(|data| data.remove::<f32>(size_id));
                } else if response.dragged() {
                    let height = (card_height - response.drag_delta().y).clamp(200.0, max_height);
                    ui.ctx().data_mut(|data| data.insert_temp(size_id, height));
                    ui.ctx().request_repaint();
                }
                self.queue_overview_cards(ui, card_height);
            } else {
                self.queue_panel(ui, viewport_height.max(440.0));
                ui.add_space(8.0);
                self.queue_overview_cards(ui, 260.0);
            }
        } else if ui.available_width() >= 900.0 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), viewport_height.max(680.0)), egui::Sense::hover());
            if !self.monitor {
                self.project_grid(ui, rect);
            } else {
                self.render_grid(ui, rect);
            }
        } else if !self.monitor {
            if self.settings.host {
                flow_content(ui, "host-connection", |ui| {
                    egui::CollapsingHeader::new("호스트 · 작업 PC 연결")
                        .id_salt("compact-host-connection")
                        .default_open(self.settings.token.trim().len() < 8)
                        .show(ui, |ui| self.setup_content(ui));
                });
                ui.add_space(SPACE);
                self.queue_panel(ui, viewport_height.max(440.0));
            } else {
                flow_content(ui, "client-connection", |ui| self.setup_content(ui));
                ui.add_space(SPACE);
                flow_content(ui, "configuration", |ui| self.configuration_content(ui));
            }
        } else {
            let (workers, _) = farm::worker_snapshot(&self.state);
            let preview_height = (ui.available_width() * 9.0 / 16.0 + 96.0).clamp(288.0, 480.0);
            flow_tile(ui, preview_height, "preview", |ui| self.preview_content(ui, &workers, preview_height - PADDING as f32 * 2.0));
            ui.add_space(SPACE);
            let progress_height = if ui.available_width() < 600.0 { 292.0 } else { 232.0 };
            flow_tile(ui, progress_height, "render-progress", |ui| self.progress_content(ui, &workers));
            ui.add_space(SPACE);
            if self.settings.host {
                let height = 144.0 + workers.len().max(1) as f32 * 40.0;
                flow_tile(ui, height, "workers", |ui| self.workers_content(ui, &workers, height - PADDING as f32 * 2.0));
                ui.add_space(SPACE);
            }
            flow_tile(ui, 164.0, "current-job", |ui| self.job_content(ui));
            ui.add_space(SPACE);
            flow_tile(ui, 256.0, "activity", |ui| self.activity_content(ui, &workers));
        }
    }

    fn queue_overview_cards(&mut self, ui: &mut egui::Ui, height: f32) {
        let workers = if self.pending_start.is_some() { Vec::new() } else { farm::worker_snapshot(&self.state).0 };
        let wide = ui.available_width() >= 700.0;
        let width = ui.available_width();
        if wide {
            let size_id = egui::Id::new("queue-overview-pc-width");
            let default_width = (width * 0.255).clamp(240.0, 300.0);
            let max_width = (width - 370.0).clamp(240.0, 480.0);
            let right_width = ui.ctx().data(|data| data.get_temp::<f32>(size_id).unwrap_or(default_width)).clamp(240.0, max_width);
            let (bounds, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            let left = egui::Rect::from_min_max(bounds.min, egui::pos2(bounds.right() - right_width - layout::GAP, bounds.bottom()));
            let right = egui::Rect::from_min_max(egui::pos2(left.right() + layout::GAP, bounds.top()), bounds.max);
            tile(ui, left, "queue-render-status", false, |ui| self.queue_status_content(ui, &workers, height - PADDING as f32 * 2.0));
            tile(ui, right, "queue-connected-pcs", false, |ui| self.queue_workers_content(ui, &workers, height - PADDING as f32 * 2.0));
            let handle = egui::Rect::from_min_max(egui::pos2(left.right(), bounds.top()), egui::pos2(right.left(), bounds.bottom()));
            let response = resize_handle(ui, handle, "queue-pc-width-resize", false).on_hover_text("좌우로 끌어 PC 패널 너비 조절 · 두 번 클릭해 초기화");
            if response.double_clicked() {
                ui.ctx().data_mut(|data| data.remove::<f32>(size_id));
            } else if response.dragged() {
                let right_width = (right_width - response.drag_delta().x).clamp(240.0, max_width);
                ui.ctx().data_mut(|data| data.insert_temp(size_id, right_width));
                ui.ctx().request_repaint();
            }
        } else {
            flow_tile(ui, 260.0, "queue-render-status", |ui| self.queue_status_content(ui, &workers, 260.0 - PADDING as f32 * 2.0));
            ui.add_space(8.0);
            flow_tile(ui, 260.0, "queue-connected-pcs", |ui| self.queue_workers_content(ui, &workers, 260.0 - PADDING as f32 * 2.0));
        }
    }

    fn queue_status_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], height: f32) {
        if self.pending_start.is_some() {
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(((height - 80.0) / 2.0).max(0.0));
                ui.add_sized([24.0, 24.0], egui::Spinner::new());
                ui.label(egui::RichText::new("렌더링 준비 중").font(figma_font(16.0, false)));
                caption(ui, "기존 프레임 확인이 끝나면 PC를 연결할 수 있습니다.");
            });
            return;
        }
        let (done, total, _, failed) = farm::progress_snapshot(&self.state);
        let (started, finished_elapsed, local_progress, final_result, live_preview) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                state.started,
                state.finished_elapsed,
                state.local_progress,
                state.final_result.clone(),
                state.latest_preview.clone().or_else(|| state.local_preview.clone()),
            )
        };
        let elapsed = finished_elapsed.map(|elapsed| elapsed.as_secs_f64()).or_else(|| {
            (self.running && !self.stopping && failed == 0 && final_result.is_none()).then(|| started.map(|at| at.elapsed().as_secs_f64())).flatten()
        });
        if started.is_none() && local_progress.is_none() {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
            let center = rect.center();
            let color = muted(ui);
            if let Some(icon) =
                figma_svg(ui, "status-empty-broadcast", include_bytes!("../../assets/ui/figma/status-empty-broadcast.svg"), egui::vec2(28.0, 28.0))
            {
                icon.paint_at(ui, egui::Rect::from_center_size(center - egui::vec2(0.0, 30.0), egui::vec2(28.0, 28.0)));
            }
            ui.painter().text(
                center + egui::vec2(0.0, 4.0),
                egui::Align2::CENTER_CENTER,
                "렌더중인 컴포지션 없음",
                egui::FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
            ui.painter().text(
                center + egui::vec2(0.0, 25.0),
                egui::Align2::CENTER_CENTER,
                "렌더링이 시작되면 여기에 정보가 표시됩니다.",
                egui::FontId::proportional(12.0),
                color,
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("렌더링 상태").font(figma_font(16.0, false)).extra_letter_spacing(-0.4));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, color) = match &final_result {
                    Some(Ok(())) => ("완료", success(ui)),
                    Some(Err(_)) => ("실패", danger(ui)),
                    None if failed > 0 => ("실패", danger(ui)),
                    None if self.stopping => ("중단 중", muted(ui)),
                    None if self.running => ("렌더링 중", accent(ui)),
                    None => ("중단", muted(ui)),
                };
                overview_chip(ui, label, color);
            });
        });
        let preview = live_preview.or_else(|| workers.iter().filter_map(|w| w.preview.clone()).max_by_key(|p| p.frame));
        let content_width = ui.available_width();
        let image_width = (content_width * 0.42).min(356.0);
        let image_height = (image_width * 9.0 / 16.0).min((height - CONTROL - SPACE).max(0.0));
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(image_width, image_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                let image_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(image_width, image_height));
                self.dashboard_image(ui, u64::MAX - 2, preview.as_ref(), image_height);
                let response = ui.interact(image_rect, ui.id().with("queue-status-preview"), egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "프레임 탐색"));
                if response.on_hover_text("완료 프레임 탐색 / 시험 렌더").clicked() {
                    self.preview_open = true;
                }
            });
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(if self.settings.comp.is_empty() { "컴포지션" } else { &self.settings.comp }).size(14.0).strong())
                        .truncate(),
                );
                let file = std::path::Path::new(&self.settings.project).file_name().unwrap_or_default().to_string_lossy();
                caption(ui, file.as_ref());
                ui.add_space(8.0);
                let speed: f64 = workers
                    .iter()
                    .filter(|w| w.connected && w.assigned.is_some())
                    .map(|w| w.started.map_or(w.fps, |at| w.current_done as f64 / at.elapsed().as_secs_f64().max(0.001)))
                    .sum();
                let eta = if speed > 0.0 { format!("약 {}", duration((total - done).max(0) as f64 / speed)) } else { "—".into() };
                let remaining = self.settings.queue.iter().filter(|task| task.status == crate::queue::Status::Pending).count();
                ui.columns(4, |cols| {
                    for (column, (label, value)) in cols.iter_mut().zip([
                        ("소요 시간", elapsed.map(elapsed_clock).unwrap_or_else(|| "—".into())),
                        ("예상 남은 시간", eta),
                        ("남은 컴포지션", format!("{remaining}개")),
                        ("초당 프레임", format!("{speed:.1} fps")),
                    ]) {
                        column.spacing_mut().item_spacing.y = 2.0;
                        column.add(
                            egui::Label::new(egui::RichText::new(label).size(12.0).line_height(Some(16.0)).extra_letter_spacing(-0.4).color(muted(column)))
                                .truncate(),
                        );
                        column.add(
                            egui::Label::new(egui::RichText::new(value).font(figma_font(14.0, false)).line_height(Some(20.0)).extra_letter_spacing(-0.4))
                                .truncate(),
                        );
                    }
                });
                ui.add_space(8.0);
                let fraction = if total > 0 { (done as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
                ui.horizontal(|ui| {
                    caption(ui, format!("{done} / {total} 프레임"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(format!("{:.0}%", fraction * 100.0)).size(12.0).color(accent(ui)));
                    });
                });
                ui.add(
                    egui::ProgressBar::new(motion::progress(ui, ("overview", self.queue_active), fraction))
                        .fill(accent(ui))
                        .desired_width(ui.available_width())
                        .desired_height(4.0),
                );
            });
        });
    }

    fn queue_workers_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], height: f32) {
        let connected: Vec<_> = workers.iter().filter(|worker| worker.connected).collect();
        if connected.is_empty() {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
            if let Some(icon) = figma_svg(ui, "pc-link-slash", include_bytes!("../../assets/ui/figma/pc-link-slash.svg"), egui::vec2(28.0, 28.0)) {
                icon.paint_at(ui, egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 22.0), egui::vec2(28.0, 28.0)));
            }
            ui.painter().text(
                rect.center() + egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "연결된 PC 없음",
                egui::FontId::proportional(16.0),
                muted(ui),
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("연결된 PC").font(figma_font(16.0, false)).extra_letter_spacing(-0.4));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                overview_chip(ui, &format!("{}대", connected.len()), success(ui));
            });
        });
        egui::ScrollArea::vertical().id_salt("connected-pc-list").max_height((height - 32.0).max(0.0)).show(ui, |ui| {
            for worker in connected {
                let name = worker.label.split(" @ ").next().unwrap_or(&worker.label);
                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
                let hover = motion::highlight(ui, ui.id().with(("connected-hover", worker.id)), response.hovered());
                ui.painter().rect_filled(rect, layout::CONTROL_RADIUS, secondary(ui).gamma_multiply(hover));
                // The wire protocol does not report a worker OS, so use the
                // design's generic display icon rather than guessing one.
                if let Some(icon) = figma_svg(ui, "pc-display", include_bytes!("../../assets/ui/figma/pc-display.svg"), egui::vec2(12.0, 12.0)) {
                    icon.paint_at(ui, egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 16.0), egui::vec2(12.0, 12.0)));
                }
                ui.painter().text(rect.min + egui::vec2(30.0, 7.0), egui::Align2::LEFT_TOP, name, egui::FontId::proportional(12.0), ui.visuals().text_color());
                ui.painter().text(rect.min + egui::vec2(30.0, 23.0), egui::Align2::LEFT_TOP, &worker.ip, egui::FontId::proportional(12.0), muted(ui));
                ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, border(ui)));
                if response.on_hover_text("PC 상세 정보").clicked() {
                    self.worker_details = Some(worker.id);
                }
            }
        });
    }

    fn project_grid(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        if !self.settings.host {
            let form_width = ((rect.width() - GAP) * 0.64).round();
            let form = egui::Rect::from_min_size(rect.min, egui::vec2(form_width, rect.height()));
            let information = egui::Rect::from_min_max(egui::pos2(form.right() + GAP, rect.top()), rect.max);
            tile(ui, form, "client-connection", false, |ui| self.setup_content(ui));
            tile(ui, information, "client-information", false, |ui| {
                self.connection_content(ui);
                ui.add_space(GAP);
                self.configuration_content(ui);
            });
            return;
        }
        let connection_width = (rect.width() * 0.29).clamp(272.0, 344.0);
        let queue = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - connection_width - GAP, rect.bottom()));
        let connection = egui::Rect::from_min_max(egui::pos2(queue.right() + GAP, rect.top()), rect.max);
        let mut queue_ui =
            ui.new_child(egui::UiBuilder::new().id_salt("project-render-queue").max_rect(queue).layout(egui::Layout::top_down(egui::Align::Min)));
        queue_ui.set_clip_rect(queue.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical().id_salt("project-render-queue-scroll").auto_shrink([false, false]).max_height(queue.height()).show(&mut queue_ui, |ui| {
            ui.set_width(ui.available_width());
            self.queue_panel(ui, queue.height());
        });
        let mut connection_ui =
            ui.new_child(egui::UiBuilder::new().id_salt("project-host-connection").max_rect(connection).layout(egui::Layout::top_down(egui::Align::Min)));
        connection_ui.set_clip_rect(connection.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical().id_salt("project-host-connection-scroll").auto_shrink([false, false]).max_height(connection.height()).show(
            &mut connection_ui,
            |ui| {
                ui.set_width(ui.available_width());
                flow_content(ui, "host-connection", |ui| self.setup_content(ui));
                ui.add_space(SPACE);
                flow_content(ui, "host-status", |ui| self.connection_content(ui));
            },
        );
    }

    fn render_grid(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let grid = DashboardGrid::new(rect);
        let (workers, _) = farm::worker_snapshot(&self.state);
        tile(ui, grid.job, "current-job", false, |ui| self.job_content(ui));
        tile(ui, grid.progress, "render-progress", false, |ui| self.progress_content(ui, &workers));
        tile(ui, grid.preview, "preview", false, |ui| self.preview_content(ui, &workers, grid.preview.height() - PADDING as f32 * 2.0));
        if self.settings.host {
            tile(ui, grid.workers, "workers", false, |ui| self.workers_content(ui, &workers, grid.workers.height() - PADDING as f32 * 2.0));
        } else {
            tile(ui, grid.workers, "local-profile", false, |ui| {
                section_title(ui, "이 PC의 렌더 환경");
                let profile = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_profile.clone();
                ui.label(if profile.is_empty() { "작업을 받으면 렌더 환경을 자동으로 설정합니다." } else { &profile });
                value_row(ui, "EffectCraft", self.engine_updater.local.as_deref().unwrap_or("확인 중"));
            });
        }
    }

    fn job_content(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        section_title(ui, "현재 작업");
        if self.settings.host {
            ui.add(
                egui::Label::new(egui::RichText::new(if self.settings.comp.is_empty() { "프로젝트 준비 전" } else { &self.settings.comp }).size(14.0).strong())
                    .truncate(),
            )
            .on_hover_text(&self.settings.project);
            let output = std::path::Path::new(&self.settings.final_video).file_name().unwrap_or_default().to_string_lossy();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "{}–{} 프레임  ·  {}",
                        self.settings.range_start,
                        self.settings.range_end,
                        if output.is_empty() { "출력 선택 전" } else { &output }
                    ))
                    .size(11.0)
                    .color(muted(ui)),
                )
                .truncate(),
            )
            .on_hover_text(&self.settings.final_video);
        } else {
            value_row(ui, "호스트", if self.settings.client_address.is_empty() { "선택 전" } else { &self.settings.client_address });
            caption(ui, "프로젝트와 렌더 설정은 호스트에서 받습니다.");
        }
    }

    fn connection_content(&mut self, ui: &mut egui::Ui) {
        section_title(ui, if self.settings.host { "호스트 상태" } else { "호스트 연결" });
        let (connected, selected) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.worker_info.values().filter(|w| w.connected).count(), state.worker_info.values().filter(|w| w.connected && w.enabled).count())
        };
        if self.settings.host {
            ui.columns(2, |columns| {
                metric(&mut columns[0], "연결된 PC", &format!("{connected}대"));
                metric(&mut columns[1], "렌더 참여", &format!("{selected}대"));
            });
            ui.separator();
            let readiness = [
                ("대기 작업", self.settings.queue.iter().any(|task| task.status == crate::queue::Status::Pending)),
                ("접속 암호", self.settings.token.trim().len() >= 8),
                ("렌더 엔진", self.engine_updater.local.is_some()),
                ("참여 PC", selected > 0),
            ];
            for row in readiness.chunks(2) {
                ui.columns(2, |columns| {
                    for (column, (label, ready)) in columns.iter_mut().zip(row) {
                        column.horizontal(|ui| {
                            let color = if *ready { success(ui) } else { muted(ui) };
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                            if *ready {
                                ui.painter().circle_filled(rect.center(), 3.0, color);
                            } else {
                                ui.painter().circle_stroke(rect.center(), 3.0, egui::Stroke::new(1.0, color));
                            }
                            ui.add(egui::Label::new(egui::RichText::new(*label).size(12.0).color(color)).truncate()).on_hover_text(if *ready {
                                "준비되었습니다"
                            } else {
                                "작업 큐와 연결 설정을 확인하세요"
                            });
                        });
                    }
                });
            }
        } else {
            value_row(ui, "호스트 주소", if self.settings.client_address.is_empty() { "선택 전" } else { &self.settings.client_address });
            caption(ui, "호스트와 같은 네트워크에서 연결하세요.");
            caption(ui, "호스트와 동일한 접속 암호를 입력하세요.");
        }
    }

    fn configuration_content(&mut self, ui: &mut egui::Ui) {
        section_title(ui, if self.settings.host { "렌더 설정" } else { "작업 설정" });
        if !self.settings.host {
            caption(ui, "렌더 품질과 출력 설정은 호스트에서 정합니다.");
            value_row(ui, "EffectCraft", self.engine_updater.local.as_deref().unwrap_or("확인 중"));
            return;
        }
        for (label, value) in [
            ("품질", if self.settings.render.draft { "Draft".into() } else { "Best".into() }),
            ("최적화", if self.settings.render.automatic { "자동".into() } else { "수동".into() }),
            ("오디오", if self.settings.render.audio { "포함".into() } else { "제외".into() }),
            ("미리보기", if self.settings.render.previews { format!("{}초 간격", self.settings.render.preview_seconds) } else { "끄기".into() }),
        ] {
            value_row(ui, label, &value);
        }
        if ui.add_sized([120.0, CONTROL], egui::Button::new("렌더 설정 변경")).clicked() {
            self.settings_tab = 1;
            self.settings_open = true;
        }
    }

    fn progress_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        let (done, total, connected, failed) = farm::progress_snapshot(&self.state);
        let (elapsed, local_progress, status) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.started.map(|at| state.finished_elapsed.unwrap_or_else(|| at.elapsed()).as_secs_f64()), state.local_progress, state.final_status.clone())
        };
        let speed: f64 = workers
            .iter()
            .filter(|w| w.connected && w.assigned.is_some())
            .map(|w| w.started.map_or(w.fps, |at| w.current_done as f64 / at.elapsed().as_secs_f64().max(0.001)))
            .sum::<f64>()
            .max(0.0);
        let (progress, text) = if self.settings.host && total > 0 {
            (done as f32 / total as f32, format!("{done} / {total} 프레임"))
        } else if let Some((start, end, done)) = local_progress {
            (done as f32 / (end - start).max(1) as f32, format!("{start}–{end} · {done}/{} 프레임", end - start))
        } else {
            (0.0, "작업 대기".into())
        };
        let waiting = elapsed.is_none() && local_progress.is_none();
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            section_title(ui, "전체 진행률");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(if waiting { "대기".into() } else { format!("{:.0}%", progress * 100.0) }).size(30.0).strong());
            });
        });
        let animated = motion::progress(ui, ("total", self.queue_active), progress);
        ui.add(egui::ProgressBar::new(animated.clamp(0.0, 1.0)).fill(accent(ui)).desired_width(ui.available_width()).desired_height(4.0));
        let progress_label = if failed > 0 { format!("{text} · 실패 {failed}구간") } else { text };
        ui.add(egui::Label::new(egui::RichText::new(progress_label).size(11.0).color(muted(ui))).truncate()).on_hover_text(if status.is_empty() {
            "프로젝트와 PC를 준비한 뒤 렌더링을 시작하세요."
        } else {
            &status
        });
        let eta = if total > 0 && done >= total {
            "프레임 완료".into()
        } else if speed > 0.0 {
            duration((total - done).max(0) as f64 / speed)
        } else if waiting {
            "—".into()
        } else {
            "계산 중".into()
        };
        let values = if self.settings.host {
            vec![
                ("예상 남은 시간", eta),
                ("작업 PC", format!("{connected}대")),
                ("처리 속도", format!("{speed:.1} fps")),
                ("경과 시간", elapsed.map(duration).unwrap_or_else(|| "—".into())),
            ]
        } else {
            let (start, end, done) = local_progress.unwrap_or((0, 0, 0));
            vec![
                ("구간 시작", start.to_string()),
                ("구간 끝 · 미포함", end.to_string()),
                ("완료 프레임", done.to_string()),
                ("남은 프레임", (end - start - done).max(0).to_string()),
            ]
        };
        let columns = if ui.available_width() >= 560.0 { 4 } else { 2 };
        for row in values.chunks(columns) {
            ui.columns(columns, |cols| {
                for (column, (label, value)) in cols.iter_mut().zip(row) {
                    column.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(egui::RichText::new(*label).size(10.0).color(muted(ui)));
                        ui.add(egui::Label::new(egui::RichText::new(value).size(17.0).strong()).truncate()).on_hover_text(value);
                    });
                }
            });
        }
    }

    fn preview_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], height: f32) {
        ui.horizontal(|ui| {
            section_title(ui, "라이브 미리보기");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_sized([96.0, CONTROL], egui::Button::new("프레임 탐색").frame(false)).on_hover_text("완료 프레임 탐색 / 시험 렌더").clicked()
                {
                    self.preview_open = true;
                }
            });
        });
        let preview = self
            .inspected
            .clone()
            .or_else(|| workers.iter().filter_map(|w| w.preview.clone()).max_by_key(|p| p.frame))
            .or_else(|| self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_preview.clone());
        self.dashboard_image(ui, u64::MAX - 1, preview.as_ref(), (height - CONTROL * 2.0 - SPACE * 3.0).max(112.0));
        ui.horizontal(|ui| {
            caption(ui, preview.as_ref().map_or_else(|| "프레임 대기".into(), |p| format!("FRAME {:06}", p.frame)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("작업 기록").frame(false)).clicked() {
                    self.logs_open = true;
                }
                let messages = farm::snapshot(&self.state).4;
                if let Some(message) = messages.iter().rev().find(|m| !m.contains("[DEBUG]")) {
                    let text = message.splitn(4, "] ").last().unwrap_or(message);
                    ui.add(egui::Label::new(egui::RichText::new(text).size(10.0).color(muted(ui))).truncate()).on_hover_text(message);
                }
            });
        });
    }

    pub(super) fn dashboard_image(&mut self, ui: &mut egui::Ui, key: u64, preview: Option<&farm::Preview>, height: f32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, layout::INSET_RADIUS, secondary(ui));
        if let Some(preview) = preview
            && let Some(texture) = self.preview_texture(ui, key, preview)
        {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
            egui::Image::new((texture.id(), size)).corner_radius(layout::INSET_RADIUS).paint_at(ui, fitted);
        } else {
            if rect.height() > 128.0 && rect.width() > 240.0 {
                let center = rect.center() - egui::vec2(0.0, 24.0);
                let monitor = egui::Rect::from_center_size(center, egui::vec2(42.0, 28.0));
                painter.rect_stroke(monitor, 3, egui::Stroke::new(1.5, muted(ui)), egui::StrokeKind::Inside);
                painter.line_segment([center + egui::vec2(0.0, 14.0), center + egui::vec2(0.0, 21.0)], egui::Stroke::new(1.5, muted(ui)));
                painter.line_segment([center + egui::vec2(-10.0, 21.0), center + egui::vec2(10.0, 21.0)], egui::Stroke::new(1.5, muted(ui)));
                painter.text(
                    rect.center() + egui::vec2(0.0, 22.0),
                    egui::Align2::CENTER_CENTER,
                    "미리보기 준비 중",
                    egui::FontId::proportional(14.0),
                    muted(ui),
                );
                painter.text(
                    rect.center() + egui::vec2(0.0, 46.0),
                    egui::Align2::CENTER_CENTER,
                    "첫 프레임이 완성되면 여기에 표시됩니다",
                    egui::FontId::proportional(12.0),
                    muted(ui),
                );
            } else {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "프레임 대기", egui::FontId::proportional(12.0), muted(ui));
            }
        }
    }

    fn workers_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], _height: f32) {
        let search_id = egui::Id::new("fleet-search");
        let filter_id = egui::Id::new("fleet-filter");
        let mut search = ui.ctx().data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
        let mut filter = ui.ctx().data_mut(|data| data.get_temp::<u8>(filter_id).unwrap_or_default());
        let wide = ui.available_width() >= 700.0;
        ui.horizontal(|ui| {
            section_title(ui, "작업 PC");
            caption(
                ui,
                format!("{}대 연결 · {}대 참여", workers.iter().filter(|w| w.connected).count(), workers.iter().filter(|w| w.connected && w.enabled).count()),
            );
            if wide {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_sized([192.0, CONTROL], egui::TextEdit::singleline(&mut search).hint_text("PC 이름 또는 IP 검색"));
                    ui.selectable_value(&mut filter, 2, "참여 중");
                    ui.selectable_value(&mut filter, 1, "연결됨");
                    ui.selectable_value(&mut filter, 0, "전체");
                });
            }
        });
        if !wide {
            ui.horizontal(|ui| {
                ui.add_sized([(ui.available_width() - 156.0).max(120.0), CONTROL], egui::TextEdit::singleline(&mut search).hint_text("PC 이름 또는 IP 검색"));
                ui.selectable_value(&mut filter, 0, "전체");
                ui.selectable_value(&mut filter, 2, "참여 중");
            });
        }
        ui.ctx().data_mut(|data| {
            data.insert_temp(search_id, search.clone());
            data.insert_temp(filter_id, filter);
        });
        if workers.is_empty() {
            ui.add_space(16.0);
            caption(ui, "연결된 작업 PC가 없습니다. 호스트를 연 뒤 같은 네트워크에서 연결하세요.");
            if ui.button("연결 설정 보기").clicked() {
                self.monitor = false;
            }
            return;
        }
        let query = search.trim().to_lowercase();
        let filtered = workers
            .iter()
            .filter(|worker| {
                (query.is_empty() || worker.label.to_lowercase().contains(&query) || worker.ip.to_lowercase().contains(&query))
                    && match filter {
                        1 => worker.connected,
                        2 => worker.connected && worker.enabled,
                        _ => true,
                    }
            })
            .collect::<Vec<_>>();
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        let (header, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
        ui.painter().rect_filled(header, layout::CONTROL_RADIUS, secondary(ui));
        let columns = fleet_columns(header, wide);
        for (rect, label) in columns.iter().zip(if wide {
            vec!["컴퓨터", "상태", "현재 구간", "처리 속도", "참여", ""]
        } else {
            vec!["컴퓨터 / 상태", "현재 구간", "참여", ""]
        }) {
            ui.painter().text(rect.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(11.0), muted(ui));
        }
        for worker in &filtered {
            let (row, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
            let hover = motion::highlight(ui, ui.id().with(("fleet-hover", worker.id)), response.contains_pointer());
            ui.painter().rect_filled(row, 0, secondary(ui).gamma_multiply(hover));
            self.worker_row(ui, worker, row, wide);
            ui.painter().hline(row.x_range(), row.bottom(), egui::Stroke::new(1.0, border(ui).gamma_multiply(0.65)));
        }
        ui.spacing_mut().item_spacing.y = spacing;
        if filtered.is_empty() {
            ui.add_space(12.0);
            caption(ui, "조건에 맞는 PC가 없습니다.");
            if ui.button("검색과 필터 초기화").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(search_id, String::new());
                    data.insert_temp(filter_id, 0_u8);
                });
            }
        }
    }

    fn worker_row(&mut self, ui: &mut egui::Ui, worker: &farm::WorkerInfo, row: egui::Rect, wide: bool) {
        let columns = fleet_columns(row, wide);
        let name = worker.label.split(" @ ").next().unwrap_or(&worker.label);
        let (status, color) = if !worker.connected {
            ("연결 끊김", muted(ui))
        } else if !worker.enabled {
            ("제외", muted(ui))
        } else if worker.assigned.is_some() {
            ("렌더링", accent(ui))
        } else {
            ("대기", success(ui))
        };
        let title_rect = columns[0].shrink2(egui::vec2(8.0, 1.0));
        let response = ui.interact(title_rect, ui.id().with(("fleet-details", worker.id)), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        let mut title =
            ui.new_child(egui::UiBuilder::new().id_salt(("fleet-name", worker.id)).max_rect(title_rect).layout(egui::Layout::top_down(egui::Align::Min)));
        title.set_clip_rect(title_rect.intersect(ui.clip_rect()));
        title.spacing_mut().item_spacing.y = 0.0;
        title.add(egui::Label::new(egui::RichText::new(name).size(12.0).strong()).truncate());
        title.add(
            egui::Label::new(egui::RichText::new(if wide { worker.ip.clone() } else { format!("{status} · {}", worker.ip) }).size(10.0).color(muted(ui)))
                .truncate(),
        );
        if response.on_hover_text(&worker.label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.worker_details = Some(worker.id);
        }
        let progress_index = if wide { 2 } else { 1 };
        if wide {
            let status_rect = columns[1];
            ui.painter().circle_filled(status_rect.left_center() + egui::vec2(10.0, 0.0), 3.0, color);
            ui.painter().text(status_rect.left_center() + egui::vec2(20.0, 0.0), egui::Align2::LEFT_CENTER, status, egui::FontId::proportional(11.0), color);
            ui.painter().text(
                columns[3].left_center() + egui::vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                format!("{:.1} fps", worker.fps),
                egui::FontId::monospace(11.0),
                ui.visuals().text_color(),
            );
        }
        let progress_rect = columns[progress_index].shrink2(egui::vec2(8.0, 0.0));
        let (fraction, detail) = if let Some((start, end)) = worker.assigned {
            (worker.current_done as f32 / (end - start).max(1) as f32, format!("{} / {} 프레임", worker.current_done, end - start))
        } else {
            (0.0, format!("누적 {} 프레임", worker.completed_frames))
        };
        ui.painter().text(progress_rect.min + egui::vec2(0.0, 5.0), egui::Align2::LEFT_TOP, detail, egui::FontId::proportional(10.0), muted(ui));
        let track = egui::Rect::from_min_size(progress_rect.min + egui::vec2(0.0, 25.0), egui::vec2(progress_rect.width(), 4.0));
        ui.painter().rect_filled(track, 2, border(ui));
        let animated = motion::progress(ui, (worker.id, worker.assigned), fraction);
        if animated > 0.0 {
            ui.painter().rect_filled(egui::Rect::from_min_size(track.min, egui::vec2(track.width() * animated.clamp(0.0, 1.0), track.height())), 2, accent(ui));
        }
        let enabled_index = if wide { 4 } else { 2 };
        let mut enabled_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("fleet-participation", worker.id))
                .max_rect(columns[enabled_index])
                .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        let mut enabled = worker.enabled;
        if enabled_ui.add_enabled(worker.connected, egui::Checkbox::without_text(&mut enabled)).on_hover_text("이 PC를 렌더링에 사용").changed() {
            self.set_worker_participation(worker, enabled);
        }
        let button_rect = columns[enabled_index + 1].shrink2(egui::vec2(4.0, 6.0));
        ui.push_id(worker.id, |ui| {
            if button_at(ui, button_rect, "fleet-details-action", ButtonSpec::new("상세", ButtonKind::Quiet, true)).clicked() {
                self.worker_details = Some(worker.id);
            }
        });
    }

    fn activity_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        ui.horizontal(|ui| {
            section_title(ui, "최근 활동");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_sized([80.0, CONTROL], egui::Button::new("전체 기록").frame(false)).clicked() {
                    self.logs_open = true;
                }
            });
        });
        let mut segments = farm::timeline(&self.state);
        if segments.is_empty() {
            segments = workers.iter().filter_map(|w| w.assigned.map(|(start, end)| (start, end, 1))).collect();
        }
        if let (Some(first), Some(last)) = (segments.iter().map(|s| s.0).min(), segments.iter().map(|s| s.1).max()) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), egui::Sense::hover());
            for (start, end, status) in segments {
                let x = |frame: i64| rect.left() + (frame - first) as f32 / (last - first).max(1) as f32 * rect.width();
                let color = match status {
                    1 => accent(ui),
                    2 => muted(ui),
                    3 => danger(ui),
                    _ => border(ui),
                };
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x(start), rect.top()), egui::pos2(x(end), rect.bottom())), 2, color);
            }
            caption(ui, format!("프레임 {first}–{last} · 분배 현황"));
            ui.separator();
        }
        let messages = farm::snapshot(&self.state).4;
        if messages.is_empty() {
            caption(ui, "연결과 렌더링 활동이 여기에 표시됩니다.");
        }
        for message in messages.iter().rev().filter(|m| !m.contains("[DEBUG]")).take(12) {
            let text = message.splitn(4, "] ").last().unwrap_or(message);
            let color = if message.contains("[ERROR]") { danger(ui) } else { muted(ui) };
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 16.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 2.0, if message.contains("[ERROR]") { danger(ui) } else { success(ui) });
                ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(color)).wrap()).on_hover_text(message);
            });
            ui.separator();
        }
    }
}
