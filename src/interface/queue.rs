use super::*;
use crate::queue::{Status, Task};

fn status_color(status: Status) -> egui::Color32 {
    match status {
        Status::Completed => GREEN,
        Status::Failed => egui::Color32::LIGHT_RED,
        Status::Preparing | Status::Waiting | Status::Rendering | Status::Encoding => BLUE,
        _ => MUTED,
    }
}

impl App {
    fn open_queue_task(&mut self, id: u64) {
        self.queue_detail = Some(id);
        self.queue_edit = self.settings.queue.iter().find(|task| task.id == id).cloned();
    }

    pub(super) fn queue_panel(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.horizontal_wrapped(|ui| {
            section_title(ui, "렌더 큐");
            let pending = self.settings.queue.iter().filter(|task| task.status == Status::Pending).count();
            caption(ui, format!("{}개 작업 · 대기 {pending}개", self.settings.queue.len()));
            ui.selectable_value(&mut self.settings.queue_cards, false, "목록");
            ui.selectable_value(&mut self.settings.queue_cards, true, "카드");
        });
        if self.settings.queue.is_empty() {
            ui.add_space(layout::SPACE);
            ui.label("등록된 렌더 작업이 없습니다.");
            caption(ui, "프로젝트와 컴포지션, 출력 파일을 준비한 뒤 렌더 큐에 추가하세요.");
            for (label, ready) in [
                ("프로젝트", !self.settings.project.is_empty()),
                ("컴포지션", !self.settings.comp.is_empty()),
                ("출력 파일", !self.settings.final_video.is_empty()),
            ] {
                layout::value_row(ui, label, if ready { "준비됨" } else { "선택 전" });
            }
            ui.add_space(layout::SPACE);
            if ui.add_sized([136.0, layout::CONTROL], egui::Button::new(if self.running { "프로젝트 화면" } else { "프로젝트 준비" })).clicked() {
                self.monitor = false;
                self.queue_view = false;
            }
            return;
        }
        let tasks = self.settings.queue.clone();
        let cards = self.settings.queue_cards;
        let columns = if cards { ((ui.available_width() + layout::GAP) / 272.0).floor().clamp(1.0, 3.0) as usize } else { 1 };
        let rows = tasks.len().div_ceil(columns);
        let item_height = if cards {
            let smallest_row = (tasks.len() / rows).max(1);
            let widest_cell = (ui.available_width() - layout::GAP * (smallest_row - 1) as f32) / smallest_row as f32;
            let media_height = (widest_cell - layout::PADDING as f32 * 2.0) * 9.0 / 16.0;
            ((media_height + 216.0) / layout::SPACE).ceil() * layout::SPACE
        } else {
            216.0
        };
        let height = (rows as f32 * item_height + rows.saturating_sub(1) as f32 * layout::GAP).max(height - 44.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        for (task, rect) in tasks.iter().zip(layout::grid_cells(rect, tasks.len(), columns, item_height)) {
            layout::tile(ui, rect, ("queue-item", task.id), true, |ui| self.queue_item(ui, task, cards, rect.height() - layout::PADDING as f32 * 2.0));
        }
    }

    fn queue_item(&mut self, ui: &mut egui::Ui, task: &Task, cards: bool, height: f32) {
        let top = ui.cursor().top();
        let active = self.queue_active == Some(task.id);
        ui.horizontal(|ui| {
            // Reserve enough room for long status labels such as “MP4 출력 중”.
            let width = (ui.available_width() - 120.0 - ui.spacing().item_spacing.x).max(1.0);
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, layout::CONTROL), egui::Sense::click());
            let mut title = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
            title.set_clip_rect(rect.intersect(ui.clip_rect()));
            title.add(egui::Label::new(egui::RichText::new(&task.composition.name).strong()).truncate());
            if response.on_hover_text(&task.composition.name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.open_queue_task(task.id);
            }
            badge(ui, task.status.label(), status_color(task.status));
        });
        let name = std::path::Path::new(&task.project).file_name().unwrap_or_default().to_string_lossy();
        ui.add(egui::Label::new(egui::RichText::new(name).size(12.0).color(MUTED)).truncate()).on_hover_text(&task.project);
        if cards {
            let preview = self.queue_previews.get(&task.id).cloned();
            self.dashboard_image(ui, u64::MAX / 2 + task.id, preview.as_ref(), (height - 184.0).max(128.0));
        }
        let total = (task.range_end - task.range_start).max(1);
        ui.add(egui::ProgressBar::new((task.done as f32 / total as f32).clamp(0.0, 1.0)).desired_height(6.0));
        caption(ui, format!("{} / {total} 프레임", task.done));
        let detail = if task.detail.is_empty() {
            format!("{} × {} · {}–{} 프레임", task.composition.width, task.composition.height, task.range_start, task.range_end)
        } else {
            task.detail.clone()
        };
        ui.add(egui::Label::new(egui::RichText::new(&detail).size(12.0).color(MUTED)).truncate()).on_hover_text(&detail);
        ui.add_space((top + height - layout::CONTROL - ui.cursor().top()).max(0.0));
        let (footer, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), layout::CONTROL), egui::Sense::hover());
        let movable = task.status == Status::Pending && !active;
        let index = self.settings.queue.iter().position(|entry| entry.id == task.id).unwrap_or(0);
        let up = egui::Rect::from_min_size(footer.min, egui::Vec2::splat(layout::CONTROL));
        let down = up.translate(egui::vec2(layout::CONTROL + 4.0, 0.0));
        if button_at(ui, up, "queue-up", ButtonSpec::new("↑", ButtonKind::Quiet, movable && index > 0 && !self.settings.queue[index - 1].status.in_progress()))
            .on_hover_text("순서 올리기")
            .clicked()
        {
            self.settings.queue.swap(index, index - 1);
        }
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
        let details = egui::Rect::from_min_size(egui::pos2(footer.right() - 96.0, footer.top()), egui::vec2(96.0, layout::CONTROL));
        if button_at(ui, details, "queue-details", ButtonSpec::new("상세 설정", ButtonKind::Normal, true)).clicked() {
            self.open_queue_task(task.id);
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
        let response = egui::Modal::new(egui::Id::new("queue-detail")).frame(card()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(280.0, 650.0));
            close = dialog_header(ui, &live.composition.name, "닫기");
            badge(ui, live.status.label(), status_color(live.status));
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
                ui.add(egui::ProgressBar::new((live.done as f32 / total as f32).clamp(0.0, 1.0)).text(format!("{} / {total} 프레임", live.done)));
                ui.label(&live.detail);
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
                    ui.colored_label(egui::Color32::LIGHT_RED, &self.last_error);
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
