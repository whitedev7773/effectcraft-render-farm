#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod discovery;
mod farm;
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
    let mut candidates = Vec::new();
    if cfg!(windows) {
        let windows = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        candidates.push(windows.join("Fonts").join("malgun.ttf"));
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/System/Library/Fonts/AppleSDGothicNeo.ttc"));
    } else {
        candidates.extend(["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/truetype/nanum/NanumGothic.ttf"].map(PathBuf::from));
    }
    let bytes = candidates
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find(|bytes| !bytes.is_empty() && bytes.len() <= 40_000_000)
        .ok_or_else(|| "한글 글꼴을 찾지 못했습니다. 시스템에 한글 글꼴을 설치한 뒤 다시 실행하세요.".to_string())?;
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("korean-system".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("korean-system".into());
    }
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 9.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
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
    egui::Button::new(egui::RichText::new(label).strong().color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(35, 103, 169))
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
        let catalog_path = settings.project.clone();
        let catalog_changed = (!catalog_path.is_empty()).then(Instant::now);
        let state = Arc::new(Mutex::new(State::with_log_path(settings::log_path().ok())));
        if let Some(notice) = update_notice {
            state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("업데이트 결과: {notice}"));
        }
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
            log_filter: String::new(),
            log_debug: false,
            textures: Default::default(),
            inspect_frame: 0,
            pending_preview: None,
            inspected: None,
            preview_status: String::new(),
        }
    }
}

impl App {
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
        ui.horizontal_wrapped(|ui| {
            ui.small(format!("v{}", env!("CARGO_PKG_VERSION")));
            if ui.add_enabled(!self.updater.busy(), egui::Button::new("업데이트 확인")).clicked() {
                self.updater.check();
            }
            ui.checkbox(&mut self.settings.check_updates, "앱 실행 시 새 버전 확인");
            if self.updater.busy() {
                ui.spinner();
            }
            ui.small(&self.updater.status);
            if self.updater.installing() {
                ui.small(format!("다운로드 {:.1} MiB", self.updater.progress.load(Ordering::Relaxed) as f64 / 1048576.0));
            }
            if self.updater.available.is_some() && !self.updater.busy() && ui.button("업데이트 보기").clicked() {
                self.updater.prompt = true;
            }
        });
        if self.updater.prompt
            && let Some(release) = self.updater.available.clone()
        {
            let mut open = true;
            let mut consent = false;
            let mut later = false;
            egui::Window::new("새 버전 업데이트")
                .collapsible(false)
                .resizable(false)
                .default_width(500.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.heading(format!("{} → {}", env!("CARGO_PKG_VERSION"), release.tag_name));
                    ui.label("동의하면 업데이트를 다운로드하고 검증한 뒤 앱과 CLI를 교체하고 다시 시작합니다.");
                    ui.label("연결할 모든 PC를 같은 버전으로 업데이트하세요.");
                    ui.hyperlink_to("GitHub 릴리스 보기", &release.html_url);
                    egui::ScrollArea::vertical().id_salt("release_notes").max_height(240.0).show(ui, |ui| {
                        ui.label(release.body.as_deref().unwrap_or("등록된 변경 내용이 없습니다."));
                    });
                    if !idle {
                        ui.colored_label(egui::Color32::YELLOW, "호스트·클라이언트와 진행 중인 작업을 종료하면 업데이트할 수 있습니다.");
                    }
                    ui.horizontal(|ui| {
                        consent = ui.add_enabled(idle && !self.updater.busy(), primary_button("동의하고 업데이트 · 재시작")).clicked();
                        later = ui.button("나중에").clicked();
                    });
                });
            if !open || later {
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
        let changed = self.textures.get(&key).is_none_or(|(bytes, _)| !Arc::ptr_eq(bytes, &preview.jpeg));
        if changed && let Ok(image) = image::load_from_memory(&preview.jpeg) {
            let rgba = image.to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
            let texture = ui.ctx().load_texture(format!("preview-{key}"), color, egui::TextureOptions::LINEAR);
            self.textures.insert(key, (preview.jpeg.clone(), texture));
        }
        if let Some((_, texture)) = self.textures.get(&key) {
            ui.add(egui::Image::new(texture).max_width(360.0).max_height(203.0));
            ui.small(format!("프레임 {} · 480px 미리보기", preview.frame));
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
            max_workers: s.max_workers,
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
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(egui::RichText::new("EffectCraft · Render Farm").size(23.0).strong().color(egui::Color32::from_rgb(242, 245, 250)));
            ui.label(if self.settings.host {
                "호스트가 프레임을 배분하고 원본 FPS·해상도로 최종 MP4를 만듭니다."
            } else {
                "호스트의 작업 지시에 따라 렌더링하고, 프레임을 전송한 뒤 임시 파일을 삭제합니다."
            });
            ui.add_space(8.0);
            self.update_ui(ui);
            ui.horizontal(|ui| {
                if !self.running
                    && !self.updater.installing()
                    && self.pending_start.is_none()
                    && ui.add(primary_button(if self.settings.host { "호스트 열기" } else { "호스트에 연결" })).clicked()
                {
                    self.start();
                }
                if self.pending_start.is_some() {
                    ui.label("호스트 준비 중…");
                }
                if self.running && self.settings.host && !self.stopping {
                    let started = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
                    if !started
                        && ui.add(primary_button("렌더 시작")).clicked()
                        && let Some(job) = &self.job
                    {
                        farm::begin_render(&self.config(), job, &self.state);
                    }
                }
                if self.running
                    && !self.settings.host
                    && !self.worker_active.load(Ordering::Relaxed)
                    && !self.stopping
                    && ui.add(primary_button("다시 연결")).clicked()
                {
                    self.spawn_worker(self.config());
                }
                if self.running && !self.stopping && ui.button("중지").clicked() {
                    self.stop.store(true, Ordering::Relaxed);
                    self.stopping = true;
                }
                if self.running {
                    ui.label(if self.stopping {
                        "중지 중"
                    } else if self.settings.host {
                        "호스트 실행 중"
                    } else {
                        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        match state.local_progress {
                            Some((start, end, done)) if done < end - start => "클라이언트 프레임 렌더링 중",
                            Some(_) => "클라이언트 준비 완료 · 작업 대기",
                            None => match state.messages.last().map(String::as_str) {
                                Some(message) if message.contains("렌더링:") => "클라이언트 프레임 렌더링 중",
                                Some(message) if message.contains("렌더 준비 완료:") || message.contains("구간 ") => "클라이언트 준비 완료 · 작업 대기",
                                _ => "클라이언트 연결 및 리소스 검사 중",
                            },
                        }
                    });
                }
            });
            if self.running && self.settings.host {
                ui.small(format!("TCP 수신 중: {} · 클라이언트에는 이 PC의 고정 LAN IP와 위 수신 포트를 입력하세요", self.settings.host_address));
            } else if !self.settings.host && self.settings.client_address.trim_start().starts_with("127.") {
                ui.colored_label(egui::Color32::YELLOW, "127.0.0.1은 이 클라이언트 PC 자신입니다. 호스트가 다른 PC라면 호스트 PC의 고정 LAN IP를 입력하세요.");
            }
            if !self.last_error.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.last_error);
            }
            if self.settings.host && !self.running {
                let missing = self.missing_host_settings();
                if !missing.is_empty() {
                    ui.colored_label(egui::Color32::YELLOW, format!("호스트 준비에 필요: {}", missing.join(", ")));
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.monitor, false, "작업 설정");
                ui.selectable_value(&mut self.monitor, true, "렌더 모니터");
                if self.running {
                    ui.small("실행 중 · 설정 변경은 종료 후 적용");
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if !self.monitor {
                    ui.add_enabled_ui(!self.running && self.pending_start.is_none(), |ui| {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            section_title(ui, "1 · 역할과 연결");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.settings.host, true, "호스트 · 작업 배분 및 영상 합치기");
                                ui.selectable_value(&mut self.settings.host, false, "클라이언트 · 프레임 렌더링");
                            });
                            ui.add_space(4.0);
                            if self.settings.host {
                                text_row(ui, "수신할 IP:포트", &mut self.settings.host_address, false);
                                ui.small("0.0.0.0은 이 PC의 모든 네트워크 주소에서 연결을 받습니다.");
                            } else {
                                text_row(ui, "호스트의 고정 IP:포트", &mut self.settings.client_address, false);
                                ui.small("예: 192.168.0.10:9778 · 호스트에서 '호스트 열기'를 완료한 뒤 연결하세요.");
                                ui.horizontal(|ui| {
                                    if ui.button("LAN에서 다시 검색").clicked() && self.pending_discovery.is_none() {
                                        self.start_discovery();
                                    }
                                    ui.small(&self.discovery_status);
                                });
                                for host in &self.discovered_hosts {
                                    let address = host.address.to_string();
                                    if ui.selectable_label(self.settings.client_address == address, format!("{} · {address}", host.name)).clicked() {
                                        self.settings.client_address = address;
                                    }
                                }
                            }
                            text_row(ui, "접속 암호", &mut self.settings.token, true);
                        });
                        ui.add_space(8.0);
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            section_title(ui, if self.settings.host { "2 · 프로젝트와 출력 폴더" } else { "2 · 클라이언트 렌더 준비" });
                            if self.settings.host {
                                if path_row(ui, "EffectCraft 프로젝트 (.ecproj)", &mut self.settings.project)
                                    && let Some(path) = rfd::FileDialog::new().add_filter("EffectCraft project", &["ecproj"]).pick_file()
                                {
                                    self.settings.project = path.display().to_string();
                                }
                                ui.label(egui::RichText::new("렌더할 컴포지션").strong());
                                let previous_comp = self.settings.comp.clone();
                                egui::ComboBox::from_id_salt("composition")
                                    .selected_text(if self.settings.comp.is_empty() { "컴포지션 선택" } else { &self.settings.comp })
                                    .show_ui(ui, |ui| {
                                        for comp in &self.catalog {
                                            ui.selectable_value(&mut self.settings.comp, comp.name.clone(), &comp.name);
                                        }
                                    });
                                if self.settings.comp != previous_comp {
                                    self.sync_selected_comp_range();
                                }
                                if let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp) {
                                    ui.small(format!("{}프레임 · {:.3} FPS · {}×{}", comp.frames, comp.rate.as_f64(), comp.width, comp.height));
                                } else {
                                    ui.small(&self.catalog_status);
                                }
                                if ui.button("프로젝트 목록 새로고침").clicked() {
                                    self.catalog_changed = Some(Instant::now());
                                }
                            } else {
                                ui.small("프로젝트·소스·글꼴·컴포지션·프레임 범위는 호스트가 렌더 시작 시 전송합니다.");
                            }
                            if path_row(ui, "렌더 CLI 실행 파일", &mut self.settings.cli)
                                && let Some(path) = rfd::FileDialog::new().pick_file()
                            {
                                self.settings.cli = path.display().to_string();
                            }
                            if self.settings.host
                                && path_row(ui, "호스트 프레임 저장 폴더", &mut self.settings.output)
                                && let Some(path) = rfd::FileDialog::new().pick_folder()
                            {
                                self.settings.output = path.display().to_string();
                            }
                            ui.checkbox(&mut self.settings.gpu, "이 PC에서 GPU 렌더링 사용");
                        });
                        if self.settings.host {
                            ui.add_space(8.0);
                            ui.group(|ui| {
                                ui.set_width(ui.available_width());
                                section_title(ui, "3 · 렌더 범위와 분배");
                                let selected = self.catalog.iter().find(|comp| comp.name == self.settings.comp);
                                let total = selected.map_or(100_000, |comp| comp.frames.max(1));
                                if let Some(comp) = selected {
                                    ui.small(format!("작업 영역: {}–{} / 전체 {}프레임", comp.work_start, comp.work_end, comp.frames));
                                    let display_end = if self.settings.range_end == 0 { total } else { self.settings.range_end };
                                    self.settings.range_end = display_end.clamp(1, total);
                                    self.settings.range_start = self.settings.range_start.clamp(0, self.settings.range_end - 1);
                                    range_bar(
                                        ui,
                                        &mut self.settings.range_start,
                                        &mut self.settings.range_end,
                                        total,
                                        (comp.work_start, comp.work_end),
                                        &mut self.range_handle,
                                    );
                                    ui.horizontal(|ui| {
                                        if ui.button("작업 영역으로 초기화").clicked() {
                                            self.settings.range_start = comp.work_start;
                                            self.settings.range_end = comp.work_end;
                                        }
                                        if ui.button("전체 컴포지션").clicked() {
                                            self.settings.range_start = 0;
                                            self.settings.range_end = total;
                                        }
                                    });
                                } else {
                                    ui.small("프로젝트와 컴포지션을 선택하면 렌더 범위 바가 표시됩니다.");
                                }
                                egui::Grid::new("render_options").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                                    ui.label("시작 프레임");
                                    ui.add(egui::DragValue::new(&mut self.settings.range_start).range(0..=total.saturating_sub(1)));
                                    ui.end_row();
                                    ui.label("끝 프레임 · 미포함");
                                    ui.add(egui::DragValue::new(&mut self.settings.range_end).range(1..=total));
                                    ui.end_row();
                                    ui.label("동시에 사용할 PC 수");
                                    ui.add(egui::DragValue::new(&mut self.settings.max_workers).range(1..=64));
                                    ui.end_row();
                                });
                                if selected.is_some() {
                                    self.settings.range_end = self.settings.range_end.clamp(1, total);
                                    self.settings.range_start = self.settings.range_start.clamp(0, self.settings.range_end - 1);
                                }
                                let has_selection = selected.is_some();
                                ui.horizontal(|ui| {
                                    ui.label("범위 미리보기 프레임");
                                    ui.add(
                                        egui::DragValue::new(&mut self.inspect_frame)
                                            .range(self.settings.range_start..=self.settings.range_end.saturating_sub(1).max(self.settings.range_start)),
                                    );
                                    if ui.add_enabled(self.pending_preview.is_none() && has_selection, egui::Button::new("선택 프레임 시험 렌더")).clicked()
                                    {
                                        let config = self.config();
                                        let frame = self.inspect_frame;
                                        let (tx, rx) = mpsc::channel();
                                        self.pending_preview = Some(rx);
                                        self.preview_status = "미리보기 렌더링 중…".into();
                                        thread::spawn(move || {
                                            let _ = tx.send(farm::render_preview(&config, frame));
                                        });
                                    }
                                });
                                ui.small(&self.preview_status);
                                if let Some(preview) = self.inspected.clone() {
                                    self.show_preview(ui, u64::MAX, &preview);
                                }
                                ui.horizontal(|ui| {
                                    ui.label("구간 분배");
                                    ui.selectable_value(&mut self.settings.distribution, Distribution::Auto, "자동 · 빠른 PC가 남은 작업 지원");
                                    ui.selectable_value(&mut self.settings.distribution, Distribution::Frames, "프레임");
                                    ui.selectable_value(&mut self.settings.distribution, Distribution::Percent, "퍼센트");
                                });
                                match self.settings.distribution {
                                    Distribution::Auto => {}
                                    Distribution::Frames => {
                                        ui.horizontal(|ui| {
                                            ui.label("구간당 프레임");
                                            ui.add(egui::DragValue::new(&mut self.settings.chunk_frames).range(1..=100_000));
                                        });
                                    }
                                    Distribution::Percent => {
                                        ui.horizontal(|ui| {
                                            ui.label("전체 선택 범위 중 구간당 비율");
                                            ui.add(egui::DragValue::new(&mut self.settings.chunk_percent).range(0.01..=100.0).suffix("%"));
                                        });
                                    }
                                }
                                if let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp)
                                    && let Ok(frames) = self.settings.effective_chunk_frames(comp.frames)
                                {
                                    let end = if self.settings.range_end == 0 { comp.frames } else { self.settings.range_end };
                                    let span = end - self.settings.range_start;
                                    let chunks = (span + frames - 1) / frames;
                                    ui.small(format!(
                                        "{}–{} · 총 {}프레임 → 구간당 최대 {}프레임, {}개 구간",
                                        self.settings.range_start, end, span, frames, chunks
                                    ));
                                }
                                ui.checkbox(&mut self.settings.local_worker, "호스트 PC도 렌더링에 참여");
                                if path_row(ui, "최종 MP4 저장 위치", &mut self.settings.final_video)
                                    && let Some(path) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).save_file()
                                {
                                    self.settings.final_video = path.display().to_string();
                                }
                                ui.collapsing("성능 · 품질 · 미리보기 옵션", |ui| {
                                    let options = &mut self.settings.render;
                                    ui.checkbox(&mut options.adaptive, "PC 처리 속도에 맞춰 대기 구간 자동 분할");
                                    ui.horizontal(|ui| {
                                        ui.label("목표 작업 시간");
                                        ui.add(egui::DragValue::new(&mut options.target_seconds).range(5..=300).suffix("초"));
                                        ui.label("실패 시 최대 시도");
                                        ui.add(egui::DragValue::new(&mut options.retries).range(1..=10).suffix("회"));
                                    });
                                    ui.checkbox(&mut options.previews, "PC별 실시간 프레임 미리보기");
                                    ui.horizontal(|ui| {
                                        ui.label("미리보기 주기");
                                        ui.add(egui::DragValue::new(&mut options.preview_seconds).range(1..=30).suffix("초"));
                                        ui.label("프레임 감지 주기");
                                        ui.add(egui::DragValue::new(&mut options.poll_ms).range(50..=2000).suffix("ms"));
                                    });
                                    ui.checkbox(&mut options.draft, "Draft 품질 · 검토용 빠른 렌더링");
                                    ui.small("기본값은 Best입니다. Draft는 일부 효과의 품질에 영향을 줄 수 있습니다. FPS와 해상도는 원본을 유지합니다.");
                                    ui.horizontal(|ui| {
                                        ui.label("최종 H.264 비트레이트");
                                        ui.add(egui::DragValue::new(&mut options.video_bitrate).range(100..=200_000).speed(100).suffix("kbps"));
                                        ui.checkbox(&mut options.audio, "원본 오디오 포함");
                                    });
                                    ui.checkbox(&mut options.durable_writes, "수신 프레임마다 디스크 동기화 · 전원 장애 대비, 저장 속도 감소");
                                    ui.small(
                                        "작은 구간은 PC 간 부하를 고르게 하지만 CLI 시작 비용이 늘어납니다. 소스가 무거우면 프레임 분배로 구간을 늘리세요.",
                                    );
                                });
                            });
                        }
                    });
                }
                ui.add_space(10.0);
                ui.separator();
                let (done, total, connected, failed, messages, _) = farm::snapshot(&self.state);
                let (workers, final_status) = farm::worker_snapshot(&self.state);
                if self.stopping {
                    ui.label("중지 요청됨 · 진행 중인 구간이 끝나기를 기다리는 중");
                    if connected == 0 && !self.worker_active.load(Ordering::Relaxed) {
                        self.running = false;
                        self.stopping = false;
                    }
                }
                if self.settings.host && total > 0 {
                    section_title(ui, "진행 상황");
                    ui.add(
                        egui::ProgressBar::new((done as f32 / total as f32).clamp(0.0, 1.0))
                            .text(format!("전체 {done} / {total} 프레임 · {:.1}%", done as f64 * 100.0 / total as f64)),
                    );
                    ui.label(format!("연결된 PC: {connected} · 실패한 구간: {failed} · 동시 사용 한도: {}", self.settings.max_workers));
                    ui.label(&final_status);
                    let (elapsed, active) = {
                        let s = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        (
                            s.started.map(|at| s.finished_elapsed.unwrap_or_else(|| at.elapsed()).as_secs_f64()),
                            workers.iter().filter(|w| w.assigned.is_some()).count(),
                        )
                    };
                    if let Some(elapsed) = elapsed {
                        let speed = workers
                            .iter()
                            .map(|w| w.started.filter(|_| w.assigned.is_some()).map_or(0.0, |at| w.current_done as f64 / at.elapsed().as_secs_f64().max(0.001)))
                            .sum::<f64>();
                        let eta = if done >= total {
                            "프레임 완료".into()
                        } else if speed > 0.0 {
                            format!("약 {:.0}초", (total - done) as f64 / speed)
                        } else {
                            "계산 중".into()
                        };
                        ui.horizontal(|ui| {
                            ui.strong(format!("경과 {:.0}초", elapsed));
                            ui.separator();
                            ui.label(format!("처리 {:.2} FPS", speed));
                            ui.separator();
                            ui.label(format!("남은 시간 {eta}"));
                            ui.separator();
                            ui.label(format!("작업 중 {active}대"));
                        });
                    }
                    let segments = farm::timeline(&self.state);
                    if let (Some(first), Some(last)) = (segments.iter().map(|s| s.0).min(), segments.iter().map(|s| s.1).max()) {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
                        let x = |frame: i64| rect.left() + (frame - first) as f32 / (last - first).max(1) as f32 * rect.width();
                        for (start, end, status) in segments {
                            let color = match status {
                                1 => egui::Color32::from_rgb(80, 150, 250),
                                2 => egui::Color32::from_rgb(55, 185, 135),
                                3 => egui::Color32::from_rgb(235, 85, 100),
                                _ => egui::Color32::from_gray(65),
                            };
                            ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x(start), rect.top()), egui::pos2(x(end), rect.bottom())), 0.0, color);
                        }
                        ui.small(format!("{first}–{last} · 회색 대기 / 파랑 진행 / 초록 완료 / 빨강 실패 · 끝 프레임 미포함"));
                        ui.collapsing("완료 프레임 탐색", |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut self.inspect_frame, first..=last - 1).text("프레임"));
                                if ui.add_enabled(self.pending_preview.is_none(), egui::Button::new("미리보기 열기")).clicked() {
                                    let output = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).output.clone();
                                    let frame = self.inspect_frame;
                                    let (tx, rx) = mpsc::channel();
                                    self.pending_preview = Some(rx);
                                    thread::spawn(move || {
                                        let _ = tx.send(farm::preview_frame(&output, frame));
                                    });
                                }
                            });
                            ui.small(&self.preview_status);
                            if let Some(preview) = self.inspected.clone() {
                                self.show_preview(ui, u64::MAX, &preview);
                            }
                        });
                    }
                }
                if !self.settings.host
                    && let Some((start, end, done)) = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_progress
                {
                    let span = end - start;
                    if span > 0 {
                        section_title(ui, "이 PC 렌더 진행률");
                        ui.add(egui::ProgressBar::new((done as f32 / span as f32).clamp(0.0, 1.0)).text(format!("{start}–{end} · {done} / {span} 프레임")));
                    }
                }
                if !self.settings.host {
                    let preview = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).local_preview.clone();
                    if let Some(preview) = preview {
                        self.show_preview(ui, 0, &preview);
                    }
                }
                if self.settings.host {
                    ui.separator();
                    section_title(ui, "PC별 진행률");
                    if !workers.iter().any(|worker| worker.connected) {
                        ui.label("연결된 PC가 없습니다. 클라이언트에서 이 호스트의 고정 IP로 연결하세요.");
                    }
                    for worker in &workers {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut enabled = worker.enabled;
                                if ui.checkbox(&mut enabled, &worker.label).changed()
                                    && let Some(ip) = farm::set_worker_enabled(&self.state, worker.id, enabled)
                                {
                                    self.settings.disabled_workers.retain(|saved| saved != &ip && saved != &worker.label);
                                    if !enabled {
                                        self.settings.disabled_workers.push(ip);
                                    }
                                }
                                if !worker.connected {
                                    ui.small("연결 끊김");
                                }
                            });
                            if let Some((start, end)) = worker.assigned {
                                let span = end - start;
                                if span > 0 {
                                    ui.add(
                                        egui::ProgressBar::new((worker.current_done as f32 / span as f32).clamp(0.0, 1.0))
                                            .text(format!("현재 {start}–{end} · {} / {span} 프레임", worker.current_done)),
                                    );
                                }
                            } else {
                                ui.small("현재 작업 대기");
                            }
                            ui.small(format!("이 PC에서 완료: {}프레임", worker.completed_frames));
                            let fps = worker
                                .started
                                .filter(|_| worker.assigned.is_some())
                                .map_or(worker.fps, |at| worker.current_done as f64 / at.elapsed().as_secs_f64().max(0.001));
                            let network = worker.transferred_bytes as f64 / 1048576.0 / worker.transfer_seconds.max(0.001);
                            ui.small(format!("{fps:.2} FPS · 수신 {:.1} MiB · {network:.1} MiB/s", worker.transferred_bytes as f64 / 1048576.0));
                            if let Some((start, end)) = worker.assigned {
                                ui.small(format!("다음 렌더 프레임: {} · 구간 {}–{}", (start + worker.current_done).min(end - 1), start, end));
                            }
                            if let Some(preview) = &worker.preview {
                                self.show_preview(ui, worker.id, preview);
                            }
                        });
                    }
                }
                ui.separator();
                section_title(ui, "작업 기록");
                ui.horizontal(|ui| {
                    ui.label("검색");
                    ui.text_edit_singleline(&mut self.log_filter);
                    ui.checkbox(&mut self.log_debug, "DEBUG 포함");
                    if ui.button("표시 로그 복사").clicked() {
                        ui.ctx().copy_text(
                            messages
                                .iter()
                                .filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                });
                egui::ScrollArea::vertical().id_salt("farm_log").max_height(170.0).show(ui, |ui| {
                    for message in messages.iter().rev().filter(|m| (self.log_debug || !m.contains("[DEBUG]")) && m.contains(&self.log_filter)) {
                        let color = if message.contains("[ERROR]") {
                            egui::Color32::LIGHT_RED
                        } else if message.contains("[WARN]") {
                            egui::Color32::YELLOW
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.colored_label(color, message);
                    }
                });
                ui.separator();
                ui.small("클라이언트가 호스트의 로컬 IP로 연결합니다. 프로젝트와 참조 소스는 호스트가 전송합니다.");
                ui.small("설정은 이 PC의 EffectCraft 사용자 설정 폴더에 저장됩니다.");
                if let Ok(path) = settings::log_path() {
                    ui.small(format!("로그 파일: {}", path.display()));
                }
            });
        });
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("EffectCraft Render Farm").with_inner_size([1120.0, 860.0]).with_min_inner_size([760.0, 600.0]),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "EffectCraft Render Farm",
        options,
        Box::new(|cc| {
            let mut app = App::default();
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
                style.spacing.item_spacing = egui::vec2(10.0, 8.0);
                style.spacing.button_padding = egui::vec2(12.0, 8.0);
                style.visuals.selection.bg_fill = egui::Color32::from_rgb(45, 100, 175);
            });
            if let Err(error) = install_ui_font(&cc.egui_ctx) {
                app.last_error = error;
            }
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("EffectCraft Render Farm could not start: {e}");
    }
}
