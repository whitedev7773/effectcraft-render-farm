#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod discovery;
mod encoding;
mod farm;
mod installation;
mod interface;
mod resources;
mod settings;
mod updater;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use farm::{CompInfo, Config, Job, Shared, State};
use settings::{Distribution, Settings};

fn install_ui_font(ctx: &egui::Context) -> Result<(), String> {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("wanted-sans".into(), std::sync::Arc::new(FontData::from_static(include_bytes!("../assets/ui/WantedSans-Regular.ttf"))));
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "wanted-sans".into());
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 9.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size.y = 34.0;
    style.visuals.panel_fill = egui::Color32::from_rgb(6, 6, 7);
    style.visuals.window_fill = egui::Color32::from_rgb(24, 24, 27);
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(35, 35, 37);
    style.visuals.faint_bg_color = egui::Color32::from_rgb(35, 35, 37);
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(252, 252, 252));
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 35, 37);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(48, 48, 51);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(58, 58, 62);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(40, 40, 44));
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(66, 66, 72));
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(4, 133, 247));
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(12);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(12);
    style.visuals.selection.bg_fill = egui::Color32::from_rgb(4, 93, 171);
    ctx.set_style_of(egui::Theme::Dark, style);
    Ok(())
}

fn path_row(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.label(egui::RichText::new(label).strong());
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 100.0).max(120.0);
        ui.add_sized([width, 27.0], egui::TextEdit::singleline(value));
        ui.button("찾아보기").clicked()
    })
    .inner
}

fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String, secret: bool) {
    ui.label(egui::RichText::new(label).strong());
    ui.add_sized([ui.available_width(), 27.0], egui::TextEdit::singleline(value).password(secret));
}

fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(18.0).strong().color(egui::Color32::from_rgb(230, 235, 242)));
}

fn primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(label).strong().color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(4, 133, 247))
}

#[derive(Clone, Copy)]
enum RangeHandle {
    Start,
    End,
}

fn range_bar(ui: &mut egui::Ui, start: &mut i64, end: &mut i64, total: i64, work: (i64, i64), handle: &mut Option<RangeHandle>) {
    if total < 1 {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(120.0), 44.0), egui::Sense::click_and_drag());
    let left = rect.left() + 12.0;
    let right = rect.right() - 12.0;
    let width = (right - left).max(1.0);
    let x = |frame: i64| left + width * (frame.clamp(0, total) as f32 / total as f32);
    let center = rect.center().y;
    let painter = ui.painter();
    painter.rect_filled(egui::Rect::from_min_max(egui::pos2(left, center - 5.0), egui::pos2(right, center + 5.0)), 5.0, egui::Color32::from_gray(67));
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(x(work.0), center - 5.0), egui::pos2(x(work.1), center + 5.0)),
        5.0,
        egui::Color32::from_rgb(82, 96, 111),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(x(*start), center - 5.0), egui::pos2(x(*end), center + 5.0)),
        5.0,
        egui::Color32::from_rgb(55, 146, 227),
    );
    for frame in [*start, *end] {
        painter.circle_filled(egui::pos2(x(frame), center), 9.0, egui::Color32::WHITE);
        painter.circle_stroke(egui::pos2(x(frame), center), 9.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(35, 103, 169)));
    }
    if (response.drag_started() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *handle = Some(if (pointer.x - x(*start)).abs() <= (pointer.x - x(*end)).abs() { RangeHandle::Start } else { RangeHandle::End });
    }
    if (response.dragged() || response.clicked())
        && let (Some(pointer), Some(selected)) = (response.interact_pointer_pos(), *handle)
    {
        let frame = (((pointer.x - left) / width).clamp(0.0, 1.0) * total as f32).round() as i64;
        match selected {
            RangeHandle::Start => *start = frame.clamp(0, end.saturating_sub(1)),
            RangeHandle::End => *end = frame.clamp(start.saturating_add(1), total),
        }
    }
    if response.drag_stopped() || response.clicked() {
        *handle = None;
    }
}

struct App {
    updater: updater::Updater,
    updater_logged_status: String,
    settings: Settings,
    running: bool,
    stopping: bool,
    stop: Arc<AtomicBool>,
    worker_active: Arc<AtomicBool>,
    state: Shared,
    job: Option<Job>,
    pending_start: Option<Receiver<Result<Job, String>>>,
    pending_config: Option<Config>,
    discovered_hosts: Vec<discovery::Host>,
    pending_discovery: Option<Receiver<Result<Vec<discovery::Host>, String>>>,
    last_discovery: Option<Instant>,
    discovery_status: String,
    last_error: String,
    last_logged_error: String,
    catalog_path: String,
    catalog_changed: Option<Instant>,
    pending_catalog: Option<Receiver<Result<Vec<CompInfo>, String>>>,
    catalog: Vec<CompInfo>,
    catalog_status: String,
    range_handle: Option<RangeHandle>,
    monitor: bool,
    settings_open: bool,
    settings_tab: u8,
    logs_open: bool,
    preview_open: bool,
    worker_details: Option<u64>,
    log_filter: String,
    log_debug: bool,
    textures: std::collections::BTreeMap<u64, (Arc<Vec<u8>>, egui::TextureHandle)>,
    inspect_frame: i64,
    pending_preview: Option<Receiver<Result<farm::Preview, String>>>,
    inspected: Option<farm::Preview>,
    preview_status: String,
}

impl Default for App {
    fn default() -> Self {
        let (settings, mut warning) = settings::load();
        let mut update_notice = None;
        if let Ok(path) = settings::update_result_path()
            && path.is_file()
        {
            if let Ok(result) = std::fs::read_to_string(&path) {
                let result = result.trim_start_matches('\u{feff}');
                if result.starts_with("Update failed:") {
                    warning = Some(format!("업데이트 적용 실패: {result}"));
                }
                update_notice = Some(result.to_owned());
            }
            let _ = std::fs::remove_file(path);
        }
        let state = Arc::new(Mutex::new(State::with_log_path(settings::log_path().ok())));
        if let Some(notice) = update_notice {
            state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("업데이트 결과: {notice}"));
        }
        Self::with_state(settings, warning, state)
    }
}

impl App {
    fn with_state(settings: Settings, warning: Option<String>, state: Shared) -> Self {
        let catalog_path = settings.project.clone();
        let catalog_changed = (!catalog_path.is_empty()).then(Instant::now);
        Self {
            updater: updater::Updater::default(),
            updater_logged_status: String::new(),
            settings,
            running: false,
            stopping: false,
            stop: Arc::new(AtomicBool::new(false)),
            worker_active: Arc::new(AtomicBool::new(false)),
            state,
            job: None,
            pending_start: None,
            pending_config: None,
            discovered_hosts: Vec::new(),
            pending_discovery: None,
            last_discovery: None,
            discovery_status: String::new(),
            last_error: warning.unwrap_or_default(),
            last_logged_error: String::new(),
            catalog_path,
            catalog_changed,
            pending_catalog: None,
            catalog: Vec::new(),
            catalog_status: String::new(),
            range_handle: None,
            monitor: false,
            settings_open: false,
            settings_tab: 0,
            logs_open: false,
            preview_open: false,
            worker_details: None,
            log_filter: String::new(),
            log_debug: false,
            textures: Default::default(),
            inspect_frame: 0,
            pending_preview: None,
            inspected: None,
            preview_status: String::new(),
        }
    }

    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let idle = !self.running && self.pending_start.is_none() && !self.worker_active.load(Ordering::Relaxed) && self.pending_preview.is_none();
        if let Some(result) = self.updater.poll(self.settings.check_updates) {
            let result = result.and_then(|prepared| {
                if !idle {
                    return Err("진행 중인 작업을 종료한 후 업데이트를 다시 선택하세요".into());
                }
                settings::save(&mut self.settings)?;
                prepared.launch()
            });
            match result {
                Ok(()) => {
                    self.updater.restarting = true;
                    self.updater.status = "업데이트 적용을 위해 앱을 종료하고 다시 시작합니다".into();
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(error) => self.updater.status = format!("업데이트 실패: {error}"),
            }
        }
        if self.updater.prompt
            && let Some(release) = self.updater.available.clone()
        {
            let mut consent = false;
            let mut later = false;
            let response = egui::Modal::new(egui::Id::new("update-consent")).show(ui.ctx(), |ui| {
                ui.set_width((ui.ctx().content_rect().width() - 80.0).clamp(280.0, 500.0));
                ui.heading(format!("{} → {}", env!("CARGO_PKG_VERSION"), release.tag_name));
                ui.label("동의하면 설치 프로그램을 다운로드·검증하고 Render Farm을 업데이트한 뒤 다시 시작합니다. EffectCraft 설치는 변경하지 않습니다.");
                ui.label("연결할 모든 PC를 같은 버전으로 업데이트하세요.");
                ui.hyperlink_to("GitHub 릴리스 보기", &release.html_url);
                egui::ScrollArea::vertical().id_salt("release_notes").max_height((ui.ctx().content_rect().height() - 320.0).clamp(80.0, 240.0)).show(
                    ui,
                    |ui| {
                        ui.label(release.body.as_deref().unwrap_or("등록된 변경 내용이 없습니다."));
                    },
                );
                if !idle {
                    ui.colored_label(egui::Color32::YELLOW, "호스트·클라이언트와 진행 중인 작업을 종료하면 업데이트할 수 있습니다.");
                }
                ui.horizontal_wrapped(|ui| {
                    consent = ui.add_enabled(idle && !self.updater.busy(), primary_button("동의하고 업데이트 · 재시작")).clicked();
                    later = ui.button("나중에").clicked();
                });
            });
            if response.should_close() || later {
                self.updater.prompt = false;
            }
            if consent {
                self.updater.consent_and_download();
            }
        }
        if self.updater.status != self.updater_logged_status {
            if !self.updater.status.is_empty() {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("업데이트: {}", self.updater.status));
            }
            self.updater_logged_status = self.updater.status.clone();
        }
    }

    fn show_preview(&mut self, ui: &mut egui::Ui, key: u64, preview: &farm::Preview) {
        self.show_preview_sized(ui, key, preview, 220.0);
    }

    fn show_preview_sized(&mut self, ui: &mut egui::Ui, key: u64, preview: &farm::Preview, max_height: f32) {
        let changed = self.textures.get(&key).is_none_or(|(bytes, _)| !Arc::ptr_eq(bytes, &preview.jpeg));
        if changed && let Ok(image) = image::load_from_memory(&preview.jpeg) {
            let rgba = image.to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
            let texture = ui.ctx().load_texture(format!("preview-{key}"), color, egui::TextureOptions::LINEAR);
            self.textures.insert(key, (preview.jpeg.clone(), texture));
        }
        if let Some((_, texture)) = self.textures.get(&key) {
            let size = egui::vec2(ui.available_width().min(max_height * 16.0 / 9.0), (ui.available_width() * 9.0 / 16.0).min(max_height));
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add(egui::Image::new(texture).fit_to_exact_size(size).maintain_aspect_ratio(true));
                ui.small(format!("프레임 {}", preview.frame));
            });
        }
    }

    fn sync_selected_comp_range(&mut self) {
        if let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp)
            && self.settings.range_comp != comp.name
        {
            self.settings.range_start = comp.work_start;
            self.settings.range_end = comp.work_end;
            self.settings.range_comp = comp.name.clone();
        }
    }

    fn missing_host_settings(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.settings.project.trim().is_empty() {
            missing.push("프로젝트 파일");
        }
        if self.settings.output.trim().is_empty() {
            missing.push("호스트 프레임 저장 폴더");
        }
        if self.settings.final_video.trim().is_empty() {
            missing.push("최종 MP4 저장 위치");
        }
        if self.settings.token.trim().len() < 8 {
            missing.push("8자 이상 접속 암호");
        }
        missing
    }

    fn poll_catalog(&mut self) {
        if !self.settings.host {
            return;
        }
        if self.catalog_path != self.settings.project {
            self.catalog_path = self.settings.project.clone();
            self.catalog_changed = Some(Instant::now());
            self.pending_catalog = None;
            self.catalog.clear();
            self.settings.range_comp.clear();
            self.catalog_status =
                if self.catalog_path.is_empty() { "프로젝트 파일을 선택하세요".into() } else { "컴포지션을 읽는 중…".into() };
        }
        let result = match self.pending_catalog.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err("프로젝트 읽기 작업이 중단됐습니다".into())),
            _ => None,
        };
        if let Some(result) = result {
            self.pending_catalog = None;
            match result {
                Ok(comps) => {
                    if !comps.iter().any(|comp| comp.name == self.settings.comp) {
                        self.settings.comp = comps.first().map(|comp| comp.name.clone()).unwrap_or_default();
                    }
                    self.catalog_status = format!("컴포지션 {}개", comps.len());
                    self.catalog = comps;
                    self.sync_selected_comp_range();
                }
                Err(error) => {
                    self.catalog.clear();
                    self.catalog_status = error;
                }
            }
        }
        if self.pending_catalog.is_none() && self.catalog_changed.is_some_and(|at| at.elapsed() >= Duration::from_millis(350)) && !self.catalog_path.is_empty()
        {
            self.catalog_changed = None;
            let path = PathBuf::from(&self.catalog_path);
            let (sender, receiver) = mpsc::channel();
            self.pending_catalog = Some(receiver);
            thread::spawn(move || {
                let result = std::panic::catch_unwind(|| farm::list_compositions(&path))
                    .unwrap_or_else(|_| Err("프로젝트 분석 중 예상하지 못한 오류가 발생했습니다".into()));
                let _ = sender.send(result);
            });
        }
    }

    fn start_discovery(&mut self) {
        let (sender, receiver) = mpsc::channel();
        self.pending_discovery = Some(receiver);
        self.last_discovery = Some(Instant::now());
        self.discovery_status = "같은 LAN의 호스트 검색 중…".into();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(discovery::scan).unwrap_or_else(|_| Err("LAN 검색 중 예상하지 못한 오류가 발생했습니다".into()));
            let _ = sender.send(result);
        });
    }

    fn poll_discovery(&mut self) {
        let result = match self.pending_discovery.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err("LAN 검색 작업이 중단됐습니다".into())),
            _ => None,
        };
        let Some(result) = result else { return };
        self.pending_discovery = None;
        match result {
            Ok(hosts) => {
                self.discovery_status = if hosts.is_empty() {
                    "검색된 호스트가 없습니다. IP를 직접 입력해 연결할 수 있습니다.".into()
                } else {
                    format!("호스트 {}대 검색됨", hosts.len())
                };
                self.discovered_hosts = hosts;
            }
            Err(error) => {
                self.discovery_status = format!("LAN 검색 실패: {error} · IP 직접 입력은 계속 사용할 수 있습니다.");
                self.discovered_hosts.clear();
            }
        }
    }

    fn config(&self) -> Config {
        let s = &self.settings;
        Config {
            project: PathBuf::from(&s.project),
            cli: PathBuf::from(&s.cli),
            output: PathBuf::from(&s.output),
            comp: s.comp.clone(),
            address: if s.host { s.host_address.clone() } else { s.client_address.clone() },
            token: s.token.clone(),
            gpu: s.gpu,
            range_start: if s.host { s.range_start } else { 0 },
            range_end: if s.host { s.range_end } else { 0 },
            chunk_frames: s.chunk_frames,
            max_workers: if s.render.automatic { 64 } else { s.max_workers },
            final_video: PathBuf::from(&s.final_video),
            disabled_workers: s.disabled_workers.clone(),
            render: s.render.clone(),
        }
    }

    fn start(&mut self) {
        self.last_error.clear();
        self.textures.clear();
        self.inspected = None;
        if self.settings.host {
            let output = PathBuf::from(&self.settings.output);
            if output.is_absolute()
                && !output.exists()
                && let Err(error) = std::fs::create_dir_all(&output)
            {
                self.last_error = format!("프레임 저장 폴더 만들기: {error}");
                return;
            }
            let missing = self.missing_host_settings();
            if !missing.is_empty() {
                self.last_error = format!("호스트 설정을 완료하세요: {}", missing.join(", "));
                return;
            }
        }
        let selected = self.catalog.iter().find(|comp| comp.name == self.settings.comp);
        let chunk_frames = if self.settings.host {
            let Some(comp) = selected else {
                self.last_error = "프로젝트의 컴포지션 목록을 읽고 렌더할 항목을 선택하세요".into();
                return;
            };
            match self.settings.effective_chunk_frames(comp.frames) {
                Ok(frames) => frames,
                Err(error) => {
                    self.last_error = error;
                    return;
                }
            }
        } else {
            self.settings.chunk_frames
        };
        if let Err(e) = settings::save(&mut self.settings) {
            self.last_error = e;
            return;
        }
        let mut config = self.config();
        config.chunk_frames = chunk_frames;
        self.stop = Arc::new(AtomicBool::new(false));
        self.stopping = false;
        if self.settings.host {
            let (sender, receiver) = mpsc::channel();
            self.pending_config = Some(config.clone());
            self.pending_start = Some(receiver);
            let state = self.state.clone();
            let stop = self.stop.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| farm::start_coordinator(config, state, stop)))
                    .unwrap_or_else(|_| Err("호스트 준비 중 예상하지 못한 오류가 발생했습니다".into()));
                let _ = sender.send(result);
            });
        } else {
            self.running = true;
            self.monitor = true;
            self.spawn_worker(config);
        }
    }

    fn poll_start(&mut self) {
        let result = match self.pending_start.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => Some(Err("호스트 준비 작업이 중단됐습니다".into())),
            _ => None,
        };
        let Some(result) = result else { return };
        self.pending_start = None;
        match result {
            Ok(job) => {
                self.inspect_frame = job.start;
                self.monitor = true;
                self.job = Some(job);
                self.running = true;
                let pending_config = self.pending_config.take();
                if self.settings.local_worker
                    && let Some(mut local) = pending_config
                {
                    let port = local.address.rsplit_once(':').map(|(_, port)| port).unwrap_or("9778");
                    local.address = format!("127.0.0.1:{port}");
                    local.range_start = 0;
                    local.range_end = 0;
                    self.spawn_worker(local);
                }
            }
            Err(e) => {
                self.last_error = e;
                self.pending_config = None;
            }
        }
    }

    fn spawn_worker(&self, config: Config) {
        let state = self.state.clone();
        let stop = self.stop.clone();
        let active = self.worker_active.clone();
        active.store(true, Ordering::Relaxed);
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| farm::run_worker(config, state.clone(), stop)))
                .unwrap_or_else(|_| Err("클라이언트 작업 중 예상하지 못한 오류가 발생했습니다".into()));
            if let Err(e) = result {
                let mut s = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                s.note(format!("클라이언트 오류: {e}"));
            }
            active.store(false, Ordering::Relaxed);
        });
    }

    fn save_settings(&mut self) {
        if let Err(e) = settings::save(&mut self.settings) {
            self.last_error = e;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        self.poll_start();
        self.poll_discovery();
        self.poll_catalog();
        if let Some(result) = self.pending_preview.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(value) => Some(value),
            Err(TryRecvError::Disconnected) => Some(Err("미리보기 작업이 중단됐습니다".into())),
            Err(TryRecvError::Empty) => None,
        }) {
            self.pending_preview = None;
            match result {
                Ok(preview) => {
                    self.inspected = Some(preview);
                    self.preview_status.clear();
                }
                Err(error) => {
                    self.inspected = None;
                    self.preview_status = error;
                }
            }
        }
        if self.running && !self.settings.host && !self.worker_active.load(Ordering::Relaxed) {
            self.running = false;
            self.stopping = false;
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(message) = state.messages.last().filter(|message| message.contains("클라이언트 오류:")) {
                self.last_error = message.clone();
            }
        }
        if !self.settings.host
            && !self.running
            && self.pending_discovery.is_none()
            && self.last_discovery.is_none_or(|last| last.elapsed() >= Duration::from_secs(10))
        {
            self.start_discovery();
        }
        let before = self.settings.clone();
        if self.stopping {
            let connected = farm::progress_snapshot(&self.state).2;
            if connected == 0 && !self.worker_active.load(Ordering::Relaxed) {
                self.running = false;
                self.stopping = false;
            }
        }
        self.draw_interface(ui);
        self.update_ui(ui);
        if self.last_error != self.last_logged_error {
            if !self.last_error.is_empty() && !self.last_error.starts_with("클라이언트 오류:") {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("화면 오류: {}", self.last_error));
            }
            self.last_logged_error = self.last_error.clone();
        }
        if self.settings != before {
            self.save_settings();
        }
    }
}

fn main() {
    #[cfg(debug_assertions)]
    if interface::review::run_requested() {
        return;
    }
    let (saved, _) = settings::load();
    let installation = match installation::detect(&saved.cli) {
        Ok(installation) => installation,
        Err(reason) => {
            State::with_log_path(settings::log_path().ok()).note(format!("EffectCraft 설치 확인 실패: {reason}"));
            installation::guide_and_exit_reason(&reason);
            return;
        }
    };
    let icon = match eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon/farm-256.png")) {
        Ok(icon) => icon,
        Err(error) => {
            eprintln!("Render Farm icon: {error}");
            return;
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EffectCraft Render Farm")
            .with_icon(icon)
            .with_inner_size([1440.0, 940.0])
            .with_min_inner_size([560.0, 520.0]),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "EffectCraft Render Farm",
        options,
        Box::new(move |cc| {
            let mut app = App::default();
            app.settings.cli = installation.cli.display().to_string();
            app.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("설치된 EffectCraft 감지: {}", installation.root.display()));
            app.save_settings();
            if let Err(error) = install_ui_font(&cc.egui_ctx) {
                app.last_error = error;
            }
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("EffectCraft Render Farm could not start: {e}");
    }
}
