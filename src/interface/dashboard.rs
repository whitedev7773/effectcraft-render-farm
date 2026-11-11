use super::*;
use layout::{CONTROL, DashboardGrid, GAP, PADDING, SPACE, flow_tile, grid_cells, tile, value_row};

impl App {
    pub(super) fn workspace_grid(&mut self, ui: &mut egui::Ui, viewport_height: f32) {
        if ui.available_width() >= 1120.0 {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), viewport_height.max(704.0)), egui::Sense::hover());
            if !self.monitor {
                self.project_grid(ui, rect);
            } else if self.queue_view && self.settings.host {
                tile(ui, rect, "queue-dashboard", false, |ui| self.queue_panel(ui, rect.height() - PADDING as f32 * 2.0));
            } else {
                self.render_grid(ui, rect);
            }
        } else if !self.monitor {
            let height = if self.running {
                224.0
            } else if self.settings.host {
                624.0
            } else {
                440.0
            };
            flow_tile(ui, height, "project-form", |ui| self.setup_content(ui));
            ui.add_space(SPACE);
            flow_tile(ui, 224.0, "connection", |ui| self.connection_content(ui));
            ui.add_space(SPACE);
            flow_tile(ui, 224.0, "configuration", |ui| self.configuration_content(ui));
        } else if self.queue_view && self.settings.host {
            let height = viewport_height.max(440.0);
            flow_tile(ui, height, "queue-dashboard", |ui| self.queue_panel(ui, height - PADDING as f32 * 2.0));
        } else {
            let (workers, _) = farm::worker_snapshot(&self.state);
            let progress_height = if ui.available_width() < 600.0 { 304.0 } else { 224.0 };
            flow_tile(ui, progress_height, "render-progress", |ui| self.progress_content(ui, &workers));
            ui.add_space(SPACE);
            if self.settings.host {
                let capacity = ((ui.available_width() - PADDING as f32 * 2.0 + GAP) / 256.0).floor().max(1.0) as usize;
                let rows = workers.len().max(1).div_ceil(capacity);
                let height = 80.0 + rows as f32 * 184.0 + rows.saturating_sub(1) as f32 * GAP;
                flow_tile(ui, height, "workers", |ui| self.workers_content(ui, &workers, height - PADDING as f32 * 2.0));
                ui.add_space(SPACE);
            }
            let preview_height = (ui.available_width() * 9.0 / 16.0 + 96.0).clamp(304.0, 448.0);
            flow_tile(ui, preview_height, "preview", |ui| self.preview_content(ui, &workers, preview_height - PADDING as f32 * 2.0));
            ui.add_space(SPACE);
            flow_tile(ui, 224.0, "current-job", |ui| self.job_content(ui));
            ui.add_space(SPACE);
            flow_tile(ui, 256.0, "activity", |ui| self.activity_content(ui, &workers));
        }
    }

    fn project_grid(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let column = (rect.width() - GAP * 2.0) / 3.0;
        let header_height = 224.0;
        let form = egui::Rect::from_min_size(rect.min, egui::vec2(column, rect.height()));
        let connection = egui::Rect::from_min_size(egui::pos2(form.right() + GAP, rect.top()), egui::vec2(column, header_height));
        let configuration = egui::Rect::from_min_max(egui::pos2(connection.right() + GAP, rect.top()), egui::pos2(rect.right(), connection.bottom()));
        let queue = egui::Rect::from_min_max(egui::pos2(connection.left(), connection.bottom() + GAP), rect.max);
        tile(ui, form, "project-form", false, |ui| self.setup_content(ui));
        tile(ui, connection, "connection", false, |ui| self.connection_content(ui));
        tile(ui, configuration, "configuration", false, |ui| self.configuration_content(ui));
        tile(ui, queue, "project-queue", false, |ui| {
            if self.settings.host {
                self.queue_panel(ui, queue.height() - PADDING as f32 * 2.0);
            } else {
                section_title(ui, "연결 상태");
                caption(ui, "호스트에 연결하면 작업 지시와 렌더 진행 상태를 확인할 수 있습니다.");
                value_row(ui, "호스트", if self.settings.client_address.is_empty() { "선택 전" } else { &self.settings.client_address });
                value_row(ui, "EffectCraft", self.engine_updater.local.as_deref().unwrap_or("확인 중"));
            }
        });
    }

    fn render_grid(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let grid = DashboardGrid::new(rect);
        let (workers, _) = farm::worker_snapshot(&self.state);
        tile(ui, grid.job, "current-job", false, |ui| self.job_content(ui));
        tile(ui, grid.progress, "render-progress", false, |ui| self.progress_content(ui, &workers));
        tile(ui, grid.activity, "activity", false, |ui| self.activity_content(ui, &workers));
        if self.settings.host {
            tile(ui, grid.preview, "preview", false, |ui| self.preview_content(ui, &workers, grid.preview.height() - PADDING as f32 * 2.0));
            tile(ui, grid.workers, "workers", false, |ui| self.workers_content(ui, &workers, grid.workers.height() - PADDING as f32 * 2.0));
        } else {
            tile(ui, grid.preview, "local-profile", false, |ui| {
                section_title(ui, "이 PC의 렌더 환경");
                let profile = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_profile.clone();
                ui.label(if profile.is_empty() { "작업을 받으면 렌더 환경을 자동으로 설정합니다." } else { &profile });
                value_row(ui, "EffectCraft", self.engine_updater.local.as_deref().unwrap_or("확인 중"));
            });
            tile(ui, grid.workers, "preview", false, |ui| self.preview_content(ui, &workers, grid.workers.height() - PADDING as f32 * 2.0));
        }
    }

    fn job_content(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "현재 작업");
        if self.settings.host {
            ui.add(
                egui::Label::new(egui::RichText::new(if self.settings.comp.is_empty() { "프로젝트 준비 전" } else { &self.settings.comp }).strong().size(16.0))
                    .truncate(),
            )
            .on_hover_text(&self.settings.comp);
            let project = std::path::Path::new(&self.settings.project).file_name().unwrap_or_default().to_string_lossy();
            let output = std::path::Path::new(&self.settings.final_video).file_name().unwrap_or_default().to_string_lossy();
            value_row(ui, "프로젝트", if project.is_empty() { "선택 전" } else { &project });
            value_row(ui, "출력 파일", if output.is_empty() { "선택 전" } else { &output });
            value_row(
                ui,
                "렌더 범위",
                &if self.settings.range_end > self.settings.range_start {
                    format!("{}–{} 프레임", self.settings.range_start, self.settings.range_end)
                } else {
                    "선택 전".into()
                },
            );
        } else {
            value_row(ui, "호스트", if self.settings.client_address.is_empty() { "선택 전" } else { &self.settings.client_address });
            caption(ui, "프로젝트와 렌더 설정은 호스트에서 받습니다.");
        }
        value_row(ui, "EffectCraft", self.engine_updater.local.as_deref().unwrap_or("확인 중"));
    }

    fn connection_content(&mut self, ui: &mut egui::Ui) {
        section_title(ui, if self.settings.host { "작업 PC 연결" } else { "호스트 연결" });
        let (connected, selected) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.worker_info.values().filter(|w| w.connected).count(), state.worker_info.values().filter(|w| w.connected && w.enabled).count())
        };
        if self.settings.host {
            ui.columns(2, |columns| {
                metric(&mut columns[0], "연결된 PC", &format!("{connected}대"));
                metric(&mut columns[1], "렌더 참여", &format!("{selected}대"));
            });
            caption(ui, "호스트를 연 뒤 다른 PC에서 주변 호스트를 찾아 연결하세요.");
            caption(ui, "연결한 PC의 렌더 참여를 선택하면 시작할 수 있습니다.");
        } else {
            value_row(ui, "호스트 주소", if self.settings.client_address.is_empty() { "선택 전" } else { &self.settings.client_address });
            caption(ui, "호스트와 같은 네트워크에서 연결하세요.");
            caption(ui, "호스트와 동일한 접속 암호를 입력하세요.");
        }
    }

    fn configuration_content(&self, ui: &mut egui::Ui) {
        section_title(ui, "렌더 설정");
        for (label, value) in [
            ("품질", if self.settings.render.draft { "Draft".into() } else { "Best".into() }),
            ("최적화", if self.settings.render.automatic { "자동".into() } else { "수동".into() }),
            ("오디오", if self.settings.render.audio { "포함".into() } else { "제외".into() }),
            ("미리보기", if self.settings.render.previews { format!("{}초 간격", self.settings.render.preview_seconds) } else { "끄기".into() }),
            ("EffectCraft", self.engine_updater.local.clone().unwrap_or_else(|| "확인 중".into())),
        ] {
            value_row(ui, label, &value);
        }
    }

    fn progress_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        let (done, total, connected, failed) = farm::progress_snapshot(&self.state);
        let (elapsed, local_progress, status) = {
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.started.map(|at| state.finished_elapsed.unwrap_or_else(|| at.elapsed()).as_secs_f64()), state.local_progress, state.final_status.clone())
        };
        ui.horizontal(|ui| {
            section_title(ui, "렌더 진행 상황");
            if failed > 0 {
                badge(ui, &format!("실패 {failed}구간"), egui::Color32::LIGHT_RED);
            }
        });
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
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(if waiting { "대기".into() } else { format!("{:.0}%", progress * 100.0) }).size(36.0).strong());
            ui.vertical(|ui| {
                ui.label(text);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if status.is_empty() { "프로젝트와 PC를 준비한 뒤 렌더링을 시작하세요." } else { &status }).size(12.0).color(MUTED),
                    )
                    .truncate(),
                )
                .on_hover_text(&status);
            });
        });
        let animated = motion(ui.ctx(), ui.id().with("total-progress"), progress, self.settings.reduced_motion);
        ui.add(egui::ProgressBar::new(animated.clamp(0.0, 1.0)).fill(BLUE).desired_height(8.0));
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
                    metric(column, label, value);
                }
            });
        }
    }

    fn preview_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], height: f32) {
        ui.horizontal(|ui| {
            section_title(ui, "프레임 미리보기");
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
        self.dashboard_image(ui, u64::MAX - 1, preview.as_ref(), (height - CONTROL - SPACE * 2.0 - 16.0).max(112.0));
        caption(ui, preview.as_ref().map_or_else(|| "첫 프레임을 받으면 미리보기가 표시됩니다.".into(), |p| format!("프레임 {}", p.frame)));
    }

    pub(super) fn dashboard_image(&mut self, ui: &mut egui::Ui, key: u64, preview: Option<&farm::Preview>, height: f32) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        ui.painter().rect_filled(rect, 8, BACKGROUND);
        if let Some(preview) = preview
            && let Some(texture) = self.preview_texture(ui, key, preview)
        {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
            ui.painter().image(texture.id(), fitted, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        } else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "프레임 대기", egui::FontId::proportional(14.0), MUTED);
        }
    }

    fn workers_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo], height: f32) {
        ui.horizontal(|ui| {
            section_title(ui, "연결된 컴퓨터");
            badge(ui, &format!("{}대", workers.iter().filter(|w| w.connected).count()), GREEN);
        });
        if workers.is_empty() {
            caption(ui, "다른 PC에서 주변 호스트를 찾아 같은 접속 암호로 연결하세요.");
            if ui.button("연결 설정 보기").clicked() {
                self.monitor = false;
            }
            return;
        }
        let columns = ((ui.available_width() + GAP) / 256.0).floor().clamp(1.0, 4.0) as usize;
        let rows = workers.len().div_ceil(columns);
        let grid_height = (height - CONTROL - SPACE).max(rows as f32 * 184.0 + rows.saturating_sub(1) as f32 * GAP);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), grid_height), egui::Sense::hover());
        for (worker, rect) in workers.iter().zip(grid_cells(rect, workers.len(), columns, 184.0)) {
            tile(ui, rect, ("worker", worker.id), true, |ui| self.worker_content(ui, worker, rect.height() - PADDING as f32 * 2.0));
        }
    }

    fn worker_content(&mut self, ui: &mut egui::Ui, worker: &farm::WorkerInfo, height: f32) {
        let top = ui.cursor().top();
        let name = worker.label.split(" @ ").next().unwrap_or(&worker.label);
        ui.add(egui::Label::new(egui::RichText::new(name).strong().size(15.0)).truncate()).on_hover_text(&worker.label);
        ui.horizontal(|ui| {
            let mut enabled = worker.enabled;
            if ui.add_enabled(worker.connected, egui::Checkbox::new(&mut enabled, "렌더 참여")).changed() {
                self.set_worker_participation(worker, enabled);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (status, color) = if !worker.connected {
                    ("연결 끊김", MUTED)
                } else if !worker.enabled {
                    ("제외", MUTED)
                } else if worker.assigned.is_some() {
                    ("렌더링", BLUE)
                } else {
                    ("대기", GREEN)
                };
                badge(ui, status, color);
            });
        });
        let (progress, summary) = if let Some((start, end)) = worker.assigned {
            (worker.current_done as f32 / (end - start).max(1) as f32, format!("{} / {} 프레임 · {:.1} fps", worker.current_done, end - start, worker.fps))
        } else {
            (0.0, format!("누적 {} 프레임", worker.completed_frames))
        };
        let value = motion(ui.ctx(), ui.id().with(("worker-progress", worker.assigned)), progress, self.settings.reduced_motion);
        ui.add(egui::ProgressBar::new(value.clamp(0.0, 1.0)).desired_height(6.0).fill(BLUE));
        ui.add(egui::Label::new(egui::RichText::new(summary).size(12.0).color(MUTED)).truncate());
        if height >= 304.0 {
            self.dashboard_image(ui, worker.id, worker.preview.as_ref(), height - 172.0);
        } else if height >= 224.0 {
            value_row(ui, "누적 완료", &format!("{} 프레임", worker.completed_frames));
            value_row(ui, "수신 데이터", &format!("{:.1} MiB", worker.transferred_bytes as f64 / 1048576.0));
        }
        ui.add_space((top + height - CONTROL - ui.cursor().top()).max(0.0));
        if footer_buttons(ui, None, None, Some(ButtonSpec::new("상세 정보", ButtonKind::Normal, true)), CONTROL).dismiss {
            self.worker_details = Some(worker.id);
        }
    }

    fn activity_content(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        ui.horizontal(|ui| {
            section_title(ui, "작업 기록");
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
                    1 => BLUE,
                    2 => GREEN,
                    3 => egui::Color32::LIGHT_RED,
                    _ => BORDER,
                };
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x(start), rect.top()), egui::pos2(x(end), rect.bottom())), 2, color);
            }
            caption(ui, format!("프레임 {first}–{last} · 파랑 진행 / 초록 완료"));
            ui.separator();
        }
        let messages = farm::snapshot(&self.state).4;
        if messages.is_empty() {
            caption(ui, "연결과 렌더링 활동이 여기에 표시됩니다.");
        }
        for message in messages.iter().rev().filter(|m| !m.contains("[DEBUG]")).take(30) {
            let text = message.splitn(4, "] ").last().unwrap_or(message);
            let color = if message.contains("[ERROR]") { egui::Color32::LIGHT_RED } else { MUTED };
            ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(color)).wrap()).on_hover_text(message);
            ui.separator();
        }
    }
}
