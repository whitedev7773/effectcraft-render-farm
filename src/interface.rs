use super::*;
mod buttons;
use buttons::{ButtonKind, ButtonSpec, button_at, dialog_header, footer_buttons, footer_status};
mod dashboard;
pub(crate) mod layout;
mod queue;
mod updates;

#[cfg(any(test, debug_assertions))]
pub(super) mod review;

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(13, 15, 20);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(22, 25, 32);
const SURFACE_SECONDARY: egui::Color32 = egui::Color32::from_rgb(29, 33, 42);
const BORDER: egui::Color32 = egui::Color32::from_rgb(47, 53, 66);
const MUTED: egui::Color32 = egui::Color32::from_rgb(169, 179, 196);
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
    ui.add_sized([layout::CONTROL, layout::CONTROL], egui::Button::image(icon_image(ui, icon)).frame(false)).on_hover_text(label)
}

fn card() -> egui::Frame {
    egui::Frame::new().fill(SURFACE).stroke(egui::Stroke::new(1.0, BORDER)).corner_radius(layout::RADIUS).inner_margin(layout::PADDING)
}

fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new().fill(color.gamma_multiply(0.12)).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).size(12.0).color(color));
    });
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
        let ctx = root.ctx().clone();
        ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = if self.settings.reduced_motion { 0.0 } else { 0.24 });
        egui::Panel::bottom("primary-actions")
            .frame(egui::Frame::new().fill(SURFACE).stroke(egui::Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(layout::PADDING, 8)))
            .show(root, |ui| {
                self.action_bar(ui);
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BACKGROUND).inner_margin(layout::PADDING)).show(root, |ui| {
            self.toolbar(ui);
            ui.add_space(layout::SPACE);
            if !self.last_error.is_empty() {
                egui::Frame::new().fill(egui::Color32::from_rgb(58, 31, 36)).corner_radius(layout::RADIUS).inner_margin(layout::PADDING).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(egui::Color32::LIGHT_RED, &self.last_error);
                        if ui.add_sized([64.0, layout::COMPACT_CONTROL], egui::Button::new("닫기").small()).clicked() {
                            self.last_error.clear();
                        }
                    });
                });
                ui.add_space(layout::SPACE);
            }
            self.workspace_tabs(ui, false);
            ui.add_space(layout::SPACE);
            let height = ui.available_height();
            egui::ScrollArea::vertical().id_salt(("workspace", self.monitor, self.queue_view)).auto_shrink([false, false]).show(ui, |ui| {
                self.workspace_grid(ui, height);
            });
        });
        self.settings_dialog(&ctx);
        self.logs_dialog(&ctx);
        self.worker_dialog(&ctx);
        self.queue_dialog(&ctx);
    }

    fn workspace_tabs(&mut self, ui: &mut egui::Ui, wide: bool) {
        ui.horizontal_wrapped(|ui| {
            if !wide && ui.selectable_label(!self.monitor, "프로젝트 · 연결").clicked() {
                self.monitor = false;
            }
            if ui.selectable_label((wide || self.monitor) && !self.queue_view, "진행 현황").clicked() {
                self.monitor = true;
                self.queue_view = false;
            }
            if self.settings.host && ui.selectable_label((wide || self.monitor) && self.queue_view, format!("렌더 큐  {}", self.settings.queue.len())).clicked()
            {
                self.monitor = true;
                self.queue_view = true;
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Render Farm").size(26.0).strong());
            });
            ui.add_space(layout::SPACE);
            let (label, color) = if self.stopping {
                ("중지 중", MUTED)
            } else if self.pending_start.is_some() {
                ("준비 중", BLUE)
            } else if self.running {
                let rendering = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
                (
                    if self.settings.host && rendering {
                        "렌더링 중"
                    } else if self.settings.host {
                        "PC 연결 대기"
                    } else {
                        "호스트 연결됨"
                    },
                    GREEN,
                )
            } else {
                ("연결 대기", MUTED)
            };
            badge(ui, label, color);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, UiIcon::Settings, "설정").clicked() {
                    self.settings_open = true;
                }
                if icon_button(ui, UiIcon::History, "작업 기록").clicked() {
                    self.logs_open = true;
                }
                if (self.updater.available.is_some() || self.engine_updater.required.is_some())
                    && ui
                        .add(egui::Button::image_and_text(icon_image(ui, UiIcon::Update), "업데이트"))
                        .on_hover_text("EffectCraft 및 Render Farm 업데이트")
                        .clicked()
                {
                    if self.engine_updater.required.is_some() {
                        self.engine_updater.prompt = true;
                    } else {
                        self.updater.prompt = true;
                    }
                }
            });
        });
        if self.updater.installing() {
            caption(ui, format!("업데이트 준비 중 · {:.1} MiB", self.updater.progress.load(Ordering::Relaxed) as f64 / 1048576.0));
        }
        if self.engine_updater.installing() {
            caption(ui, format!("EffectCraft 다운로드·설치 중 · {:.1} MiB", self.engine_updater.progress.load(Ordering::Relaxed) as f64 / 1048576.0));
        } else if self.engine_install_requested {
            caption(ui, "현재 작업을 마친 뒤 EffectCraft를 업데이트합니다.");
        }
    }

    fn start_blocker(&self) -> Option<String> {
        if self.engine_install_requested || self.updater.installing() || self.engine_updater.installing() {
            return Some("업데이트가 끝나면 연결할 수 있습니다.".into());
        }
        if self.engine_updater.checking() {
            return Some("EffectCraft 버전을 확인하고 있습니다…".into());
        }
        if self.engine_updater.local.is_none() {
            return Some("설정 → 앱 정보에서 EffectCraft 설치 상태를 확인하세요.".into());
        }
        if self.pending_preview.is_some() || self.worker_active.load(Ordering::Relaxed) {
            return Some("진행 중인 작업을 마치고 있습니다…".into());
        }
        if self.settings.host && !self.settings.queue.is_empty() {
            if !self.settings.queue.iter().any(|task| task.status == crate::queue::Status::Pending) {
                return Some("렌더 큐에 작업을 추가하거나 기존 작업을 다시 대기 상태로 바꾸세요.".into());
            }
        } else if self.settings.host {
            if let Some(missing) = self.missing_host_settings().first() {
                return Some(format!("{missing} 항목을 준비하세요."));
            }
            if !self.catalog.iter().any(|comp| comp.name == self.settings.comp) {
                return Some("렌더할 컴포지션을 선택하세요.".into());
            }
        } else if self.settings.client_address.trim().is_empty() {
            return Some("주변 호스트를 선택하거나 호스트 주소를 입력하세요.".into());
        }
        if self.settings.token.trim().len() < 8 {
            return Some("호스트와 동일한 접속 암호를 8자 이상 입력하세요.".into());
        }
        None
    }

    fn action_bar(&mut self, ui: &mut egui::Ui) {
        let mut primary = None;
        let mut dismiss = None;
        let busy = self.pending_start.is_some() || self.stopping;
        let hint = if self.pending_start.is_some() {
            "프로젝트와 소스를 준비하고 있습니다…".into()
        } else if !self.running {
            let blocker = self.start_blocker();
            let mut action = ButtonSpec::new(if self.settings.host { "호스트 열기" } else { "호스트에 연결" }, ButtonKind::Primary, blocker.is_none());
            action.min_width = 120.0;
            primary = Some(action);
            blocker.unwrap_or_else(|| {
                if self.settings.host {
                    "호스트를 연 뒤 작업 PC를 연결하고 렌더링을 시작하세요.".into()
                } else {
                    "준비 완료 · 호스트의 렌더 작업에 참여합니다.".into()
                }
            })
        } else {
            let started = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
            let selected = self.connected_render_workers();
            if self.settings.host && !started {
                let mut action = ButtonSpec::new(
                    if self.queue_automatic { "연결된 PC로 계속" } else { "렌더 시작" },
                    ButtonKind::Primary,
                    !self.stopping && !selected.is_empty() && self.engine_updater.required.is_none(),
                );
                action.min_width = 120.0;
                primary = Some(action);
            }
            dismiss = Some(ButtonSpec::new(if self.stopping { "종료 중…" } else { "연결 종료" }, ButtonKind::Normal, !self.stopping));
            if self.stopping {
                "진행 중인 구간을 마치고 연결을 종료합니다.".into()
            } else if self.settings.host && !started {
                format!("사용할 연결 PC {}대 · PC 카드에서 렌더 참여를 선택하세요.", selected.len())
            } else if started {
                "렌더링 중 · 연결을 종료하면 현재 구간을 마친 뒤 중지합니다.".into()
            } else {
                "호스트와 연결되어 있습니다. 작업 지시를 기다립니다.".into()
            }
        };
        let clicks = footer_buttons(ui, None, primary, dismiss, layout::PRIMARY_CONTROL);
        if let Some(rect) = clicks.free_left {
            footer_status(ui, rect, &hint, busy);
        }
        if clicks.primary {
            if self.running {
                self.begin_current_render();
            } else if self.settings.host && !self.settings.queue.is_empty() {
                self.queue_automatic = false;
                self.queue_workers.clear();
                self.start_next_queued();
            } else {
                self.start();
            }
        }
        if clicks.dismiss {
            self.queue_automatic = false;
            self.stop.store(true, Ordering::Relaxed);
            self.stopping = true;
        }
    }

    fn setup_content(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 8.0;
        section_title(
            ui,
            if self.running {
                "현재 작업"
            } else if self.settings.host {
                "프로젝트 준비"
            } else {
                "호스트 연결"
            },
        );
        if self.running || self.pending_start.is_some() {
            ui.add_space(8.0);
            if self.settings.host {
                ui.label(egui::RichText::new(&self.settings.comp).size(16.0).strong());
                let name = std::path::Path::new(&self.settings.project).file_name().unwrap_or_default().to_string_lossy();
                ui.label(name).on_hover_text(&self.settings.project);
                ui.add_space(layout::SPACE);
                caption(ui, "출력 파일");
                let output = std::path::Path::new(&self.settings.final_video).file_name().unwrap_or_default().to_string_lossy();
                ui.add(egui::Label::new(output).truncate()).on_hover_text(&self.settings.final_video);
            } else {
                caption(ui, "연결된 호스트");
                ui.label(&self.settings.client_address);
                caption(ui, "렌더 설정은 호스트에서 자동으로 적용됩니다.");
            }
            ui.add_space(8.0);
            caption(ui, "연결을 종료하면 프로젝트와 연결 설정을 바꿀 수 있습니다.");
            return;
        }
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
                ui.add_space(layout::SPACE);
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
                    caption(ui, if self.catalog_status.is_empty() { "프로젝트를 선택하면 컴포지션을 불러옵니다." } else { &self.catalog_status });
                }
                if self.catalog.iter().any(|comp| comp.name == self.settings.comp) {
                    ui.add_space(layout::SPACE);
                    let frames = self.settings.range_end.saturating_sub(self.settings.range_start);
                    egui::CollapsingHeader::new(format!("렌더 범위 · {frames} 프레임")).id_salt("render-range").show(ui, |ui| self.range_controls(ui));
                }
                ui.add_space(layout::GAP);
                if path_row(ui, "최종 MP4", &mut self.settings.final_video)
                    && let Some(path) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).save_file()
                {
                    self.settings.final_video = path.display().to_string();
                }
                ui.add_space(8.0);
                if ui
                    .add_enabled(!self.settings.comp.is_empty() && !self.settings.final_video.trim().is_empty(), egui::Button::new("+ 렌더 큐에 추가"))
                    .on_hover_text("여러 작업을 모아서 순서대로 렌더링합니다.")
                    .clicked()
                {
                    self.enqueue_current();
                }
            } else {
                text_row(ui, "호스트 주소", &mut self.settings.client_address, false);
                caption(ui, "예: 192.168.0.10:9778");
                if ui
                    .add_enabled(
                        self.pending_discovery.is_none(),
                        egui::Button::new(if self.pending_discovery.is_some() { "호스트 찾는 중…" } else { "주변 호스트 찾기" }),
                    )
                    .clicked()
                {
                    self.start_discovery();
                }
                for host in &self.discovered_hosts {
                    let address = host.address.to_string();
                    if ui.selectable_label(self.settings.client_address == address, format!("{} · {address}", host.name)).clicked() {
                        self.settings.client_address = address;
                    }
                }
                if !self.discovery_status.is_empty() {
                    caption(ui, &self.discovery_status);
                }
                ui.add_space(layout::SPACE);
            }
            ui.add_space(8.0);
            text_row(ui, "접속 암호 · 8자 이상", &mut self.settings.token, true);
        });
        ui.add_space(layout::GAP);
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
            if ui.add_sized([80.0, layout::COMPACT_CONTROL], egui::Button::new("작업 영역").small()).clicked() {
                self.settings.range_start = comp.work_start;
                self.settings.range_end = comp.work_end;
            }
            if ui.add_sized([80.0, layout::COMPACT_CONTROL], egui::Button::new("전체").small()).clicked() {
                self.settings.range_start = 0;
                self.settings.range_end = total;
            }
            caption(ui, format!("{}프레임 · 끝 프레임 미포함", self.settings.range_end - self.settings.range_start));
        });
    }

    fn set_worker_participation(&mut self, worker: &farm::WorkerInfo, enabled: bool) {
        if let Some(ip) = farm::set_worker_enabled(&self.state, worker.id, enabled) {
            self.settings.disabled_workers.retain(|saved| saved != &ip && saved != &worker.label);
            if !enabled {
                self.settings.disabled_workers.push(ip);
            }
        }
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
                if dialog_header(ui, "설정", "설정 닫기") {
                    self.settings_open = false;
                }
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
                            ui.add_space(layout::GAP);
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
        ui.add_space(layout::GAP);
        section_title(ui, "화면과 동작");
        ui.checkbox(&mut self.settings.render.previews, "PC별 프레임 미리보기");
        ui.horizontal_wrapped(|ui| {
            ui.label("미리보기 주기");
            ui.add(egui::DragValue::new(&mut self.settings.render.preview_seconds).range(1..=30).suffix("초"));
        });
        ui.add_space(layout::GAP);
        caption(ui, "EffectCraft CLI");
        caption(ui, format!("EffectCraft {}", self.engine_updater.local.as_deref().unwrap_or("버전 확인 필요")));
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
        ui.add_space(layout::SPACE);
        caption(ui, format!("프레임 저장 폴더 · 자동: {}", settings::frame_output(&self.settings.project).display()));
        ui.add_space(layout::GAP);
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
        ui.add_space(layout::GAP);
        text_row(ui, "작업 PC의 호스트 주소", &mut self.settings.client_address, false);
        caption(ui, "다른 PC에 연결할 때는 해당 PC의 LAN IP를 사용하세요.");
    }

    fn about_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "EffectCraft");
        caption(ui, format!("설치 버전 {}", self.engine_updater.local.as_deref().unwrap_or("확인 필요")));
        ui.label(&self.engine_updater.status);
        if ui
            .add_enabled(
                !self.running
                    && self.pending_start.is_none()
                    && !self.worker_active.load(Ordering::Relaxed)
                    && !self.engine_updater.checking()
                    && !self.engine_updater.installing()
                    && !self.updater.installing(),
                egui::Button::new("EffectCraft 버전 다시 확인"),
            )
            .clicked()
        {
            self.engine_updater.check(PathBuf::from(&self.settings.cli));
        }
        if self.engine_updater.required.is_some() && ui.button("EffectCraft 업데이트 보기").clicked() {
            self.settings_open = false;
            self.engine_updater.prompt = true;
        }
        ui.add_space(layout::GAP);
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
            if dialog_header(ui, "작업 기록", "작업 기록 닫기") {
                self.logs_open = false;
            }
            let wide = ui.available_width() >= 640.0;
            if !wide {
                ui.add_sized([ui.available_width(), layout::CONTROL], egui::TextEdit::singleline(&mut self.log_filter).hint_text("기록 검색"));
            }
            let mut copy = false;
            ui.horizontal(|ui| {
                if wide {
                    let width = (ui.available_width() - 240.0).max(120.0);
                    ui.add_sized([width, layout::CONTROL], egui::TextEdit::singleline(&mut self.log_filter).hint_text("기록 검색"));
                }
                ui.checkbox(&mut self.log_debug, "상세 로그");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    copy = ui
                        .add_sized([128.0, layout::CONTROL], egui::Button::image_and_text(icon_image(ui, UiIcon::Copy), "기록 복사").frame(false))
                        .on_hover_text("현재 검색 결과에 표시된 기록을 복사합니다.")
                        .clicked();
                });
            });
            let messages = farm::snapshot(&self.state).4;
            if copy {
                ctx.copy_text(
                    messages
                        .iter()
                        .filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            egui::ScrollArea::vertical().id_salt("log-body").max_height((ctx.content_rect().height() - 252.0).max(120.0)).show(ui, |ui| {
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
                if dialog_header(ui, "컴퓨터 상세 정보", "컴퓨터 상세 정보 닫기") {
                    self.worker_details = None;
                }
                egui::ScrollArea::vertical().id_salt("worker-body").max_height((ctx.content_rect().height() - 144.0).max(120.0)).show(ui, |ui| {
                    if let Some(worker) = worker {
                        ui.label(&worker.label);
                        ui.label(&worker.profile);
                        let mut enabled = worker.enabled;
                        if ui.checkbox(&mut enabled, "이 PC를 렌더링에 사용").changed() {
                            self.set_worker_participation(&worker, enabled);
                        }
                        if let Some(preview) = &worker.preview {
                            self.show_preview_sized(ui, worker.id, preview, 200.0);
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
        let mut close = false;
        let response = modal.show(ctx, |ui| {
            ui.set_opacity(reveal);
            if !self.preview_open {
                ui.disable();
            }
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 760.0));
            close = dialog_header(ui, "프레임 탐색", "프레임 탐색 닫기");
            let range =
                self.job.as_ref().map(|j| (j.start, j.end)).unwrap_or((self.settings.range_start, self.settings.range_end.max(self.settings.range_start + 1)));
            self.inspect_frame = self.inspect_frame.clamp(range.0, range.1 - 1);
            egui::ScrollArea::vertical().id_salt("preview-body").max_height((ctx.content_rect().height() - 224.0).max(120.0)).show(ui, |ui| {
                caption(ui, format!("{}–{} 프레임 · 끝 프레임 미포함", range.0, range.1));
                ui.add(egui::Slider::new(&mut self.inspect_frame, range.0..=range.1 - 1).text("프레임"));
                if !self.preview_status.is_empty() {
                    caption(ui, &self.preview_status);
                }
                if self.pending_preview.is_some() {
                    ui.spinner();
                }
                if let Some(preview) = self.inspected.clone() {
                    self.show_preview(ui, u64::MAX, &preview);
                }
            });
            ui.separator();
            let render = self.pending_preview.is_none()
                && !self.running
                && !self.engine_updater.installing()
                && !self.engine_install_requested
                && !self.settings.comp.is_empty();
            let load = self.pending_preview.is_none() && self.settings.host;
            let clicks = footer_buttons(
                ui,
                None,
                Some(ButtonSpec::new("시험 렌더", ButtonKind::Primary, render)),
                Some(ButtonSpec::new("완료 프레임 보기", ButtonKind::Normal, load)),
                layout::CONTROL,
            );
            if clicks.dismiss {
                let output = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).output.clone();
                self.request_preview(move |frame| farm::preview_frame(&output, frame));
            }
            if clicks.primary {
                let config = self.config();
                self.request_preview(move |frame| farm::render_preview(&config, frame));
            }
        });
        if response.should_close() || close {
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
