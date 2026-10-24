#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod discovery;
mod farm;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use farm::{Config, Job, Shared, State};
use settings::Settings;

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

struct App {
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
}

impl Default for App {
    fn default() -> Self {
        let (settings, warning) = settings::load();
        Self {
            settings,
            running: false,
            stopping: false,
            stop: Arc::new(AtomicBool::new(false)),
            worker_active: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(State::default())),
            job: None,
            pending_start: None,
            pending_config: None,
            discovered_hosts: Vec::new(),
            pending_discovery: None,
            last_discovery: None,
            discovery_status: String::new(),
            last_error: warning.unwrap_or_default(),
        }
    }
}

impl App {
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
        }
    }

    fn start(&mut self) {
        self.last_error.clear();
        if let Err(e) = settings::save(&mut self.settings) {
            self.last_error = e;
            return;
        }
        let config = self.config();
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
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        self.poll_start();
        self.poll_discovery();
        if self.running && !self.settings.host && !self.worker_active.load(Ordering::Relaxed) {
            self.running = false;
            self.stopping = false;
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(message) = state.messages.last().filter(|message| message.starts_with("클라이언트 오류:")) {
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
            ui.label("여러 PC에서 프레임을 렌더링하고, 호스트에서 원본 FPS·해상도로 최종 MP4를 만듭니다.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if !self.running
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
                        "클라이언트 연결 중"
                    });
                }
            });
            if !self.last_error.is_empty() {
                ui.colored_label(egui::Color32::RED, &self.last_error);
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
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
                        section_title(ui, "2 · 프로젝트와 공유 폴더");
                        if path_row(ui, "EffectCraft 프로젝트 (.ecproj)", &mut self.settings.project)
                            && let Some(path) = rfd::FileDialog::new().add_filter("EffectCraft project", &["ecproj"]).pick_file()
                        {
                            self.settings.project = path.display().to_string();
                        }
                        text_row(ui, "렌더할 컴포지션", &mut self.settings.comp, false);
                        if path_row(ui, "렌더 CLI 실행 파일", &mut self.settings.cli)
                            && let Some(path) = rfd::FileDialog::new().pick_file()
                        {
                            self.settings.cli = path.display().to_string();
                        }
                        if path_row(ui, "공유 프레임 폴더 · 모든 PC에서 접근 가능", &mut self.settings.output)
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
                            egui::Grid::new("render_options").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                                ui.label("시작 프레임");
                                ui.add(egui::DragValue::new(&mut self.settings.range_start).range(0..=1_000_000));
                                ui.end_row();
                                ui.label("끝 프레임 · 미포함, 0은 끝까지");
                                ui.add(egui::DragValue::new(&mut self.settings.range_end).range(0..=1_000_000));
                                ui.end_row();
                                ui.label("동시에 사용할 PC 수");
                                ui.add(egui::DragValue::new(&mut self.settings.max_workers).range(1..=64));
                                ui.end_row();
                                ui.label("PC당 작업 구간 · 프레임");
                                ui.add(egui::DragValue::new(&mut self.settings.chunk_frames).range(1..=3_600));
                                ui.end_row();
                            });
                            ui.checkbox(&mut self.settings.local_worker, "호스트 PC도 렌더링에 참여");
                            if path_row(ui, "최종 MP4 저장 위치", &mut self.settings.final_video)
                                && let Some(path) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).save_file()
                            {
                                self.settings.final_video = path.display().to_string();
                            }
                        });
                    }
                });
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
                if total > 0 {
                    section_title(ui, "진행 상황");
                    ui.add(egui::ProgressBar::new((done as f32 / total as f32).clamp(0.0, 1.0)).text(format!("{done} / {total} 프레임")));
                    ui.label(format!("연결된 PC: {connected} · 실패한 구간: {failed} · 동시 사용 한도: {}", self.settings.max_workers));
                    ui.label(&final_status);
                }
                if self.settings.host {
                    ui.separator();
                    section_title(ui, "연결된 PC");
                    if !workers.iter().any(|worker| worker.connected) {
                        ui.label("연결된 PC가 없습니다. 클라이언트에서 이 호스트의 고정 IP로 연결하세요.");
                    }
                    for worker in workers.iter().filter(|worker| worker.connected) {
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
                            if let Some((start, end)) = worker.assigned {
                                ui.label(format!("{start}–{end}"));
                            }
                        });
                    }
                }
                ui.separator();
                section_title(ui, "작업 기록");
                egui::ScrollArea::vertical().id_salt("farm_log").max_height(170.0).show(ui, |ui| {
                    for message in messages.iter().rev() {
                        ui.label(message);
                    }
                });
                ui.separator();
                ui.small("클라이언트가 호스트의 로컬 IP로 연결합니다. 두 PC에는 동일한 프로젝트·미디어·글꼴과 공유 폴더 접근 권한이 필요합니다.");
                ui.small("설정은 이 PC의 EffectCraft 사용자 설정 폴더에 저장됩니다.");
            });
        });
        if self.settings != before {
            self.save_settings();
        }
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("EffectCraft Render Farm").with_inner_size([900.0, 760.0]),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "EffectCraft Render Farm",
        options,
        Box::new(|cc| {
            let mut app = App::default();
            if let Err(error) = install_ui_font(&cc.egui_ctx) {
                app.last_error = error;
            }
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("EffectCraft Render Farm could not start: {e}");
    }
}
