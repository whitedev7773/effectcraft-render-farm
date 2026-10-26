use super::*;
mod queue;

#[cfg(any(test, debug_assertions))]
pub(super) mod review;

// HeroUI v3 default dark theme, converted from its OKLCH design tokens.
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(6, 6, 7);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(24, 24, 27);
const SURFACE_SECONDARY: egui::Color32 = egui::Color32::from_rgb(35, 35, 37);
const BORDER: egui::Color32 = egui::Color32::from_rgb(40, 40, 44);
const MUTED: egui::Color32 = egui::Color32::from_rgb(159, 159, 169);
const BLUE: egui::Color32 = egui::Color32::from_rgb(4, 133, 247);
const GREEN: egui::Color32 = egui::Color32::from_rgb(76, 214, 124);

#[derive(Clone, Copy)]
enum UiIcon {
    Settings,
    History,
    Update,
    Copy,
    Close,
}

impl UiIcon {
    fn asset(self) -> (&'static str, &'static [u8]) {
        match self {
            Self::Settings => ("settings", include_bytes!("../assets/ui/icons/gear.png")),
            Self::History => ("history", include_bytes!("../assets/ui/icons/clock-arrow-rotate-left.png")),
            Self::Update => ("update", include_bytes!("../assets/ui/icons/arrow-down-to-line.png")),
            Self::Copy => ("copy", include_bytes!("../assets/ui/icons/copy.png")),
            Self::Close => ("close", include_bytes!("../assets/ui/icons/xmark.png")),
        }
    }
}

fn icon_image(ui: &egui::Ui, icon: UiIcon) -> egui::Image<'static> {
    let (name, bytes) = icon.asset();
    let id = egui::Id::new(("gravity-icon", name));
    let texture = if let Some(texture) = ui.ctx().data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        texture
    } else {
        let rgba = image::load_from_memory(bytes).unwrap_or_else(|_| image::DynamicImage::new_rgba8(40, 40)).into_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
        let texture = ui.ctx().load_texture(format!("gravity-{name}"), color, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    };
    egui::Image::new((texture.id(), texture.size_vec2())).fit_to_exact_size(egui::vec2(18.0, 18.0))
}

fn icon_button(ui: &mut egui::Ui, icon: UiIcon, label: &str) -> egui::Response {
    ui.add(egui::Button::image(icon_image(ui, icon)).min_size(egui::vec2(36.0, 34.0))).on_hover_text(label)
}

fn card() -> egui::Frame {
    egui::Frame::new().fill(SURFACE).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(20).inner_margin(16)
}

fn animated_modal(ctx: &egui::Context, id: &str, open: bool, reduced: bool) -> Option<(egui::Modal, f32)> {
    let reveal = motion(ctx, egui::Id::new((id, "reveal")), f32::from(open), reduced);
    if reveal < 0.001 {
        return None;
    }
    let area = egui::Modal::default_area(egui::Id::new(id)).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, (1.0 - reveal) * 14.0));
    let frame = card().fill(SURFACE.gamma_multiply(reveal)).stroke(egui::Stroke::new(1.0, BORDER.gamma_multiply(reveal)));
    Some((egui::Modal::new(egui::Id::new(id)).area(area).frame(frame).backdrop_color(egui::Color32::from_black_alpha((160.0 * reveal) as u8)), reveal))
}

fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).size(12.0).color(MUTED));
}

fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    if seconds >= 3600 {
        format!("{}시간 {}분", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{}분 {}초", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}초")
    }
}

#[derive(Clone, Copy, Default)]
struct Spring {
    value: f32,
    velocity: f32,
}

/// Closed-form critically damped spring: continuous velocity on retarget, no overshoot.
fn motion(ctx: &egui::Context, id: egui::Id, target: f32, reduced: bool) -> f32 {
    if reduced {
        ctx.data_mut(|data| data.insert_temp(id, Spring { value: target, velocity: 0.0 }));
        return target;
    }
    let dt = ctx.input(|input| input.stable_dt).clamp(0.001, 0.05);
    let mut spring = ctx.data_mut(|data| data.get_temp::<Spring>(id).unwrap_or_default());
    let displacement = spring.value - target;
    let impulse = spring.velocity + 22.0 * displacement;
    let decay = (-22.0 * dt).exp();
    spring.value = target + (displacement + impulse * dt) * decay;
    spring.velocity = (spring.velocity - 22.0 * impulse * dt) * decay;
    if (spring.value - target).abs() > 0.001 || spring.velocity.abs() > 0.001 {
        ctx.request_repaint_after(Duration::from_millis(16));
    } else {
        spring = Spring { value: target, velocity: 0.0 };
    }
    ctx.data_mut(|data| data.insert_temp(id, spring));
    spring.value
}

fn metric(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        caption(ui, label);
        ui.label(egui::RichText::new(value).size(24.0).strong());
    });
}

impl App {
    pub(super) fn draw_interface(&mut self, root: &mut egui::Ui) {
        if self.settings.host && self.settings.output.trim().is_empty() {
            let final_path = PathBuf::from(&self.settings.final_video);
            if final_path.is_absolute()
                && let (Some(parent), Some(stem)) = (final_path.parent(), final_path.file_stem())
            {
                self.settings.output = parent.join(format!("{}.frames", stem.to_string_lossy())).display().to_string();
            }
        }
        let ctx = root.ctx().clone();
        ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = if self.settings.reduced_motion { 0.0 } else { 0.24 });
        egui::Panel::bottom("primary-actions")
            .frame(egui::Frame::new().fill(SURFACE).stroke(egui::Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(20, 10)))
            .show(root, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.action_bar(ui);
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BACKGROUND).inner_margin(20)).show(root, |ui| {
            let wide = ui.available_width() >= 1120.0;
            self.toolbar(ui, wide);
            ui.add_space(14.0);
            if !self.last_error.is_empty() {
                egui::Frame::new().fill(egui::Color32::from_rgb(58, 31, 36)).corner_radius(10).inner_margin(12).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(egui::Color32::LIGHT_RED, &self.last_error);
                        if ui.small_button("닫기").clicked() {
                            self.last_error.clear();
                        }
                    });
                });
                ui.add_space(10.0);
            }
            if !wide {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.monitor, false, "프로젝트 · 연결");
                    ui.selectable_value(&mut self.monitor, true, "렌더 모니터");
                });
                ui.add_space(12.0);
            }
            if wide {
                let left = (ui.available_width() * 0.29).clamp(320.0, 420.0);
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                let left_rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + left, rect.bottom()));
                let right_rect = egui::Rect::from_min_max(egui::pos2(left_rect.right() + 20.0, rect.top()), rect.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(left_rect), |ui| {
                    ui.set_width(left_rect.width());
                    ui.set_clip_rect(left_rect.intersect(ui.clip_rect()));
                    egui::ScrollArea::vertical().id_salt("setup-scroll").auto_shrink([false, false]).show(ui, |ui| {
                        self.setup_panel(ui);
                        ui.add_space(14.0);
                        self.session_panel(ui);
                        ui.add_space(14.0);
                        self.configuration_panel(ui);
                    });
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(right_rect), |ui| {
                    ui.set_width(right_rect.width());
                    ui.set_clip_rect(right_rect.intersect(ui.clip_rect()));
                    egui::ScrollArea::vertical().id_salt("monitor-scroll").auto_shrink([false, false]).show(ui, |ui| {
                        if self.settings.host {
                            self.queue_panel(ui);
                            ui.add_space(14.0);
                        }
                        self.monitor_panel(ui);
                    });
                });
            } else {
                let page_id = egui::Id::new("active-compact-page");
                if ui.ctx().data_mut(|data| data.get_temp::<bool>(page_id)) != Some(self.monitor) {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(page_id, self.monitor);
                        data.insert_temp(egui::Id::new(("page-reveal", self.monitor)), Spring::default());
                    });
                }
                egui::ScrollArea::vertical().id_salt(("compact", self.monitor)).auto_shrink([false, false]).show(ui, |ui| {
                    let reveal = motion(ui.ctx(), egui::Id::new(("page-reveal", self.monitor)), 1.0, self.settings.reduced_motion);
                    ui.set_opacity(reveal);
                    if self.settings.host {
                        self.queue_panel(ui);
                        ui.add_space(14.0);
                    }
                    if self.monitor {
                        self.monitor_panel(ui);
                    } else {
                        self.setup_panel(ui);
                    }
                });
            }
        });
        self.settings_dialog(&ctx);
        self.logs_dialog(&ctx);
        self.worker_dialog(&ctx);
        self.queue_dialog(&ctx);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, wide: bool) {
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Render Farm").size(26.0).strong());
            });
            if wide {
                ui.add_space(24.0);
            }
            let (label, color) = if self.stopping {
                ("중지 중", MUTED)
            } else if self.pending_start.is_some() {
                ("준비 중", BLUE)
            } else if self.running {
                (if self.settings.host { "호스트 연결됨" } else { "작업 PC 연결됨" }, GREEN)
            } else {
                ("연결 대기", MUTED)
            };
            ui.colored_label(color, format!("● {label}"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, UiIcon::Settings, "설정").clicked() {
                    self.settings_open = true;
                }
                if icon_button(ui, UiIcon::History, "작업 기록").clicked() {
                    self.logs_open = true;
                }
                if self.updater.available.is_some() && icon_button(ui, UiIcon::Update, "업데이트").clicked() {
                    self.updater.prompt = true;
                }
            });
        });
        if self.updater.installing() {
            caption(ui, format!("업데이트 준비 중 · {:.1} MiB", self.updater.progress.load(Ordering::Relaxed) as f64 / 1048576.0));
        }
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        if self.pending_start.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("프로젝트와 소스를 준비하고 있습니다…");
            });
        } else if !self.running {
            if self.settings.host && !self.settings.queue.is_empty() {
                let ready = self.settings.queue.iter().any(|task| task.status == crate::queue::Status::Pending)
                    && !self.updater.installing()
                    && self.pending_preview.is_none()
                    && !self.worker_active.load(Ordering::Relaxed);
                if ui.add_enabled(ready, primary_button("큐 시작").min_size(egui::vec2(130.0, 42.0))).clicked() {
                    self.queue_automatic = true;
                    self.start_next_queued();
                }
                return;
            }
            let enabled = !self.updater.installing() && self.pending_preview.is_none() && (!self.settings.host || self.missing_host_settings().is_empty());
            let response = ui.add_enabled(
                enabled,
                primary_button(if self.settings.host { "호스트 열기" } else { "호스트에 연결" }).min_size(egui::vec2(ui.available_width().min(200.0), 42.0)),
            );
            if response.clicked() {
                self.start();
            }
            if !enabled && self.settings.host {
                caption(ui, format!("다음 항목을 준비하세요: {}", self.missing_host_settings().join(", ")));
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                let started = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
                if self.settings.host
                    && !started
                    && !self.stopping
                    && ui.add(primary_button("렌더 시작").min_size(egui::vec2(140.0, 40.0))).clicked()
                    && let Some(job) = &self.job
                {
                    farm::begin_render(&self.config(), job, &self.state);
                }
                if ui.add_enabled(!self.stopping, egui::Button::new("연결 종료").min_size(egui::vec2(100.0, 40.0))).clicked() {
                    self.queue_automatic = false;
                    self.stop.store(true, Ordering::Relaxed);
                    self.stopping = true;
                }
            });
            if self.stopping {
                caption(ui, "진행 중인 구간이 끝나면 연결을 종료합니다.");
            }
        }
    }

    fn setup_panel(&mut self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, if self.settings.host { "프로젝트" } else { "호스트 연결" });
            ui.add_space(8.0);
            ui.add_enabled_ui(!self.running && self.pending_start.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings.host, true, "호스트");
                    ui.selectable_value(&mut self.settings.host, false, "작업 PC");
                });
                ui.add_space(8.0);
                if self.settings.host {
                    if path_row(ui, "프로젝트", &mut self.settings.project)
                        && let Some(path) = rfd::FileDialog::new().add_filter("EffectCraft project", &["ecproj"]).pick_file()
                    {
                        self.settings.project = path.display().to_string();
                    }
                    ui.add_space(6.0);
                    ui.label("컴포지션");
                    let previous = self.settings.comp.clone();
                    egui::ComboBox::from_id_salt("composition")
                        .width(ui.available_width())
                        .selected_text(if self.settings.comp.is_empty() { "컴포지션 선택" } else { &self.settings.comp })
                        .show_ui(ui, |ui| {
                            for comp in &self.catalog {
                                ui.selectable_value(&mut self.settings.comp, comp.name.clone(), &comp.name);
                            }
                        });
                    if previous != self.settings.comp {
                        self.sync_selected_comp_range();
                    }
                    if let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp) {
                        caption(ui, format!("{} × {}  ·  {:.3} fps", comp.width, comp.height, comp.rate.as_f64()));
                    } else {
                        caption(ui, &self.catalog_status);
                    }
                    ui.add_space(12.0);
                    self.range_controls(ui);
                    ui.add_space(12.0);
                    let previous_video = self.settings.final_video.clone();
                    if path_row(ui, "최종 MP4", &mut self.settings.final_video)
                        && let Some(path) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).save_file()
                    {
                        self.settings.final_video = path.display().to_string();
                    }
                    if previous_video != self.settings.final_video {
                        let previous_frames = PathBuf::from(previous_video).with_extension("frames");
                        if self.settings.output.is_empty() || std::path::Path::new(&self.settings.output) == previous_frames {
                            self.settings.output = PathBuf::from(&self.settings.final_video).with_extension("frames").display().to_string();
                        }
                    }
                    ui.add_space(8.0);
                    if ui.button("+ 렌더 큐에 추가").clicked() {
                        self.enqueue_current();
                    }
                    caption(ui, "프로젝트·컴포지션과 출력 위치를 바꿔 여러 작업을 추가하세요.");
                } else {
                    text_row(ui, "호스트 주소", &mut self.settings.client_address, false);
                    caption(ui, "예: 192.168.0.10:9778");
                    if ui.button("주변 호스트 찾기").clicked() && self.pending_discovery.is_none() {
                        self.start_discovery();
                    }
                    for host in &self.discovered_hosts {
                        let address = host.address.to_string();
                        if ui.selectable_label(self.settings.client_address == address, format!("{} · {address}", host.name)).clicked() {
                            self.settings.client_address = address;
                        }
                    }
                    caption(ui, &self.discovery_status);
                    ui.add_space(10.0);
                }
                ui.add_space(8.0);
                text_row(ui, "접속 암호 · 8자 이상", &mut self.settings.token, true);
            });
            ui.add_space(12.0);
            ui.separator();
            if self.settings.host {
                caption(
                    ui,
                    format!(
                        "{} · {}",
                        if self.settings.render.automatic { "자동 최적화" } else { "수동 설정" },
                        if self.settings.render.draft { "Draft" } else { "Best 품질" }
                    ),
                );
            } else {
                caption(ui, "렌더 설정: 호스트 기준");
            }
            if self.running && self.settings.host {
                caption(ui, format!("연결 포트: {}", self.settings.host_address));
            }
        });
    }

    fn session_panel(&self, ui: &mut egui::Ui) {
        let (done, total, connected, failed) = farm::progress_snapshot(&self.state);
        card().fill(SURFACE_SECONDARY).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "세션 개요");
            caption(ui, if self.running { "현재 연결 및 렌더 상태" } else { "연결을 시작하면 상태가 여기에 표시됩니다" });
            ui.add_space(8.0);
            ui.columns(2, |columns| {
                metric(&mut columns[0], "연결된 작업 PC", &format!("{connected}대"));
                metric(&mut columns[1], "완료 프레임", &format!("{done} / {total}"));
            });
            if failed > 0 {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("실패한 구간 {failed}개 · 작업 기록 확인"));
            }
        });
    }

    fn configuration_panel(&self, ui: &mut egui::Ui) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "작업 구성");
            caption(ui, "현재 세션에 적용된 렌더 설정");
            ui.add_space(10.0);
            for (label, value) in [
                ("품질", if self.settings.render.draft { "Draft".to_string() } else { "Best".to_string() }),
                ("최적화", if self.settings.render.automatic { "자동".to_string() } else { "수동".to_string() }),
                ("미리보기", if self.settings.render.previews { format!("{}초 간격", self.settings.render.preview_seconds) } else { "끄기".to_string() }),
                ("작업 구간", format!("{}–{} 프레임", self.settings.range_start, self.settings.range_end)),
                ("재시도", format!("최대 {}회", self.settings.render.retries)),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(value);
                    });
                });
                ui.separator();
            }
        });
    }

    fn range_controls(&mut self, ui: &mut egui::Ui) {
        let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp).cloned() else { return };
        let total = comp.frames.max(1);
        if self.settings.range_end == 0 {
            self.settings.range_end = total;
        }
        self.settings.range_end = self.settings.range_end.clamp(1, total);
        self.settings.range_start = self.settings.range_start.clamp(0, self.settings.range_end - 1);
        ui.horizontal_wrapped(|ui| {
            ui.label("렌더 범위");
            ui.add(egui::DragValue::new(&mut self.settings.range_start).range(0..=self.settings.range_end - 1));
            ui.label("→");
            ui.add(egui::DragValue::new(&mut self.settings.range_end).range(self.settings.range_start + 1..=total));
        });
        range_bar(ui, &mut self.settings.range_start, &mut self.settings.range_end, total, (comp.work_start, comp.work_end), &mut self.range_handle);
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("작업 영역").clicked() {
                self.settings.range_start = comp.work_start;
                self.settings.range_end = comp.work_end;
            }
            if ui.small_button("전체").clicked() {
                self.settings.range_start = 0;
                self.settings.range_end = total;
            }
            caption(ui, format!("{}프레임 · 끝 프레임 미포함", self.settings.range_end - self.settings.range_start));
        });
    }

    fn monitor_panel(&mut self, ui: &mut egui::Ui) {
        let (done, total, connected, failed) = farm::progress_snapshot(&self.state);
        let (workers, status) = farm::worker_snapshot(&self.state);
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "렌더 진행 상황");
                if failed > 0 {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("실패 {failed}구간 · 작업 기록 확인"));
                }
            });
            let (elapsed, local_progress) = {
                let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                (state.started.map(|at| state.finished_elapsed.unwrap_or_else(|| at.elapsed()).as_secs_f64()), state.local_progress)
            };
            let speed: f64 = workers
                .iter()
                .filter(|w| w.connected && w.assigned.is_some())
                .map(|w| w.started.map_or(w.fps, |at| w.current_done as f64 / at.elapsed().as_secs_f64().max(0.001)))
                .sum();
            let (progress, progress_text) = if self.settings.host && total > 0 {
                (done as f32 / total as f32, format!("{done} / {total} 프레임"))
            } else if let Some((start, end, done)) = local_progress {
                (done as f32 / (end - start).max(1) as f32, format!("현재 {start}–{end} · {done}/{} 프레임", end - start))
            } else {
                (0.0, "작업 대기".into())
            };
            let animated = motion(ui.ctx(), ui.id().with("total-progress"), progress, self.settings.reduced_motion);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{:.0}%", progress * 100.0)).size(40.0).strong());
                ui.vertical(|ui| {
                    ui.label(&progress_text);
                    caption(ui, if status.is_empty() { "프레임 준비 대기" } else { &status });
                });
            });
            ui.add(egui::ProgressBar::new(animated.clamp(0.0, 1.0)).fill(BLUE).desired_height(6.0));
            ui.add_space(12.0);
            let eta = if total > 0 && done >= total {
                "프레임 완료".into()
            } else if speed > 0.0 {
                duration((total - done).max(0) as f64 / speed)
            } else {
                "계산 중".into()
            };
            let columns = if ui.available_width() >= 540.0 { 4 } else { 2 };
            let values = if self.settings.host {
                vec![
                    ("남은 시간 · 프레임 기준", eta),
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
            for row in values.chunks(columns) {
                ui.columns(columns, |cols| {
                    for (column, (label, value)) in cols.iter_mut().zip(row) {
                        metric(column, label, value);
                    }
                });
            }
        });
        ui.add_space(14.0);
        if self.settings.host && ui.available_width() >= 900.0 {
            ui.columns(2, |columns| {
                self.preview_panel(&mut columns[0], &workers);
                columns[0].add_space(14.0);
                self.activity_panel(&mut columns[0]);
                self.workers_panel(&mut columns[1], &workers);
            });
            ui.add_space(14.0);
            self.timeline_panel(ui, &workers);
        } else {
            self.preview_panel(ui, &workers);
            ui.add_space(14.0);
            if self.settings.host {
                self.workers_panel(ui, &workers);
            } else {
                let profile = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_profile.clone();
                if !profile.is_empty() {
                    card().show(ui, |ui| {
                        section_title(ui, "이 PC의 렌더 환경");
                        caption(ui, profile);
                    });
                }
            }
        }
    }

    fn activity_panel(&mut self, ui: &mut egui::Ui) {
        let messages = farm::snapshot(&self.state).4;
        card().fill(SURFACE_SECONDARY).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "최근 활동");
                if ui.small_button("전체 기록").clicked() {
                    self.logs_open = true;
                }
            });
            let recent: Vec<_> = messages.iter().rev().filter(|message| !message.contains("[DEBUG]")).take(4).collect();
            if recent.is_empty() {
                caption(ui, "작업이 시작되면 활동이 여기에 표시됩니다.");
            } else {
                for message in recent {
                    ui.add(egui::Label::new(egui::RichText::new(message).size(12.0).color(MUTED)).wrap());
                }
            }
        });
    }

    fn timeline_panel(&self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        let mut segments = farm::timeline(&self.state);
        if segments.is_empty() {
            segments = workers
                .iter()
                .filter_map(|worker| worker.assigned.map(|(start, end)| (start, end, if worker.current_done >= end - start { 2 } else { 1 })))
                .collect();
        }
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "프레임 작업 구간");
            caption(ui, "작업 PC에 배정된 프레임과 완료 상태");
            ui.add_space(10.0);
            if let (Some(first), Some(last)) = (segments.iter().map(|item| item.0).min(), segments.iter().map(|item| item.1).max()) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 12.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 6.0, SURFACE_SECONDARY);
                for (start, end, status) in &segments {
                    let x = |frame: i64| rect.left() + (frame - first) as f32 / (last - first).max(1) as f32 * rect.width();
                    let color = match status {
                        1 => BLUE,
                        2 => GREEN,
                        3 => egui::Color32::LIGHT_RED,
                        _ => BORDER,
                    };
                    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x(*start), rect.top()), egui::pos2(x(*end), rect.bottom())), 3.0, color);
                }
                caption(ui, format!("{first} → {last} · 파랑 진행 · 초록 완료 · 빨강 실패"));
            } else {
                caption(ui, "배정된 프레임 구간이 없습니다.");
            }
            ui.add_space(8.0);
            for row in workers.chunks(3) {
                ui.columns(3, |columns| {
                    for (column, worker) in columns.iter_mut().zip(row) {
                        let name = worker.label.split(" @ ").next().unwrap_or(&worker.label);
                        column.label(egui::RichText::new(name).strong());
                        caption(
                            column,
                            match worker.assigned {
                                Some((start, end)) => format!("{start}–{end} · {}프레임 완료", worker.current_done),
                                None => "배정 대기".to_string(),
                            },
                        );
                    }
                });
            }
        });
    }

    fn workers_panel(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        ui.horizontal_wrapped(|ui| {
            section_title(ui, "연결된 컴퓨터");
        });
        ui.add_space(8.0);
        if workers.is_empty() {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label("연결된 PC 없음");
            });
        }
        let columns = ((ui.available_width() + 14.0) / 270.0).floor().clamp(1.0, 4.0) as usize;
        for row in workers.chunks(columns) {
            ui.columns(columns, |cols| {
                for (column, worker) in cols.iter_mut().zip(row) {
                    self.worker_card(column, worker);
                }
            });
            ui.add_space(10.0);
        }
    }

    fn preview_panel(&mut self, ui: &mut egui::Ui, workers: &[farm::WorkerInfo]) {
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "프레임 미리보기");
                if ui.small_button("프레임 탐색 / 시험 렌더").clicked() {
                    self.preview_open = true;
                }
            });
            let preview = self
                .inspected
                .clone()
                .or_else(|| workers.iter().filter_map(|w| w.preview.clone()).max_by_key(|p| p.frame))
                .or_else(|| self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_preview.clone());
            if let Some(preview) = preview {
                self.show_preview_sized(ui, u64::MAX - 1, &preview, 540.0);
            } else {
                let height = (ui.available_width() * 0.46).clamp(160.0, 330.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
                ui.painter().rect_filled(rect, 10.0, BACKGROUND);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "미리보기 없음", egui::FontId::proportional(14.0), MUTED);
            }
            let segments = farm::timeline(&self.state);
            if let (Some(first), Some(last)) = (segments.iter().map(|s| s.0).min(), segments.iter().map(|s| s.1).max()) {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
                let x = |frame: i64| rect.left() + (frame - first) as f32 / (last - first).max(1) as f32 * rect.width();
                for (start, end, status) in segments {
                    let color = match status {
                        1 => BLUE,
                        2 => GREEN,
                        3 => egui::Color32::LIGHT_RED,
                        _ => BORDER,
                    };
                    ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x(start), rect.top()), egui::pos2(x(end), rect.bottom())), 0.0, color);
                }
                caption(ui, format!("{first} → {last} · 대기 / 진행 / 완료 / 실패"));
            }
        });
    }

    fn worker_card(&mut self, ui: &mut egui::Ui, worker: &farm::WorkerInfo) {
        ui.push_id(worker.id, |ui| {
            card().fill(SURFACE_SECONDARY).inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let name = worker.label.split(" @ ").next().unwrap_or(&worker.label);
                ui.label(egui::RichText::new(name).size(16.0).strong()).on_hover_text(&worker.label);
                let label = if !worker.connected {
                    "연결 끊김"
                } else if !worker.enabled {
                    "참여 안 함"
                } else if worker.assigned.is_some() {
                    "렌더링 중"
                } else {
                    "작업 대기"
                };
                ui.colored_label(if worker.assigned.is_some() { BLUE } else { MUTED }, label);
                if let Some(preview) = &worker.preview {
                    let height = if ui.available_width() >= 340.0 { 100.0 } else { 170.0 };
                    self.show_preview_sized(ui, worker.id, preview, height);
                } else {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 8.0, BACKGROUND);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "프레임 대기", egui::FontId::proportional(12.0), MUTED);
                }
                if let Some((start, end)) = worker.assigned {
                    let progress = worker.current_done as f32 / (end - start).max(1) as f32;
                    let value = motion(ui.ctx(), ui.id().with(("progress", start, end)), progress, self.settings.reduced_motion);
                    ui.add(egui::ProgressBar::new(value.clamp(0.0, 1.0)).fill(BLUE).desired_height(5.0));
                    caption(ui, format!("{} → {} · {}/{}", start, end, worker.current_done, end - start));
                } else {
                    caption(ui, format!("누적 {}프레임 완료", worker.completed_frames));
                }
                if ui.button("PC 상세 정보").clicked() {
                    self.worker_details = Some(worker.id);
                }
            });
        });
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        let Some((modal, reveal)) = animated_modal(ctx, "settings-dialog", self.settings_open, self.settings.reduced_motion) else {
            return;
        };
        let width = (ctx.content_rect().width() - 64.0).clamp(280.0, 640.0);
        let response = modal.show(ctx, |ui| {
            ui.set_width(width);
            ui.set_opacity(reveal);
            ui.add_enabled_ui(self.settings_open, |ui| {
                ui.horizontal(|ui| {
                    section_title(ui, "설정");
                    if icon_button(ui, UiIcon::Close, "설정 닫기").clicked() {
                        self.settings_open = false;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [(0, "일반"), (1, "렌더링"), (2, "연결"), (3, "앱 정보")] {
                        ui.selectable_value(&mut self.settings_tab, tab, label);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("settings-body")
                    .max_height((ctx.content_rect().height() - 230.0).clamp(120.0, 620.0))
                    .min_scrolled_height((ctx.content_rect().height() - 230.0).clamp(120.0, 620.0))
                    .show(ui, |ui| {
                        if self.settings_tab == 0 {
                            ui.checkbox(&mut self.settings.reduced_motion, "동작 줄이기 · 전환과 진행률 애니메이션 끄기");
                            ui.add_space(12.0);
                        }
                        if self.settings_tab == 3 {
                            self.about_settings(ui);
                        } else {
                            if self.running || self.pending_start.is_some() {
                                caption(ui, "작업을 종료하면 설정을 변경할 수 있습니다.");
                            }
                            ui.add_enabled_ui(!self.running && self.pending_start.is_none(), |ui| match self.settings_tab {
                                0 => self.general_settings(ui),
                                1 => self.render_settings(ui),
                                _ => self.connection_settings(ui),
                            });
                        }
                    });
            });
        });
        if response.should_close() {
            self.settings_open = false;
        }
    }

    fn general_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "렌더링");
        ui.checkbox(&mut self.settings.render.automatic, "자동 최적화").on_hover_text("CPU/GPU, 인코더, 스레드 수와 작업량을 PC 성능에 맞춰 조절합니다.");
        ui.checkbox(&mut self.settings.local_worker, "호스트 PC도 렌더링에 참여");
        ui.add_enabled(!self.settings.render.automatic, egui::Checkbox::new(&mut self.settings.gpu, "수동: 이 PC에서 GPU 가속 사용"));
        ui.add_space(14.0);
        section_title(ui, "화면과 동작");
        ui.checkbox(&mut self.settings.render.previews, "PC별 프레임 미리보기");
        ui.horizontal_wrapped(|ui| {
            ui.label("미리보기 주기");
            ui.add(egui::DragValue::new(&mut self.settings.render.preview_seconds).range(1..=30).suffix("초"));
        });
        ui.add_space(14.0);
        caption(ui, "EffectCraft CLI");
        ui.label(egui::RichText::new(&self.settings.cli).small());
        if ui.button("컴포지션 목록 새로고침").clicked() {
            self.catalog_changed = Some(Instant::now());
        }
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "품질과 출력");
        let options = &mut self.settings.render;
        ui.checkbox(&mut options.draft, "Draft 품질").on_hover_text("빠른 검토용 품질입니다. 끄면 Best 품질을 사용합니다.");
        ui.checkbox(&mut options.audio, "원본 오디오 포함");
        ui.horizontal_wrapped(|ui| {
            ui.label("H.264 비트레이트");
            ui.add(egui::DragValue::new(&mut options.video_bitrate).range(100..=200_000).speed(100).suffix("kbps"));
        });
        ui.add_enabled(!options.automatic, egui::Checkbox::new(&mut options.nvenc, "FFmpeg 직접 인코딩"));
        if path_row(ui, "FFmpeg 경로 · 비워 두면 자동 감지", &mut options.ffmpeg)
            && let Some(path) = rfd::FileDialog::new().pick_file()
        {
            options.ffmpeg = path.display().to_string();
        }
        ui.add_space(10.0);
        if path_row(ui, "프레임 저장 폴더 · 고급", &mut self.settings.output)
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            self.settings.output = path.display().to_string();
        }
        ui.add_space(14.0);
        section_title(ui, "작업 분배와 복구");
        ui.horizontal_wrapped(|ui| {
            ui.label("목표 구간 시간");
            ui.add(egui::DragValue::new(&mut options.target_seconds).range(5..=300).suffix("초"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("최대 시도 횟수");
            ui.add(egui::DragValue::new(&mut options.retries).range(1..=10).suffix("회"));
        });
        ui.add_enabled(!options.automatic, egui::Checkbox::new(&mut options.adaptive, "처리 속도에 따라 구간 나누기"));
        ui.checkbox(&mut options.durable_writes, "수신 프레임마다 디스크 동기화");
        caption(ui, "디스크 동기화는 전원 장애에 대비하지만 저장 시간이 늘어납니다.");
        ui.horizontal_wrapped(|ui| {
            ui.label("프레임 감지 주기");
            ui.add(egui::DragValue::new(&mut options.poll_ms).range(50..=2000).suffix("ms"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("구간 분배");
            ui.selectable_value(&mut self.settings.distribution, Distribution::Auto, "자동");
            ui.selectable_value(&mut self.settings.distribution, Distribution::Frames, "프레임");
            ui.selectable_value(&mut self.settings.distribution, Distribution::Percent, "비율");
        });
        match self.settings.distribution {
            Distribution::Frames => {
                ui.add(egui::DragValue::new(&mut self.settings.chunk_frames).range(1..=100_000).suffix("프레임"));
            }
            Distribution::Percent => {
                ui.add(egui::DragValue::new(&mut self.settings.chunk_percent).range(0.01..=100.0).suffix("%"));
            }
            Distribution::Auto => {}
        }
        if !self.settings.render.automatic {
            ui.horizontal_wrapped(|ui| {
                ui.label("동시 작업 PC");
                ui.add(egui::DragValue::new(&mut self.settings.max_workers).range(1..=64).suffix("대"));
            });
        }
    }

    fn connection_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "네트워크");
        text_row(ui, "호스트 수신 IP:포트", &mut self.settings.host_address, false);
        caption(ui, "0.0.0.0: 모든 네트워크 인터페이스");
        ui.add_space(12.0);
        text_row(ui, "작업 PC의 호스트 주소", &mut self.settings.client_address, false);
        caption(ui, "다른 PC에 연결할 때는 해당 PC의 LAN IP를 사용하세요.");
    }

    fn about_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "EffectCraft Render Farm");
        caption(ui, format!("버전 {}", env!("CARGO_PKG_VERSION")));
        ui.checkbox(&mut self.settings.check_updates, "앱 실행 시 새 버전 확인");
        if ui.add_enabled(!self.updater.busy(), egui::Button::new("업데이트 확인")).clicked() {
            self.updater.check();
        }
        ui.label(&self.updater.status);
        if self.updater.available.is_some() && !self.updater.busy() && ui.button("업데이트 보기").clicked() {
            self.settings_open = false;
            self.updater.prompt = true;
        }
        caption(ui, "새 버전은 사용자가 동의한 후 설치합니다.");
        if let Ok(path) = settings::log_path() {
            caption(ui, format!("로그 파일: {}", path.display()));
        }
        ui.hyperlink_to("GitHub", "https://github.com/whitedev7773/effectcraft-render-farm");
    }

    fn logs_dialog(&mut self, ctx: &egui::Context) {
        let Some((modal, reveal)) = animated_modal(ctx, "logs-dialog", self.logs_open, self.settings.reduced_motion) else {
            return;
        };
        let response = modal.show(ctx, |ui| {
            ui.set_opacity(reveal);
            if !self.logs_open {
                ui.disable();
            }
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 1000.0));
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "작업 기록");
                if icon_button(ui, UiIcon::Close, "작업 기록 닫기").clicked() {
                    self.logs_open = false;
                }
                ui.checkbox(&mut self.log_debug, "상세 로그");
            });
            ui.add(egui::TextEdit::singleline(&mut self.log_filter).hint_text("검색").desired_width(ui.available_width()));
            let messages = farm::snapshot(&self.state).4;
            if ui.add(egui::Button::image_and_text(icon_image(ui, UiIcon::Copy), "표시된 기록 복사")).clicked() {
                ctx.copy_text(
                    messages
                        .iter()
                        .filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            egui::ScrollArea::vertical().id_salt("log-body").max_height((ctx.content_rect().height() - 230.0).max(120.0)).show(ui, |ui| {
                for message in messages.iter().rev().filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter)) {
                    let color = if message.contains("[ERROR]") {
                        egui::Color32::LIGHT_RED
                    } else if message.contains("[WARN]") {
                        egui::Color32::LIGHT_YELLOW
                    } else {
                        MUTED
                    };
                    ui.add(egui::Label::new(egui::RichText::new(message).size(12.0).color(color)).wrap());
                }
            });
        });
        if response.should_close() {
            self.logs_open = false;
        }
    }

    fn worker_dialog(&mut self, ctx: &egui::Context) {
        let last_id = egui::Id::new("last-worker-details");
        if let Some(id) = self.worker_details {
            ctx.data_mut(|data| data.insert_temp(last_id, id));
        }
        if let Some((modal, reveal)) = animated_modal(ctx, "worker-dialog", self.worker_details.is_some(), self.settings.reduced_motion) {
            let id = ctx.data_mut(|data| data.get_temp::<u64>(last_id)).unwrap_or(0);
            let worker = farm::worker_snapshot(&self.state).0.into_iter().find(|worker| worker.id == id);
            let response = modal.show(ctx, |ui| {
                ui.set_opacity(reveal);
                if self.worker_details.is_none() {
                    ui.disable();
                }
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 520.0));
                ui.horizontal(|ui| {
                    section_title(ui, "컴퓨터 상세 정보");
                    if icon_button(ui, UiIcon::Close, "컴퓨터 상세 정보 닫기").clicked() {
                        self.worker_details = None;
                    }
                });
                if let Some(worker) = worker {
                    ui.label(&worker.label);
                    ui.label(&worker.profile);
                    let mut enabled = worker.enabled;
                    if ui.checkbox(&mut enabled, "이 PC를 렌더링에 사용").changed()
                        && let Some(ip) = farm::set_worker_enabled(&self.state, id, enabled)
                    {
                        self.settings.disabled_workers.retain(|saved| saved != &ip && saved != &worker.label);
                        if !enabled {
                            self.settings.disabled_workers.push(ip);
                        }
                    }
                    caption(ui, format!("완료 {}프레임 · {:.2} fps", worker.completed_frames, worker.fps));
                    caption(
                        ui,
                        format!(
                            "수신 {:.1} MiB · {:.1} MiB/s",
                            worker.transferred_bytes as f64 / 1048576.0,
                            worker.transferred_bytes as f64 / 1048576.0 / worker.transfer_seconds.max(0.001)
                        ),
                    );
                } else {
                    ui.label("이 PC의 연결 정보가 더 이상 없습니다.");
                }
            });
            if response.should_close() {
                self.worker_details = None;
            }
        }
        self.preview_dialog(ctx);
    }

    fn preview_dialog(&mut self, ctx: &egui::Context) {
        let Some((modal, reveal)) = animated_modal(ctx, "preview-dialog", self.preview_open, self.settings.reduced_motion) else {
            return;
        };
        let response = modal.show(ctx, |ui| {
            ui.set_opacity(reveal);
            if !self.preview_open {
                ui.disable();
            }
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 760.0));
            ui.horizontal_wrapped(|ui| {
                section_title(ui, "프레임 탐색");
                if icon_button(ui, UiIcon::Close, "프레임 탐색 닫기").clicked() {
                    self.preview_open = false;
                }
            });
            let range = self
                .job
                .as_ref()
                .map(|job| (job.start, job.end))
                .unwrap_or((self.settings.range_start, self.settings.range_end.max(self.settings.range_start + 1)));
            self.inspect_frame = self.inspect_frame.clamp(range.0, range.1 - 1);
            ui.add(egui::Slider::new(&mut self.inspect_frame, range.0..=range.1 - 1).text("프레임"));
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(self.pending_preview.is_none() && self.settings.host, egui::Button::new("완료 프레임 보기")).clicked() {
                    let output = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).output.clone();
                    self.request_preview(move |frame| farm::preview_frame(&output, frame));
                }
                if ui.add_enabled(self.pending_preview.is_none() && !self.running && !self.settings.comp.is_empty(), egui::Button::new("시험 렌더")).clicked()
                {
                    let config = self.config();
                    self.request_preview(move |frame| farm::render_preview(&config, frame));
                }
            });
            caption(ui, &self.preview_status);
            if self.pending_preview.is_some() {
                ui.spinner();
            }
            if let Some(preview) = self.inspected.clone() {
                self.show_preview(ui, u64::MAX, &preview);
            }
        });
        if response.should_close() {
            self.preview_open = false;
        }
    }

    fn request_preview(&mut self, render: impl FnOnce(i64) -> Result<farm::Preview, String> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let frame = self.inspect_frame;
        self.pending_preview = Some(rx);
        self.preview_status = "프레임을 준비하고 있습니다…".into();
        thread::spawn(move || {
            let _ = tx.send(render(frame));
        });
    }
}
