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
    preview: Option<farm::Preview>,
    preview_pending: Option<Receiver<Result<farm::Preview, String>>>,
}

impl ProjectImport {
    pub fn ready(project: String, mut compositions: Vec<CompInfo>, render: settings::RenderOptions) -> Self {
        compositions.retain(|comp| (1..=100_000).contains(&comp.frames) && comp.width > 0 && comp.height > 0);
        Self {
            project,
            compositions,
            selected: None,
            output: String::new(),
            range_start: 0,
            range_end: 0,
            render,
            error: String::new(),
            pending: None,
            preview: None,
            preview_pending: None,
        }
    }

    pub fn select(&mut self, index: usize) {
        let Some(comp) = self.compositions.get(index) else { return };
        self.selected = Some(index);
        self.preview = None;
        self.preview_pending = None;
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
        let preview = match self.preview_pending.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err("미리보기 작업이 중단됐습니다".into())),
            _ => None,
        };
        if let Some(result) = preview {
            self.preview_pending = None;
            match result {
                Ok(preview) => self.preview = Some(preview),
                Err(error) => self.error = error,
            }
        }
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
        let mut render_preview = false;
        let tab_id = egui::Id::new("import-tab");
        let mut tab = ctx.data(|data| data.get_temp::<usize>(tab_id).unwrap_or(0));
        let frame = card(ctx);
        let (modal, reveal) = motion::opening_modal(ctx, egui::Id::new("project-import"), self.settings.reduced_motion, frame);
        let response = modal.show(ctx, |ui| {
            ui.multiply_opacity(reveal);
            ui.set_width((ctx.content_rect().width() - 112.0).clamp(280.0, 752.0));
            cancel |= dialog_header(ui, "대기열 추가", "대기열 추가 닫기");
            caption(ui, "컴포지션을 선택하고 렌더링 설정을 확인하세요.");
            ui.add_space(4.0);
            import_tabs(ui, &mut tab);
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("project-import-body").max_height((ctx.content_rect().height() - 250.0).clamp(150.0, 510.0)).show(ui, |ui| {
                if tab == 0 {
                    let width = ui.available_width();
                    ui.spacing_mut().item_spacing.x = layout::GAP;
                    let height = ((width - layout::GAP) * 0.5 * 9.0 / 16.0).clamp(95.0, 208.0);
                    ui.columns(2, |columns| {
                        let ui = &mut columns[0];
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
                        ui.painter().rect_filled(rect, layout::INSET_RADIUS, secondary(ui));
                        if let Some(preview) = &draft.preview {
                            if let Some(texture) = self.preview_texture(ui, u64::MAX - 1, preview) {
                                let image = egui::Image::new((texture.id(), texture.size_vec2()))
                                    .max_size(rect.size())
                                    .maintain_aspect_ratio(true)
                                    .corner_radius(layout::INSET_RADIUS);
                                image.paint_at(ui, rect);
                            }
                        } else if draft.preview_pending.is_some() {
                            let mut child =
                                ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)));
                            motion::busy_indicator(&mut child);
                        } else {
                            let button_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(128.0, layout::CONTROL));
                            render_preview =
                                button_at(ui, button_rect, "import-preview", ButtonSpec::new("미리보기 렌더", ButtonKind::Normal, draft.selected.is_some()))
                                    .clicked();
                        }
                        let ui = &mut columns[1];
                        ui.label(egui::RichText::new("렌더할 컴포지션").size(14.0));
                        if draft.pending.is_some() {
                            ui.horizontal(|ui| {
                                motion::busy_indicator(ui);
                                ui.label("컴포지션을 읽는 중…");
                            });
                        } else if draft.compositions.is_empty() {
                            caption(ui, "렌더 가능한 컴포지션이 없습니다.");
                        } else {
                            let selected = draft.selected.and_then(|i| draft.compositions.get(i)).map_or("선택하세요", |comp| comp.name.as_str()).to_owned();
                            let mut chosen = None;
                            ui.scope(|ui| {
                                let field_fill = surface(ui);
                                let visuals = &mut ui.style_mut().visuals.widgets;
                                visuals.inactive.bg_fill = field_fill;
                                visuals.inactive.weak_bg_fill = field_fill;
                                visuals.hovered.bg_fill = field_fill;
                                visuals.hovered.weak_bg_fill = field_fill;
                                egui::ComboBox::from_id_salt("import-composition").width(ui.available_width()).selected_text(selected).show_ui(ui, |ui| {
                                    for (index, comp) in draft.compositions.iter().enumerate() {
                                        if ui.selectable_label(draft.selected == Some(index), &comp.name).clicked() {
                                            chosen = Some(index);
                                        }
                                    }
                                });
                            });
                            if let Some(index) = chosen {
                                draft.select(index);
                            }
                        }
                    });
                    ui.add_space(4.0);
                    let comp = draft.selected.and_then(|index| draft.compositions.get(index)).cloned();
                    import_range(ui, &mut draft, comp.as_ref(), &mut self.range_handle);
                    ui.horizontal(|ui| {
                        let fps = comp.as_ref().map_or(1.0, |comp| comp.rate.as_f64()).max(0.001);
                        ui.label(egui::RichText::new(import_time(draft.range_start, fps)).size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new(import_time(comp.as_ref().map_or(0, |c| c.frames), fps)).size(12.0));
                        });
                    });
                } else {
                    ui.add(egui::Label::new(egui::RichText::new(&draft.project).size(12.0).color(muted(ui))).truncate()).on_hover_text(&draft.project);
                    if let Some(comp) = draft.selected.and_then(|index| draft.compositions.get(index)).cloned() {
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
                    } else {
                        caption(ui, "컴포지션을 먼저 선택하세요.");
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
                Some(ButtonSpec::new("대기열에 추가", ButtonKind::Primary, ready)),
                Some(ButtonSpec::new("취소", ButtonKind::Normal, true)),
                layout::PRIMARY_CONTROL,
            );
            add = clicks.primary;
            cancel |= clicks.dismiss;
        });
        ctx.data_mut(|data| data.insert_temp(tab_id, tab));
        if cancel || response.should_close() {
            ctx.data_mut(|data| data.remove::<usize>(tab_id));
            return;
        }
        if render_preview && let Some(comp) = draft.selected.and_then(|index| draft.compositions.get(index)) {
            let mut config = self.config();
            config.project = PathBuf::from(&draft.project);
            config.comp = comp.name.clone();
            config.range_start = draft.range_start;
            config.range_end = draft.range_end;
            config.render = draft.render.clone();
            let frame = draft.range_start;
            let (tx, rx) = mpsc::channel();
            draft.preview_pending = Some(rx);
            draft.preview = None;
            draft.error.clear();
            thread::spawn(move || {
                let _ = tx.send(farm::render_preview(&config, frame));
            });
        }
        self.project_import = Some(draft);
        if add && self.commit_project_import() {
            ctx.data_mut(|data| data.remove::<usize>(tab_id));
        }
        if self.project_import.as_ref().is_some_and(|draft| draft.pending.is_some() || draft.preview_pending.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
}

fn import_time(frame: i64, fps: f64) -> String {
    let seconds = (frame.max(0) as f64 / fps).floor() as u64;
    format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
}

fn import_tabs(ui: &mut egui::Ui, selected: &mut usize) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, layout::INSET_RADIUS, secondary(ui));
    let width = (rect.width() - 8.0) / 2.0;
    for (index, label, name, bytes) in [
        (0, "컴포지션 선택", "import-timeline", &include_bytes!("../../assets/ui/figma-import/timeline.svg")[..]),
        (1, "렌더링 설정", "import-wrench", &include_bytes!("../../assets/ui/figma-import/wrench.svg")[..]),
    ] {
        let bounds = egui::Rect::from_min_size(rect.min + egui::vec2(4.0 + width * index as f32, 4.0), egui::vec2(width, 32.0));
        let response = ui.interact(bounds, ui.id().with(("import-tab", index)), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), *selected == index, label));
        let hover = motion::highlight(ui, response.id.with("hover"), response.hovered());
        let selected_fill = motion::highlight(ui, response.id.with("selected"), *selected == index);
        let fill = ui.visuals().widgets.hovered.bg_fill.gamma_multiply(hover).lerp_to_gamma(surface(ui), selected_fill);
        ui.painter().rect_filled(bounds, layout::CONTROL_RADIUS, fill);
        let color = if *selected == index { ui.visuals().text_color() } else { muted(ui) };
        let galley = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(14.0), color);
        let total = galley.size().x + 22.0;
        let left = bounds.center().x - total / 2.0;
        if let Some(icon) = figma_svg(ui, name, bytes, egui::Vec2::splat(16.0)) {
            icon.paint_at(ui, egui::Rect::from_min_size(egui::pos2(left, bounds.center().y - 8.0), egui::Vec2::splat(16.0)));
        }
        ui.painter().galley(egui::pos2(left + 22.0, bounds.center().y - galley.size().y / 2.0), galley, color);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *selected = index;
        }
    }
}

fn import_range(ui: &mut egui::Ui, draft: &mut ProjectImport, comp: Option<&CompInfo>, handle: &mut Option<RangeHandle>) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::click_and_drag());
    ui.painter().rect_filled(rect, 6, secondary(ui));
    let Some(comp) = comp else { return };
    let total = comp.frames.max(1);
    let x = |frame: i64| rect.left() + rect.width() * frame.clamp(0, total) as f32 / total as f32;
    let start = x(draft.range_start);
    let end = x(draft.range_end);
    let selected = egui::Rect::from_min_max(egui::pos2(start, rect.top()), egui::pos2(end, rect.bottom()));
    ui.painter().rect_filled(selected, 4, layout::PRIMARY_FILL);
    for center in [start, end] {
        let grip = egui::Rect::from_center_size(egui::pos2(center, rect.center().y), egui::vec2(6.0, 15.0));
        ui.painter().rect_filled(grip, 3, accent(ui));
        ui.painter().rect_stroke(grip, 3, egui::Stroke::new(1.0, layout::ON_PRIMARY), egui::StrokeKind::Inside);
    }
    if (response.drag_started() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *handle = Some(if (pointer.x - start).abs() <= (pointer.x - end).abs() { RangeHandle::Start } else { RangeHandle::End });
    }
    if (response.dragged() || response.clicked())
        && let (Some(pointer), Some(which)) = (response.interact_pointer_pos(), *handle)
    {
        let frame = (((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * total as f32).round() as i64;
        match which {
            RangeHandle::Start => draft.range_start = frame.clamp(0, draft.range_end.saturating_sub(1)),
            RangeHandle::End => draft.range_end = frame.clamp(draft.range_start + 1, total),
        }
    }
    if response.drag_stopped() || response.clicked() {
        *handle = None;
    }
}
