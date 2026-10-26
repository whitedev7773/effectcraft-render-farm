use super::*;
use crate::queue::{Status, Task};

fn status_color(status: Status) -> egui::Color32 {
    match status {
        Status::Completed => GREEN,
        Status::Failed => egui::Color32::LIGHT_RED,
        Status::Preparing | Status::Rendering | Status::Encoding => BLUE,
        _ => MUTED,
    }
}

impl App {
    fn open_queue_task(&mut self, id: u64) {
        self.queue_detail = Some(id);
        self.queue_edit = self.settings.queue.iter().find(|task| task.id == id).cloned();
    }

    pub(super) fn queue_panel(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "렌더 큐");
                let pending = self.settings.queue.iter().filter(|task| task.status == Status::Pending).count();
                caption(ui, format!("{}개 작업 · 대기 {pending}개", self.settings.queue.len()));
                ui.selectable_value(&mut self.settings.queue_cards, false, "리스트");
                ui.selectable_value(&mut self.settings.queue_cards, true, "카드");
            });
            if self.settings.queue.is_empty() {
                ui.add_space(12.0);
                ui.label("프로젝트와 컴포지션을 선택해 렌더 큐에 추가하세요.");
                caption(ui, "작업마다 별도 설정을 저장하며, 최종 MP4 출력 후 다음 작업으로 진행합니다.");
                return;
            }
            caption(ui, "항목을 선택하면 렌더 설정·진행 상태·프레임을 확인할 수 있습니다.");
            let tasks = self.settings.queue.clone();
            let columns = if self.settings.queue_cards { ((ui.available_width() + 12.0) / 250.0).floor().max(1.0) as usize } else { 1 };
            egui::ScrollArea::vertical().id_salt("render-queue").max_height(if self.settings.queue_cards { 370.0 } else { 270.0 }).show(ui, |ui| {
                if self.settings.queue_cards {
                    // Reserve a few pixels for fractional column rounding and the scroll bar.
                    ui.set_width((ui.available_width() - 8.0).max(1.0));
                    for row in tasks.chunks(columns) {
                        ui.columns(columns, |columns| {
                            for (column, task) in columns.iter_mut().zip(row) {
                                column.push_id(task.id, |ui| self.queue_item(ui, task, true));
                            }
                        });
                    }
                } else {
                    for task in &tasks {
                        ui.push_id(task.id, |ui| self.queue_item(ui, task, false));
                    }
                }
            });
            if self.queue_automatic {
                caption(ui, "큐 자동 진행 중 · 오류가 발생하면 큐를 멈춥니다.");
            }
        });
    }

    fn queue_item(&mut self, ui: &mut egui::Ui, task: &Task, cards: bool) {
        let active = self.queue_active == Some(task.id);
        let frame =
            egui::Frame::new().fill(SURFACE_SECONDARY).stroke(egui::Stroke::new(1.0, if active { BLUE } else { BORDER })).corner_radius(12).inner_margin(12);
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let title = format!("{}  ·  {}", task.composition.name, task.status.label());
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(title).color(if active { egui::Color32::WHITE } else { status_color(task.status) }))
                        .wrap()
                        .selected(active),
                )
                .clicked()
            {
                self.open_queue_task(task.id);
            }
            let name = std::path::Path::new(&task.project).file_name().unwrap_or_default().to_string_lossy();
            ui.add(egui::Label::new(egui::RichText::new(name).size(12.0).color(MUTED)).truncate()).on_hover_text(&task.project);
            if cards {
                if let Some(preview) = self.queue_previews.get(&task.id).cloned() {
                    self.show_preview_sized(ui, u64::MAX / 2 + task.id, &preview, 135.0);
                } else {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 112.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 8.0, BACKGROUND);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if task.status.in_progress() { "첫 프레임 수신 대기" } else { "프레임 미리보기 대기" },
                        egui::FontId::proportional(12.0),
                        MUTED,
                    );
                }
            }
            let total = (task.range_end - task.range_start).max(1);
            ui.add(
                egui::ProgressBar::new((task.done as f32 / total as f32).clamp(0.0, 1.0))
                    .desired_width(ui.available_width())
                    .text(format!("{} / {total} 프레임", task.done)),
            );
            if !task.detail.is_empty() {
                ui.add(egui::Label::new(egui::RichText::new(&task.detail).size(12.0).color(MUTED)).truncate()).on_hover_text(&task.detail);
            }
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("상세 설정").clicked() {
                    self.open_queue_task(task.id);
                }
                let movable = task.status == Status::Pending && !active;
                let index = self.settings.queue.iter().position(|entry| entry.id == task.id).unwrap_or(0);
                if ui
                    .add_enabled(movable && index > 0 && !self.settings.queue[index - 1].status.in_progress(), egui::Button::new("↑").small())
                    .on_hover_text("순서 올리기")
                    .clicked()
                {
                    self.settings.queue.swap(index, index - 1);
                }
                if ui
                    .add_enabled(
                        movable && index + 1 < self.settings.queue.len() && !self.settings.queue.get(index + 1).is_some_and(|entry| entry.status.in_progress()),
                        egui::Button::new("↓").small(),
                    )
                    .on_hover_text("순서 내리기")
                    .clicked()
                {
                    self.settings.queue.swap(index, index + 1);
                }
            });
        });
        ui.add_space(6.0);
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
        let response = egui::Modal::new(egui::Id::new("queue-detail")).frame(card()).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(280.0, 650.0));
            ui.horizontal_wrapped(|ui| {
                section_title(ui, &live.composition.name);
                ui.colored_label(status_color(live.status), live.status.label());
                close = icon_button(ui, UiIcon::Close, "닫기").clicked();
            });
            egui::ScrollArea::vertical().id_salt("queue-detail-scroll").max_height((ctx.content_rect().height() - 200.0).max(180.0)).show(ui, |ui| {
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
                    if path_row(ui, "프레임 저장 폴더", &mut edit.output)
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        edit.output = path.display().to_string();
                    }
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
                        ui.add(egui::DragValue::new(&mut edit.render.preview_seconds).range(1..=30).suffix("초 간격"));
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
            });
            ui.horizontal_wrapped(|ui| {
                save = ui.add_enabled(editable, primary_button("설정 저장")).clicked();
                retry = ui.add_enabled(editable && matches!(live.status, Status::Failed | Status::Interrupted), egui::Button::new("저장하고 재대기")).clicked();
                remove = ui.add_enabled(!live.status.in_progress() && self.queue_active != Some(id), egui::Button::new("큐에서 제거")).clicked();
            });
            if !self.last_error.is_empty() {
                ui.colored_label(egui::Color32::LIGHT_RED, &self.last_error);
            }
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
                }
                Err(error) => self.last_error = error,
            }
        }
        self.queue_edit = Some(edit);
        if remove {
            self.settings.queue.retain(|task| task.id != id);
            self.queue_previews.remove(&id);
        }
        if response.should_close() || close || remove {
            self.queue_detail = None;
            self.queue_edit = None;
        }
    }
}
