use super::*;

pub(crate) struct ProjectImport {
    pub project: String,
    pub compositions: Vec<CompInfo>,
    pub selected: Option<usize>,
    pub output: String,
    pub range_start: i64,
    pub range_end: i64,
    pub render: settings::RenderOptions,
    pub error: String,
    pending: Option<Receiver<Result<Vec<CompInfo>, String>>>,
}

impl ProjectImport {
    pub fn ready(project: String, mut compositions: Vec<CompInfo>, render: settings::RenderOptions) -> Self {
        compositions.retain(|comp| (1..=100_000).contains(&comp.frames) && comp.width > 0 && comp.height > 0);
        Self { project, compositions, selected: None, output: String::new(), range_start: 0, range_end: 0, render, error: String::new(), pending: None }
    }

    pub fn select(&mut self, index: usize) {
        let Some(comp) = self.compositions.get(index) else { return };
        self.selected = Some(index);
        self.range_end = comp.work_end.clamp(1, comp.frames);
        self.range_start = comp.work_start.clamp(0, self.range_end - 1);
        let project = std::path::Path::new(&self.project);
        let stem = project.file_stem().unwrap_or_default().to_string_lossy();
        let name: String = comp.name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
        let name = name.trim().trim_end_matches('.');
        let base = format!("{stem}-{}", if name.is_empty() { "render" } else { name });
        let parent = project.parent().unwrap_or_else(|| std::path::Path::new("."));
        let mut output = parent.join(format!("{base}.mp4"));
        let mut suffix = 2;
        while output.exists() {
            output = parent.join(format!("{base}-{suffix}.mp4"));
            suffix += 1;
        }
        self.output = output.display().to_string();
        self.error.clear();
    }

    fn poll(&mut self) {
        let result = match self.pending.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err("프로젝트 읽기 작업이 중단됐습니다".into())),
            _ => None,
        };
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(mut compositions) => {
                    compositions.retain(|comp| (1..=100_000).contains(&comp.frames) && comp.width > 0 && comp.height > 0);
                    self.compositions = compositions;
                }
                Err(error) => self.error = error,
            }
        }
    }
}

impl App {
    pub(super) fn choose_project_for_queue(&mut self) {
        if self.running || self.pending_start.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("EffectCraft project", &["ecproj"]).pick_file() {
            self.begin_project_import(path);
        }
    }

    pub(crate) fn begin_project_import(&mut self, path: PathBuf) {
        if self.running || self.pending_start.is_some() {
            return;
        }
        let mut draft = ProjectImport::ready(path.display().to_string(), Vec::new(), self.settings.render.clone());
        let (sender, receiver) = mpsc::channel();
        draft.pending = Some(receiver);
        self.project_import = Some(draft);
        self.queue_detail = None;
        self.queue_edit = None;
        thread::spawn(move || {
            let result = std::panic::catch_unwind(|| farm::list_compositions(&path))
                .unwrap_or_else(|_| Err("프로젝트 분석 중 예상하지 못한 오류가 발생했습니다".into()));
            let _ = sender.send(result);
        });
    }

    pub(crate) fn commit_project_import(&mut self) -> bool {
        let Some(draft) = &self.project_import else { return false };
        let result: Result<crate::queue::Task, String> = (|| {
            if self.running || self.pending_start.is_some() {
                return Err("현재 연결을 종료한 뒤 프로젝트를 추가하세요".into());
            }
            if self.settings.queue.len() >= 200 {
                return Err("큐에는 최대 200개 작업을 등록할 수 있습니다".into());
            }
            if draft.pending.is_some() {
                return Err("프로젝트를 읽는 중입니다".into());
            }
            let comp = draft.selected.and_then(|index| draft.compositions.get(index)).cloned().ok_or("렌더할 컴포지션을 선택하세요")?;
            let mut settings = self.settings.clone();
            settings.project = draft.project.clone();
            settings.comp = comp.name.clone();
            settings.range_comp = comp.name.clone();
            settings.range_start = draft.range_start;
            settings.range_end = draft.range_end;
            settings.final_video = draft.output.clone();
            settings.render = draft.render.clone();
            let id = self.settings.queue.iter().map(|task| task.id).max().unwrap_or(0).checked_add(1).ok_or("작업 ID를 만들 수 없습니다")?;
            let task = crate::queue::Task::capture(id, &settings, comp);
            crate::queue::validate_unique(&task, &self.settings.queue)?;
            Ok(task)
        })();
        match result {
            Ok(task) => {
                self.settings.queue.push(task);
                self.settings.queue_cards = true;
                self.project_import = None;
                self.queue_detail = None;
                self.queue_edit = None;
                self.monitor = true;
                self.queue_view = true;
                self.last_error.clear();
                true
            }
            Err(error) => {
                if let Some(draft) = &mut self.project_import {
                    draft.error = error;
                }
                false
            }
        }
    }

    pub(super) fn project_import_ui(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.project_import.take() else { return };
        draft.poll();
        let mut cancel = false;
        let mut add = false;
        let response = egui::Modal::new(egui::Id::new("project-import")).frame(card(ctx)).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 820.0));
            cancel = dialog_header(ui, "컴포지션 선택", "가져오기 취소");
            caption(ui, "렌더할 컴포지션 하나를 선택해 큐에 추가하세요.");
            ui.add(egui::Label::new(egui::RichText::new(&draft.project).size(12.0).color(muted(ui))).truncate()).on_hover_text(&draft.project);
            ui.separator();
            egui::ScrollArea::vertical().id_salt("project-import-body").max_height((ctx.content_rect().height() - 250.0).clamp(140.0, 560.0)).show(ui, |ui| {
                if draft.pending.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("컴포지션을 읽는 중…");
                    });
                } else if draft.compositions.is_empty() && draft.error.is_empty() {
                    ui.label("렌더 가능한 컴포지션이 없습니다.");
                } else if !draft.compositions.is_empty() {
                    caption(ui, format!("렌더 가능한 컴포지션 {}개", draft.compositions.len()));
                    egui::ScrollArea::vertical().id_salt("import-compositions").max_height(230.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for (index, comp) in draft.compositions.clone().iter().enumerate() {
                            let valid = comp.frames > 0 && comp.width > 0 && comp.height > 0;
                            let label = format!("{}\n{} × {}  ·  {:.3} fps  ·  {} 프레임", comp.name, comp.width, comp.height, comp.rate.as_f64(), comp.frames);
                            let button = egui::Button::new(label).selected(draft.selected == Some(index));
                            if ui.add_enabled(valid, button.min_size(egui::vec2(ui.available_width(), 58.0))).clicked() {
                                draft.select(index);
                            }
                        }
                    });
                    if let Some(comp) = draft.selected.and_then(|index| draft.compositions.get(index)).cloned() {
                        ui.add_space(8.0);
                        ui.separator();
                        section_title(ui, "선택한 컴포지션의 출력");
                        if path_row(ui, "최종 MP4", &mut draft.output)
                            && let Some(path) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).save_file()
                        {
                            draft.output = path.display().to_string();
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label("렌더 범위");
                            ui.add(egui::DragValue::new(&mut draft.range_start).range(0..=draft.range_end.saturating_sub(1)));
                            ui.label("→");
                            ui.add(egui::DragValue::new(&mut draft.range_end).range(draft.range_start + 1..=comp.frames));
                            caption(ui, "끝 프레임 미포함");
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.checkbox(&mut draft.render.draft, "Draft 품질");
                            ui.checkbox(&mut draft.render.audio, "오디오 포함");
                        });
                        caption(ui, "세부 렌더링 속성은 추가된 큐 카드의 설정 아이콘에서 변경할 수 있습니다.");
                    }
                }
                if !draft.error.is_empty() {
                    ui.colored_label(danger(ui), &draft.error);
                }
            });
            ui.separator();
            let ready = draft.pending.is_none() && draft.selected.is_some();
            let clicks = footer_buttons(
                ui,
                None,
                Some(ButtonSpec::new("큐에 추가", ButtonKind::Primary, ready)),
                Some(ButtonSpec::new("취소", ButtonKind::Normal, true)),
                layout::PRIMARY_CONTROL,
            );
            add = clicks.primary;
            cancel |= clicks.dismiss;
        });
        if cancel || response.should_close() {
            return;
        }
        self.project_import = Some(draft);
        if add {
            self.commit_project_import();
        }
        if self.project_import.as_ref().is_some_and(|draft| draft.pending.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}
