#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod desktop;
mod discovery;
mod effectcraft_update;
mod encoding;
mod farm;
mod installation;
mod project_import;
mod queue;
mod resources;
mod settings;
mod updater;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use farm::{CompInfo, Config, Job, Shared, State};
use settings::{Distribution, Settings};

struct App {
    #[cfg(test)]
    settings_in_memory: bool,
    queue_active: Option<u64>,
    queue_automatic: bool,
    queue_workers: std::collections::BTreeSet<String>,
    queue_previews: std::collections::BTreeMap<u64, farm::Preview>,
    project_import: Option<project_import::ProjectImport>,
    updater: updater::Updater,
    updater_logged_status: String,
    engine_updater: effectcraft_update::Updater,
    engine_logged_status: String,
    engine_install_requested: bool,
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
    installation_error: String,
    last_progress_save: Instant,
    catalog_path: String,
    catalog_changed: Option<Instant>,
    pending_catalog: Option<Receiver<Result<Vec<CompInfo>, String>>>,
    catalog: Vec<CompInfo>,
    catalog_status: String,
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
        let mut settings = settings;
        for task in &mut settings.queue {
            if task.status.in_progress() {
                task.status = queue::Status::Interrupted;
                task.detail = "앱이 종료되어 작업이 중단됐습니다. 재대기하면 저장된 프레임부터 이어서 렌더합니다.".into();
            }
        }
        let catalog_path = settings.project.clone();
        let catalog_changed = (!catalog_path.is_empty()).then(Instant::now);
        Self {
            #[cfg(test)]
            settings_in_memory: false,
            queue_active: None,
            queue_automatic: false,
            queue_workers: Default::default(),
            queue_previews: Default::default(),
            project_import: None,
            updater: updater::Updater::default(),
            updater_logged_status: String::new(),
            engine_updater: effectcraft_update::Updater::default(),
            engine_logged_status: String::new(),
            engine_install_requested: false,
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
            installation_error: String::new(),
            last_progress_save: Instant::now(),
            catalog_path,
            catalog_changed,
            pending_catalog: None,
            catalog: Vec::new(),
            catalog_status: String::new(),
            pending_preview: None,
            inspected: None,
            preview_status: String::new(),
        }
    }

    fn poll_app_update(&mut self) {
        let idle = !self.running
            && self.pending_start.is_none()
            && !self.worker_active.load(Ordering::Relaxed)
            && self.pending_preview.is_none()
            && !self.engine_updater.installing();
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
                }
                Err(error) => self.updater.status = format!("업데이트 실패: {error}"),
            }
        }
        if self.updater.status != self.updater_logged_status {
            if !self.updater.status.is_empty() {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("업데이트: {}", self.updater.status));
            }
            self.updater_logged_status = self.updater.status.clone();
        }
    }

    fn engine_ready(&mut self) -> bool {
        self.poll_engine_update();
        if self.engine_install_requested || self.engine_updater.installing() || self.updater.installing() {
            self.last_error = "업데이트 설치가 끝난 뒤 연결하세요".into();
            return false;
        }
        if self.engine_updater.local.is_none() || self.engine_updater.checking() {
            self.last_error = format!("EffectCraft 버전 확인을 완료한 뒤 연결하세요. {}", self.engine_updater.status);
            return false;
        }
        // Permit the version handshake even when an update is known, so a newer peer
        // can raise the target before either computer downloads an installer.
        true
    }

    fn poll_engine_update(&mut self) {
        if let Some(cli) = self.engine_updater.poll() {
            self.settings.cli = cli.display().to_string();
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).engine_update = None;
            self.save_settings();
        }
        let required = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).engine_update.clone();
        if let Some(required) = required
            && let Some(local) = &self.engine_updater.local
            && let Ok(required) = effectcraft_update::requirement(local, required.peer.as_deref(), required.latest.as_deref())
        {
            self.engine_updater.observe(required);
        }
        if self.engine_install_requested && self.engine_update_idle() && !self.updater.busy() && !self.engine_updater.checking() {
            self.engine_install_requested = false;
            self.engine_updater.consent_and_install(PathBuf::from(&self.settings.cli));
        }
        if self.engine_updater.status != self.engine_logged_status {
            if !self.engine_updater.status.is_empty() {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("EffectCraft 업데이트: {}", self.engine_updater.status));
            }
            self.engine_logged_status = self.engine_updater.status.clone();
        }
    }

    fn engine_update_idle(&self) -> bool {
        !self.running && self.pending_start.is_none() && !self.worker_active.load(Ordering::Relaxed) && self.pending_preview.is_none()
    }

    fn request_engine_install(&mut self) {
        if self.engine_updater.required.is_none() || self.updater.busy() || self.engine_updater.checking() || self.engine_updater.installing() {
            return;
        }
        self.engine_install_requested = true;
        if !self.engine_update_idle() {
            self.queue_automatic = false;
            self.stop.store(true, Ordering::Relaxed);
            self.stopping = true;
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
            engine: farm::EngineVersion {
                installed: self.engine_updater.local.clone().unwrap_or_default(),
                latest: self.engine_updater.latest.as_ref().map(|release| release.tag_name.clone()),
            },
            project: PathBuf::from(&s.project),
            cli: PathBuf::from(&s.cli),
            output: PathBuf::new(),
            comp: s.comp.clone(),
            address: if s.host { s.host_address.clone() } else { s.client_address.clone() },
            token: s.token.clone(),
            gpu: s.render.automatic || s.gpu,
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
        if !self.engine_ready() {
            return;
        }
        self.last_error.clear();
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
        if let Err(e) = self.persist_settings() {
            self.last_error = e;
            return;
        }
        let mut config = self.config();
        config.chunk_frames = chunk_frames;
        self.stop = Arc::new(AtomicBool::new(false));
        self.stopping = false;
        if self.settings.host {
            {
                let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.queue_session = self.queue_active.is_some();
                state.final_status = "프로젝트와 소스를 준비하고 있습니다…".into();
            }
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
            {
                let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.worker_comp = None;
                state.remote_queue = false;
                state.engine_mismatch = false;
                state.local_started = None;
                state.local_progress = None;
                state.local_preview = None;
                state.final_status = "호스트에 연결하는 중…".into();
            }
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
                if self.stop.load(Ordering::Relaxed) {
                    self.stopping = true;
                } else if self.settings.local_worker
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
                self.pending_config = None;
                if self.stop.load(Ordering::Relaxed) {
                    if let Some(id) = self.queue_active.take()
                        && let Some(task) = self.settings.queue.iter_mut().find(|task| task.id == id)
                    {
                        task.status = queue::Status::Interrupted;
                        task.detail = "호스트 준비가 취소되었습니다. 재대기하면 기존 프레임을 다시 확인합니다.".into();
                    }
                    self.queue_automatic = false;
                    self.stopping = false;
                    self.last_error.clear();
                    self.save_settings();
                } else {
                    self.last_error = e.clone();
                    if self.queue_active.is_some() {
                        self.fail_queued(e);
                    }
                }
            }
        }
    }

    fn spawn_worker(&self, config: Config) {
        let state = self.state.clone();
        let stop = self.stop.clone();
        let active = self.worker_active.clone();
        let reconnect = !self.settings.host;
        active.store(true, Ordering::Relaxed);
        thread::spawn(move || {
            let mut completed_session = false;
            loop {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| farm::run_worker(config.clone(), state.clone(), stop.clone())))
                    .unwrap_or_else(|_| Err("클라이언트 작업 중 예상하지 못한 오류가 발생했습니다".into()));
                match result {
                    Ok(()) if reconnect && !stop.load(Ordering::Relaxed) && state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remote_queue => {
                        completed_session = true;
                        let mut state = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        state.worker_comp = None;
                        state.final_status = "다음 큐 작업 연결 대기".into();
                    }
                    Err(_)
                        if reconnect
                            && completed_session
                            && !stop.load(Ordering::Relaxed)
                            && !state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).engine_mismatch => {}
                    Err(e) => {
                        state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("클라이언트 오류: {e}"));
                        break;
                    }
                    Ok(()) => break,
                }
                for _ in 0..30 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            if reconnect {
                state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).worker_comp = None;
            }
            active.store(false, Ordering::Relaxed);
        });
    }

    fn persist_settings(&mut self) -> Result<(), String> {
        #[cfg(test)]
        if self.settings_in_memory {
            return Ok(());
        }
        settings::save(&mut self.settings)
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.persist_settings() {
            self.last_error = e;
        }
    }
    fn poll(&mut self) {
        self.poll_engine_update();
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
        if self.stopping && self.pending_start.is_none() {
            let connected = farm::progress_snapshot(&self.state).2;
            let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let host_stopped = !self.settings.host || (!state.coordinator_active && (!state.assembly_started || state.final_result.is_some()));
            drop(state);
            if connected == 0 && !self.worker_active.load(Ordering::Relaxed) && host_stopped {
                self.running = false;
                self.stopping = false;
            }
        }
        let active = self.queue_active;
        self.poll_queue();
        if active.is_some() && active == self.queue_active && self.last_progress_save.elapsed() >= Duration::from_secs(2) {
            self.last_progress_save = Instant::now();
            self.save_settings();
        }
        if let Some(draft) = &mut self.project_import {
            draft.poll();
        }
        self.poll_app_update();
        if self.last_error != self.last_logged_error {
            if !self.last_error.is_empty() && !self.last_error.starts_with("클라이언트 오류:") {
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("화면 오류: {}", self.last_error));
            }
            self.last_logged_error = self.last_error.clone();
        }
    }
}

fn main() {
    if let Err(error) = desktop::run() {
        State::with_log_path(settings::log_path().ok()).note(format!("Render Farm 시작 실패: {error}"));
        eprintln!("EffectCraft Render Farm: {error}");
        let dialog = rfd::MessageDialog::new().set_title("EffectCraft Render Farm").set_level(rfd::MessageLevel::Error);
        if cfg!(windows) {
            let result = dialog
                .set_description(format!("앱을 시작할 수 없습니다. WebView2 Runtime 설치 페이지를 열까요?\n\n{error}"))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show();
            if result == rfd::MessageDialogResult::Yes {
                let _ = webbrowser::open("https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section");
            }
        } else {
            dialog.set_description(format!("앱을 시작할 수 없습니다.\n\n{error}")).show();
        }
    }
}
