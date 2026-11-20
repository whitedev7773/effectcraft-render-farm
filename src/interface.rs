use super::*;
mod buttons;
mod components;
pub(super) mod import;
use buttons::{ButtonKind, ButtonSpec, button_at, dialog_header, footer_buttons, footer_status};
mod dashboard;
pub(crate) mod layout;
mod queue;
mod shell;
mod updates;

#[cfg(any(test, debug_assertions))]
pub(super) mod review;

fn background(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().panel_fill
}
fn surface(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().window_fill
}
fn secondary(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().faint_bg_color
}
fn border(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().widgets.noninteractive.bg_stroke.color
}
fn muted(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(151, 158, 168) } else { egui::Color32::from_rgb(105, 111, 119) }
}
fn accent(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(106, 170, 255) } else { egui::Color32::from_rgb(34, 102, 215) }
}
fn success(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(116, 200, 160) } else { egui::Color32::from_rgb(34, 126, 85) }
}
fn danger(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(255, 150, 150) } else { egui::Color32::from_rgb(168, 36, 36) }
}
fn warning(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode { egui::Color32::from_rgb(245, 205, 120) } else { egui::Color32::from_rgb(135, 90, 0) }
}

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
    egui::Image::new((texture.id(), texture.size_vec2())).fit_to_exact_size(egui::vec2(18.0, 18.0)).tint(ui.visuals().text_color())
}

fn icon_button(ui: &mut egui::Ui, icon: UiIcon, label: &str) -> egui::Response {
    ui.add_sized([layout::CONTROL, layout::CONTROL], egui::Button::image(icon_image(ui, icon)).frame(false)).on_hover_text(label)
}

fn card(ctx: &egui::Context) -> egui::Frame {
    let style = ctx.style_of(ctx.theme());
    let visuals = &style.visuals;
    egui::Frame::new().fill(visuals.window_fill).stroke(visuals.widgets.noninteractive.bg_stroke).corner_radius(layout::RADIUS).inner_margin(layout::PADDING)
}

fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    components::status(ui, text, color);
}

fn animated_modal(ctx: &egui::Context, id: &str, open: bool, reduced: bool) -> Option<(egui::Modal, f32)> {
    if !open {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new((id, "reveal")), Spring::default()));
        return None;
    }
    let reveal = motion(ctx, egui::Id::new((id, "reveal")), f32::from(open), reduced);
    if reveal < 0.001 {
        return None;
    }
    let area = egui::Modal::default_area(egui::Id::new(id)).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, (1.0 - reveal) * 14.0));
    let frame = card(ctx);
    Some((egui::Modal::new(egui::Id::new(id)).area(area).frame(frame).backdrop_color(egui::Color32::from_black_alpha((160.0 * reveal) as u8)), reveal))
}

fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).size(12.0).color(muted(ui)));
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
        ctx.set_theme(self.settings.theme.egui());
        ctx.style_mut_of(self.settings.theme.egui(), |style| style.animation_time = if self.settings.reduced_motion { 0.0 } else { 0.24 });
        let compact = ctx.content_rect().width() < 1000.0;
        if !compact {
            egui::Panel::left("workspace-sidebar")
                .resizable(false)
                .default_size(200.0)
                .min_size(200.0)
                .max_size(200.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(surface(root)).inner_margin(egui::Margin::symmetric(16, 20)))
                .show(root, |ui| self.sidebar(ui));
        }
        egui::Panel::bottom("primary-actions")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(surface(root)).inner_margin(egui::Margin::symmetric(24, 14)))
            .show(root, |ui| {
                ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().top() - 14.0, egui::Stroke::new(1.0, border(ui)));
                self.action_bar(ui);
            });
        let padding = if compact { 16 } else { 24 };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(background(root)).inner_margin(padding)).show(root, |ui| {
            self.page_heading(ui, compact);
            if compact {
                self.workspace_tabs(ui);
                ui.add_space(12.0);
            }
            if !self.last_error.is_empty() {
                let error_fill = if ui.visuals().dark_mode { egui::Color32::from_rgb(58, 31, 36) } else { egui::Color32::from_rgb(255, 239, 239) };
                egui::Frame::new().fill(error_fill).corner_radius(layout::RADIUS).inner_margin(12).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(danger(ui), &self.last_error);
                        if ui.add_sized([64.0, layout::COMPACT_CONTROL], egui::Button::new("닫기").small()).clicked() {
                            self.last_error.clear();
                        }
                    });
                });
                ui.add_space(layout::SPACE);
            }
            let height = ui.available_height();
            egui::ScrollArea::vertical().id_salt(("workspace", self.monitor, self.queue_view)).auto_shrink([false, false]).show(ui, |ui| {
                self.workspace_grid(ui, height);
            });
        });
        self.settings_dialog(&ctx);
        self.logs_dialog(&ctx);
        self.worker_dialog(&ctx);
        self.queue_dialog(&ctx);
        self.project_import_ui(&ctx);
    }

    fn workspace_tabs(&mut self, ui: &mut egui::Ui) {
        let selected = if !self.monitor {
            0
        } else if self.queue_view {
            2
        } else {
            1
        };
        let queue = format!("렌더 큐  {}", self.settings.queue.len());
        let labels =
            if self.settings.host { vec!["프로젝트 준비", "렌더 모니터", queue.as_str()] } else { vec!["프로젝트 준비", "렌더 모니터"] };
        if let Some(view) = components::segmented(ui, "workspace-tabs", selected, &labels) {
            self.monitor = view != 0;
            self.queue_view = view == 2;
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
            return Some("설정 → 앱과 업데이트에서 EffectCraft 설치 상태를 확인하세요.".into());
        }
        if self.pending_preview.is_some() || self.worker_active.load(Ordering::Relaxed) {
            return Some("진행 중인 작업을 마치고 있습니다…".into());
        }
        if self.settings.host {
            if !self.settings.queue.iter().any(|task| task.status == crate::queue::Status::Pending) {
                return Some("프로젝트를 불러와 컴포지션을 큐에 추가하세요.".into());
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
                format!("사용할 연결 PC {}대 · 작업 PC 표에서 렌더 참여를 선택하세요.", selected.len())
            } else if started {
                "렌더링 중 · 연결을 종료하면 현재 구간을 마친 뒤 중지합니다.".into()
            } else {
                "호스트와 연결되어 있습니다. 작업 지시를 기다립니다.".into()
            }
        };
        let show_hint_above = !self.running && !busy && self.start_blocker().is_some() && ui.available_width() < 780.0;
        if show_hint_above {
            ui.add(egui::Label::new(egui::RichText::new(&hint).size(12.0).color(muted(ui))).wrap());
            ui.add_space(layout::SPACE);
        }
        let clicks = footer_buttons(ui, None, primary, dismiss, layout::PRIMARY_CONTROL);
        if let Some(rect) = clicks.free_left.filter(|_| !show_hint_above) {
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
        section_title(ui, "연결 설정");
        ui.add_enabled_ui(!self.running && self.pending_start.is_none(), |ui| {
            if let Some(role) = components::segmented(ui, "connection-role", usize::from(!self.settings.host), &["호스트", "작업 PC"]) {
                self.settings.host = role == 0;
                self.queue_view = false;
            }
            caption(
                ui,
                if self.settings.host {
                    "이 PC에서 큐와 출력 결과를 관리합니다."
                } else {
                    "연결된 호스트의 렌더링에 참여합니다."
                },
            );
            ui.add_space(8.0);
            if self.settings.host {
                text_row(ui, "수신 IP:포트", &mut self.settings.host_address, false);
                caption(ui, "0.0.0.0은 모든 네트워크에서 수신합니다.");
                ui.checkbox(&mut self.settings.local_worker, "이 PC도 렌더링에 참여");
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
            }
            ui.add_space(8.0);
            text_row(ui, "접속 암호 · 8자 이상", &mut self.settings.token, true);
            caption(ui, "호스트와 작업 PC에 같은 암호를 입력하세요.");
        });
        if self.running || self.pending_start.is_some() {
            caption(ui, "연결을 종료하면 연결 설정을 바꿀 수 있습니다.");
        }
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
        let wide = ctx.content_rect().width() >= 1000.0;
        let width = (ctx.content_rect().width() - 64.0).clamp(280.0, if wide { 760.0 } else { 520.0 });
        let body_height = (ctx.content_rect().height() - 240.0).clamp(160.0, 480.0);
        let response = modal.show(ctx, |ui| {
            ui.set_width(width);
            ui.set_opacity(reveal);
            ui.add_enabled_ui(self.settings_open, |ui| {
                if dialog_header(ui, "설정", "설정 닫기") {
                    self.settings_open = false;
                }
                caption(ui, "작업 환경과 렌더링 기본값을 관리합니다.");
                ui.add_space(layout::SPACE);
                let tabs = [(0, "화면"), (1, "렌더링"), (2, "연결"), (3, "앱과 업데이트")];
                if wide {
                    let (body, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), body_height), egui::Sense::hover());
                    let navigation = egui::Rect::from_min_size(body.min, egui::vec2(148.0, body.height()));
                    ui.painter().rect_filled(navigation, 6, secondary(ui));
                    let mut nav = ui.new_child(
                        egui::UiBuilder::new().id_salt("settings-navigation").max_rect(navigation.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    for (tab, label) in tabs {
                        let (rect, response) = nav.allocate_exact_size(egui::vec2(nav.available_width(), 40.0), egui::Sense::click());
                        let selected = self.settings_tab == tab;
                        if selected || response.hovered() {
                            nav.painter().rect_filled(rect, 5, if selected { nav.visuals().selection.bg_fill } else { surface(&nav) });
                        }
                        nav.painter().text(
                            rect.left_center() + egui::vec2(12.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(13.0),
                            if selected { accent(&nav) } else { muted(&nav) },
                        );
                        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.settings_tab = tab;
                        }
                    }
                    let content = egui::Rect::from_min_max(egui::pos2(navigation.right() + 24.0, body.top()), body.max);
                    let mut page =
                        ui.new_child(egui::UiBuilder::new().id_salt("settings-page").max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
                    page.set_clip_rect(content.intersect(ui.clip_rect()));
                    egui::ScrollArea::vertical().id_salt(("settings-body", self.settings_tab)).max_height(body_height).auto_shrink([false, false]).show(
                        &mut page,
                        |ui| {
                            self.settings_page(ui);
                        },
                    );
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for (tab, label) in tabs {
                            ui.selectable_value(&mut self.settings_tab, tab, label);
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("settings-body", self.settings_tab))
                        .max_height(body_height)
                        .min_scrolled_height(body_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.settings_page(ui));
                }
                ui.add_space(layout::SPACE);
                ui.separator();
                let clicks = footer_buttons(ui, None, None, Some(ButtonSpec::new("닫기", ButtonKind::Normal, true)), layout::CONTROL);
                if clicks.dismiss {
                    self.settings_open = false;
                }
                if let Some(rect) = clicks.free_left {
                    footer_status(ui, rect, "변경 사항은 자동으로 저장됩니다.", false);
                }
            });
        });
        if response.should_close() {
            self.settings_open = false;
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        ui.set_width(ui.available_width());
        let (title, description) = match self.settings_tab {
            0 => ("화면과 동작", "테마, 애니메이션과 미리보기 표시를 조정합니다."),
            1 => ("렌더링", "작업 PC의 성능과 최종 영상의 품질을 설정합니다."),
            2 => ("네트워크 연결", "호스트를 열거나 다른 PC에 연결할 주소를 설정합니다."),
            _ => ("앱과 업데이트", "설치된 엔진과 Render Farm 버전을 확인합니다."),
        };
        ui.label(egui::RichText::new(title).size(20.0).strong());
        caption(ui, description);
        ui.add_space(layout::GAP);
        if self.settings_tab == 0 {
            egui::Frame::new().fill(secondary(ui)).corner_radius(6).inner_margin(16).show(ui, |ui| {
                ui.set_width(ui.available_width());
                section_title(ui, "화면 동작");
                ui.horizontal(|ui| {
                    ui.label("테마");
                    ui.selectable_value(&mut self.settings.theme, settings::Theme::Dark, "다크");
                    ui.selectable_value(&mut self.settings.theme, settings::Theme::Light, "라이트");
                });
                ui.checkbox(&mut self.settings.reduced_motion, "동작 줄이기");
                caption(ui, "화면 전환과 진행률 애니메이션을 끕니다.");
            });
            ui.add_space(layout::GAP);
        }
        if self.settings_tab == 3 {
            self.about_settings(ui);
        } else {
            if self.running || self.pending_start.is_some() {
                egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(6).inner_margin(12).show(ui, |ui| {
                    caption(ui, "작업을 종료하면 렌더링과 연결 설정을 변경할 수 있습니다.");
                });
                ui.add_space(layout::SPACE);
            }
            ui.add_enabled_ui(!self.running && self.pending_start.is_none(), |ui| match self.settings_tab {
                0 => self.general_settings(ui),
                1 => self.render_settings(ui),
                _ => self.connection_settings(ui),
            });
        }
    }

    fn general_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "작업 화면 미리보기");
        ui.checkbox(&mut self.settings.render.previews, "PC별 프레임 미리보기");
        ui.add_enabled_ui(self.settings.render.previews, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("미리보기 주기");
                ui.add(egui::DragValue::new(&mut self.settings.render.preview_seconds).range(1..=30).suffix("초"));
            });
        });
    }

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "렌더 성능과 참여");
        ui.checkbox(&mut self.settings.render.automatic, "자동 최적화")
            .on_hover_text("GPU 렌더링을 기본으로 유지하고, 인코더·스레드 수·작업량을 PC 성능에 맞춰 조절합니다. GPU 렌더링 실패 시 CPU로 전환합니다.");
        ui.checkbox(&mut self.settings.local_worker, "호스트 PC도 렌더링에 참여");
        ui.add_enabled(!self.settings.render.automatic, egui::Checkbox::new(&mut self.settings.gpu, "수동: 이 PC에서 GPU 가속 사용"));
        ui.add_space(layout::GAP);
        section_title(ui, "품질과 출력");
        let options = &mut self.settings.render;
        ui.checkbox(&mut options.draft, "Draft 품질").on_hover_text("빠른 검토용 품질입니다. 끄면 Best 품질을 사용합니다.");
        ui.checkbox(&mut options.audio, "원본 오디오 포함");
        ui.horizontal_wrapped(|ui| {
            ui.label("H.264 비트레이트");
            ui.add(egui::DragValue::new(&mut options.video_bitrate).range(100..=200_000).speed(100).suffix("kbps"));
        });
        ui.collapsing("고급 설정 · 인코딩과 작업 분배", |ui| {
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
            if !options.automatic {
                ui.horizontal_wrapped(|ui| {
                    ui.label("동시 작업 PC");
                    ui.add(egui::DragValue::new(&mut self.settings.max_workers).range(1..=64).suffix("대"));
                });
            }
        });
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
        caption(ui, "CLI 경로");
        ui.label(egui::RichText::new(&self.settings.cli).small());
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
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 800.0));
            if dialog_header(ui, "작업 로그", "작업 로그 닫기") {
                self.logs_open = false;
            }
            caption(ui, "연결, 작업 배정과 렌더링 활동을 확인합니다.");
            ui.add_space(layout::SPACE);
            let wide = ui.available_width() >= 640.0;
            let mut copy = false;
            egui::Frame::new().fill(secondary(ui)).corner_radius(6).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if !wide {
                    ui.add_sized([ui.available_width(), layout::CONTROL], egui::TextEdit::singleline(&mut self.log_filter).hint_text("기록 검색"));
                }
                ui.horizontal(|ui| {
                    if wide {
                        let width = (ui.available_width() - 236.0).max(120.0);
                        ui.add_sized([width, layout::CONTROL], egui::TextEdit::singleline(&mut self.log_filter).hint_text("기록 검색"));
                    }
                    ui.checkbox(&mut self.log_debug, "상세 로그");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        copy = ui
                            .add_sized([112.0, layout::CONTROL], egui::Button::image_and_text(icon_image(ui, UiIcon::Copy), "기록 복사"))
                            .on_hover_text("현재 검색 결과에 표시된 기록을 복사합니다.")
                            .clicked();
                    });
                });
            });
            let messages = farm::snapshot(&self.state).4;
            let filtered: Vec<_> = messages.iter().rev().filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter)).collect();
            if copy {
                ctx.copy_text(filtered.iter().rev().map(|message| message.as_str()).collect::<Vec<_>>().join("\n"));
            }
            ui.horizontal(|ui| {
                caption(ui, format!("{}개 기록 표시", filtered.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    caption(ui, "최신순");
                });
            });
            let log_height = (ctx.content_rect().height() - if wide { 304.0 } else { 344.0 }).clamp(96.0, 440.0);
            egui::Frame::new().fill(background(ui)).corner_radius(6).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt("log-body").max_height(log_height).min_scrolled_height(log_height).auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        if filtered.is_empty() {
                            ui.add_space(32.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    egui::RichText::new(if self.log_filter.is_empty() {
                                        "아직 작업 기록이 없습니다"
                                    } else {
                                        "검색 결과가 없습니다"
                                    })
                                    .size(16.0)
                                    .strong(),
                                );
                                caption(
                                    ui,
                                    if self.log_filter.is_empty() {
                                        "호스트에 연결하거나 렌더링을 시작하면 기록이 표시됩니다."
                                    } else {
                                        "다른 검색어를 입력하거나 상세 로그를 켜 보세요."
                                    },
                                );
                            });
                        }
                        for message in filtered {
                            let (level, color) = if message.contains("[ERROR]") {
                                ("ERROR", danger(ui))
                            } else if message.contains("[WARN]") {
                                ("WARN", warning(ui))
                            } else if message.contains("[DEBUG]") {
                                ("DEBUG", muted(ui))
                            } else {
                                ("INFO", success(ui))
                            };
                            ui.horizontal_top(|ui| {
                                ui.add_sized(
                                    [48.0, 18.0],
                                    egui::Label::new(egui::RichText::new(level).monospace().size(10.0).color(color)).halign(egui::Align::Min),
                                );
                                let text = message.replacen(&format!("[{level}] "), "", 1);
                                ui.add(egui::Label::new(egui::RichText::new(text).size(12.0)).wrap()).on_hover_text(message);
                            });
                            ui.separator();
                        }
                    },
                );
            });
            ui.add_space(layout::SPACE);
            let clicks = footer_buttons(ui, None, None, Some(ButtonSpec::new("닫기", ButtonKind::Normal, true)), layout::CONTROL);
            if clicks.dismiss {
                self.logs_open = false;
            }
            if let Some(rect) = clicks.free_left {
                footer_status(ui, rect, "전체 기록은 로그 파일에 보관됩니다.", false);
            }
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
                ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 580.0));
                if dialog_header(ui, "컴퓨터 상세 정보", "컴퓨터 상세 정보 닫기") {
                    self.worker_details = None;
                }
                if let Some(worker) = worker {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(worker.label.split(" @ ").next().unwrap_or(&worker.label)).size(20.0).strong());
                        badge(ui, if worker.connected { "연결됨" } else { "연결 끊김" }, if worker.connected { success(ui) } else { muted(ui) });
                    });
                    caption(ui, &worker.ip);
                    ui.add_space(layout::SPACE);
                    egui::ScrollArea::vertical()
                        .id_salt("worker-body")
                        .max_height((ctx.content_rect().height() - 270.0).clamp(160.0, 560.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::Frame::new().fill(secondary(ui)).corner_radius(6).inner_margin(14).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let mut enabled = worker.enabled;
                                if ui.checkbox(&mut enabled, "이 PC를 렌더링에 사용").changed() {
                                    self.set_worker_participation(&worker, enabled);
                                }
                                if let Some((start, end)) = worker.assigned {
                                    caption(ui, format!("현재 구간 {start}–{end} · {} / {} 프레임", worker.current_done, end - start));
                                    ui.add(
                                        egui::ProgressBar::new((worker.current_done as f32 / (end - start).max(1) as f32).clamp(0.0, 1.0))
                                            .desired_width(ui.available_width())
                                            .desired_height(4.0)
                                            .fill(accent(ui)),
                                    );
                                } else {
                                    caption(
                                        ui,
                                        if enabled {
                                            "새 렌더 작업을 기다리고 있습니다."
                                        } else {
                                            "렌더링 참여에서 제외된 PC입니다."
                                        },
                                    );
                                }
                            });
                            ui.add_space(layout::SPACE);
                            egui::Frame::new().fill(secondary(ui)).corner_radius(6).inner_margin(14).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.columns(2, |columns| {
                                    metric(&mut columns[0], "누적 완료", &format!("{} 프레임", worker.completed_frames));
                                    metric(&mut columns[1], "처리 속도", &format!("{:.2} fps", worker.fps));
                                });
                                ui.separator();
                                layout::value_row(ui, "수신 데이터", &format!("{:.1} MiB", worker.transferred_bytes as f64 / 1048576.0));
                                layout::value_row(
                                    ui,
                                    "전송 속도",
                                    &format!("{:.1} MiB/s", worker.transferred_bytes as f64 / 1048576.0 / worker.transfer_seconds.max(0.001)),
                                );
                            });
                            ui.add_space(layout::SPACE);
                            section_title(ui, "렌더 환경");
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(if worker.profile.is_empty() {
                                        "작업을 받으면 렌더 환경이 표시됩니다."
                                    } else {
                                        &worker.profile
                                    })
                                    .size(12.0)
                                    .color(muted(ui)),
                                )
                                .wrap(),
                            );
                            if let Some(preview) = &worker.preview {
                                ui.add_space(layout::SPACE);
                                section_title(ui, "최근 렌더 프레임");
                                self.dashboard_image(ui, worker.id, Some(preview), if ctx.content_rect().height() < 800.0 { 156.0 } else { 200.0 });
                                caption(ui, format!("프레임 {}", preview.frame));
                            }
                            ui.add_space(layout::SPACE);
                        });
                } else {
                    ui.add_space(layout::GAP);
                    caption(ui, "이 PC의 연결 정보가 더 이상 없습니다.");
                    ui.add_space(layout::GAP);
                }
                ui.add_space(layout::SPACE);
                ui.separator();
                if footer_buttons(ui, None, None, Some(ButtonSpec::new("닫기", ButtonKind::Normal, true)), layout::CONTROL).dismiss {
                    self.worker_details = None;
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
