use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{Request, Response, StatusCode};

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

const START_URL: &str = if cfg!(windows) { "http://farm-ui.localhost/" } else { "farm-ui://localhost/" };
const FARM_RELEASES: &str = "https://github.com/whitedev7773/effectcraft-render-farm/releases";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Default)]
struct Media {
    sequence: u64,
    images: BTreeMap<String, (u64, farm::Preview)>,
}

impl Media {
    fn describe(&mut self, key: String, preview: &farm::Preview) -> Value {
        let entry = self.images.entry(key.clone()).or_insert_with(|| {
            self.sequence += 1;
            (self.sequence, preview.clone())
        });
        if !Arc::ptr_eq(&entry.1.jpeg, &preview.jpeg) {
            self.sequence += 1;
            *entry = (self.sequence, preview.clone());
        }
        json!({"frame": preview.frame, "url": format!("/frame/{key}/{}", entry.0)})
    }

    fn asset(&self, path: &str) -> Option<Vec<u8>> {
        let (key, version) = path.strip_prefix("/frame/")?.split_once('/')?;
        let (current, preview) = self.images.get(key)?;
        (*current == version.parse::<u64>().ok()?).then(|| preview.jpeg.as_ref().clone())
    }
}

fn resource(request: Request<Vec<u8>>, media: &Mutex<Media>) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let asset = embedded(if path == "/" { "/index.html" } else { path })
        .map(|(mime, bytes)| (mime, Cow::Borrowed(bytes)))
        .or_else(|| media.lock().unwrap_or_else(std::sync::PoisonError::into_inner).asset(path).map(|bytes| ("image/jpeg", Cow::Owned(bytes))));
    let mut response = if let Some((mime, bytes)) = asset {
        let mut response = Response::new(bytes);
        response.headers_mut().insert("content-type", wry::http::HeaderValue::from_static(mime));
        response
    } else {
        let mut response = Response::default();
        *response.status_mut() = StatusCode::NOT_FOUND;
        response
    };
    let headers = response.headers_mut();
    headers.insert("content-security-policy", wry::http::HeaderValue::from_static(CSP));
    headers.insert("x-content-type-options", wry::http::HeaderValue::from_static("nosniff"));
    if path == "/" || path.starts_with("/frame/") {
        headers.insert("cache-control", wry::http::HeaderValue::from_static("no-store"));
    }
    response
}

#[derive(Deserialize)]
struct RequestMessage {
    id: u32,
    #[serde(flatten)]
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Command {
    Snapshot { logs: bool },
    DismissError,
    OpenProject,
    CloseImport,
    SelectComposition { index: usize },
    ConfigureImport { options: Box<TaskOptions>, commit: bool, preview: bool },
    PickOutput { suggested: String },
    PickFfmpeg,
    SaveTask { id: u64, options: Box<TaskOptions>, retry: bool },
    RemoveTask { id: u64 },
    RemoveTasks { ids: Vec<u64> },
    MoveTask { id: u64, direction: i8 },
    QueueView { cards: bool },
    SetConnection { host: bool, address: String, token: String, local_worker: bool },
    SetAppearance { theme: settings::Theme, reduced_motion: bool, check_updates: bool },
    SetRender { render: settings::RenderOptions, gpu: bool, distribution: Distribution, chunk_frames: i64, chunk_percent: f64, max_workers: usize },
    Discover,
    Connect,
    Disconnect,
    CloseWhenIdle,
    Window { operation: WindowOperation },
    Render,
    WorkerParticipation { id: u64, enabled: bool },
    Inspect { frame: i64, render: bool },
    CheckEngine,
    CheckApp,
    UpdatePrompt { engine: bool, open: bool },
    CancelEngineUpdate,
    InstallUpdate { engine: bool },
    OpenLocation { location: Location, task: Option<u64> },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Location {
    EngineSource,
    EngineRelease,
    FarmRelease,
    LogDirectory,
    OutputDirectory,
    ProjectDirectory,
    FrameDirectory,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WindowOperation {
    Welcome,
    Workspace,
    Drag,
    Minimize,
    Maximize,
    Close,
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Clone, Deserialize, Serialize)]
struct TaskOptions {
    final_video: String,
    range_start: i64,
    range_end: i64,
    distribution: Distribution,
    chunk_frames: i64,
    chunk_percent: f64,
    max_workers: usize,
    render: settings::RenderOptions,
    gpu: bool,
}

impl TaskOptions {
    fn apply(self, task: &mut queue::Task) {
        task.final_video = self.final_video;
        task.range_start = self.range_start;
        task.range_end = self.range_end;
        task.distribution = self.distribution;
        task.chunk_frames = self.chunk_frames;
        task.chunk_percent = self.chunk_percent;
        task.max_workers = self.max_workers;
        task.render = self.render;
        task.gpu = self.gpu;
    }
}

impl App {
    fn refresh_installation(&mut self) {
        match installation::detect(&self.settings.cli) {
            Ok(installation) => {
                self.installation_error.clear();
                if self.settings.cli != installation.cli.to_string_lossy() {
                    self.settings.cli = installation.cli.display().to_string();
                    self.save_settings();
                }
                self.engine_updater.check(installation.cli);
                self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).note(format!("설치된 EffectCraft 감지: {}", installation.root.display()));
            }
            Err(reason) => {
                self.engine_updater.local = None;
                self.installation_error = reason;
            }
        }
    }

    fn connection_active(&self) -> bool {
        self.running || self.pending_start.is_some() || self.stopping
    }

    fn start_blocker(&self) -> Option<&'static str> {
        if self.engine_install_requested || self.updater.installing() || self.engine_updater.installing() {
            Some("업데이트가 끝나면 연결할 수 있습니다.")
        } else if self.engine_updater.checking() {
            Some("EffectCraft 버전을 확인하고 있습니다.")
        } else if self.engine_updater.local.is_none() {
            Some("설정에서 EffectCraft 설치 상태를 확인하세요.")
        } else if !self.engine_update_idle() || self.project_import.as_ref().is_some_and(|draft| draft.preview_pending.is_some()) {
            Some("진행 중인 작업을 마치고 있습니다.")
        } else if self.settings.host && !self.settings.queue.iter().any(|task| task.status == queue::Status::Pending) {
            Some("프로젝트를 불러와 컴포지션을 큐에 추가하세요.")
        } else if !self.settings.host && self.settings.client_address.trim().is_empty() {
            Some("주변 호스트를 선택하거나 호스트 주소를 입력하세요.")
        } else if self.settings.token.trim().len() < 8 {
            Some("연결 설정에서 접속 암호를 8자 이상 입력하세요.")
        } else {
            None
        }
    }

    fn change_settings(&mut self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings == before {
            return Ok(());
        }
        if let Err(error) = self.persist_settings() {
            self.settings = before;
            return Err(error);
        }
        Ok(())
    }

    fn import_options(&self) -> Option<TaskOptions> {
        let draft = self.project_import.as_ref()?;
        Some(TaskOptions {
            final_video: draft.output.clone(),
            range_start: draft.range_start,
            range_end: draft.range_end,
            distribution: draft.distribution,
            chunk_frames: draft.chunk_frames,
            chunk_percent: draft.chunk_percent,
            max_workers: draft.max_workers,
            render: draft.render.clone(),
            gpu: draft.gpu,
        })
    }

    fn handle(&mut self, command: Command, media: &Mutex<Media>, parent: &tao::window::Window) -> Result<Value, String> {
        match command {
            Command::Snapshot { logs } => return Ok(self.desktop_snapshot(logs, media)),
            Command::DismissError => self.last_error.clear(),
            Command::OpenProject => {
                if self.connection_active() || !self.settings.host || self.project_import.is_some() {
                    return Err("호스트 모드에서 연결이 종료된 상태로 프로젝트를 추가하세요.".into());
                }
                if let Some(path) = rfd::FileDialog::new().set_parent(parent).add_filter("EffectCraft 프로젝트", &["ecproj"]).pick_file() {
                    self.begin_project_import(path);
                }
            }
            Command::CloseImport => {
                if self.project_import.as_ref().is_some_and(|draft| draft.preview_pending.is_some()) {
                    return Err("시험 렌더가 끝나면 닫을 수 있습니다.".into());
                }
                self.project_import = None;
            }
            Command::SelectComposition { index } => {
                let draft = self.project_import.as_mut().ok_or("열려 있는 프로젝트가 없습니다.")?;
                if draft.preview_pending.is_some() || draft.compositions.get(index).is_none() {
                    return Err("선택할 수 없는 컴포지션입니다.".into());
                }
                draft.select(index, &self.settings.queue);
            }
            Command::ConfigureImport { options, commit, preview } => {
                let draft = self.project_import.as_mut().ok_or("열려 있는 프로젝트가 없습니다.")?;
                if draft.preview_pending.is_some() {
                    return Err("시험 렌더가 끝나면 설정을 변경할 수 있습니다.".into());
                }
                draft.output = options.final_video;
                draft.range_start = options.range_start;
                draft.range_end = options.range_end;
                draft.distribution = options.distribution;
                draft.chunk_frames = options.chunk_frames;
                draft.chunk_percent = options.chunk_percent;
                draft.max_workers = options.max_workers;
                draft.gpu = options.gpu;
                draft.render = options.render;
                draft.error.clear();
                if commit && !self.commit_project_import() {
                    return Err(self.project_import.as_ref().map_or("작업을 추가할 수 없습니다.", |draft| &draft.error).into());
                }
                if preview {
                    self.preview_project_import();
                }
            }
            Command::PickOutput { suggested } => {
                let mut dialog = rfd::FileDialog::new().set_parent(parent).add_filter("MP4", &["mp4"]);
                let path = Path::new(&suggested);
                if let Some(dir) = path.parent().filter(|dir| dir.is_dir()) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    dialog = dialog.set_file_name(name);
                }
                return Ok(json!(dialog.save_file().map(|path| path.display().to_string())));
            }
            Command::PickFfmpeg => {
                return Ok(json!(rfd::FileDialog::new().set_parent(parent).pick_file().map(|path| path.display().to_string())));
            }
            Command::SaveTask { id, options, retry } => {
                let index = self.task_index(id)?;
                let mut edit = self.settings.queue[index].clone();
                if edit.status.in_progress() || edit.status == queue::Status::Completed || self.queue_active == Some(id) {
                    return Err("진행 중이거나 완료된 작업의 설정은 변경할 수 없습니다.".into());
                }
                options.apply(&mut edit);
                queue::validate_unique(&edit, &self.settings.queue)?;
                if retry && matches!(edit.status, queue::Status::Failed | queue::Status::Interrupted) {
                    edit.status = queue::Status::Pending;
                    edit.done = 0;
                    edit.detail.clear();
                }
                self.change_settings(|settings| settings.queue[index] = edit)?;
            }
            Command::RemoveTask { id } => self.remove_tasks(&[id])?,
            Command::RemoveTasks { ids } => self.remove_tasks(&ids)?,
            Command::MoveTask { id, direction } => {
                let index = self.task_index(id)?;
                let tasks = &self.settings.queue;
                if tasks[index].status != queue::Status::Pending {
                    return Err("대기 중인 작업만 순서를 변경할 수 있습니다.".into());
                }
                let is_pending = |task: &queue::Task| task.status == queue::Status::Pending;
                let target = match direction {
                    -1 => tasks[..index].iter().rposition(is_pending),
                    1 => tasks[index + 1..].iter().position(is_pending).map(|offset| index + 1 + offset),
                    _ => None,
                };
                if let Some(target) = target {
                    self.change_settings(|settings| settings.queue.swap(index, target))?;
                }
            }
            Command::QueueView { cards } => self.change_settings(|settings| settings.queue_cards = cards)?,
            Command::SetConnection { host, address, token, local_worker } => {
                if self.connection_active() || self.project_import.is_some() {
                    return Err("진행 중인 연결이나 프로젝트 추가를 마친 뒤 설정을 변경하세요.".into());
                }
                let address = normalize_address(if host && address.trim().is_empty() { "0.0.0.0:9778" } else { &address })?;
                if token.len() > 256 {
                    return Err("접속 암호는 UTF-8 기준 256바이트 이하여야 합니다.".into());
                }
                self.change_settings(|settings| {
                    settings.host = host;
                    if host {
                        settings.host_address = address;
                    } else {
                        settings.client_address = address;
                    }
                    settings.token = token.trim().into();
                    settings.local_worker = local_worker;
                })?;
            }
            Command::SetAppearance { theme, reduced_motion, check_updates } => self.change_settings(|settings| {
                settings.theme = theme;
                settings.reduced_motion = reduced_motion;
                settings.check_updates = check_updates;
            })?,
            Command::SetRender { render, gpu, distribution, chunk_frames, chunk_percent, max_workers } => {
                if self.connection_active() {
                    return Err("연결을 종료한 뒤 렌더 기본값을 변경하세요.".into());
                }
                render.validate()?;
                if !(1..=100_000).contains(&chunk_frames) || !(0.01..=100.0).contains(&chunk_percent) || !(1..=64).contains(&max_workers) {
                    return Err("작업 분배 옵션 범위를 확인하세요.".into());
                }
                self.change_settings(|settings| {
                    settings.render = render;
                    settings.gpu = gpu;
                    settings.distribution = distribution;
                    settings.chunk_frames = chunk_frames;
                    settings.chunk_percent = chunk_percent;
                    settings.max_workers = max_workers;
                })?;
            }
            Command::Discover => {
                if self.pending_discovery.is_none() && !self.connection_active() {
                    self.start_discovery();
                }
            }
            Command::Connect => {
                if self.connection_active() {
                    return Err("이미 연결을 준비 중이거나 연결되어 있습니다.".into());
                }
                if let Some(reason) = self.start_blocker() {
                    return Err(reason.into());
                }
                if self.settings.host {
                    self.queue_automatic = false;
                    self.queue_workers.clear();
                    self.start_next_queued();
                } else {
                    self.start();
                }
            }
            Command::Disconnect => self.disconnect(),
            Command::CloseWhenIdle => self.disconnect(),
            Command::Window { .. } => {}
            Command::Render => self.begin_current_render(),
            Command::WorkerParticipation { id, enabled } => {
                if !self.settings.host || self.stopping {
                    return Err("호스트에서 참여 PC를 선택할 수 있습니다.".into());
                }
                if let Some((ip, label)) = farm::set_worker_enabled(&self.state, id, enabled) {
                    self.settings.disabled_workers.retain(|saved| saved != &ip && saved != &label);
                    if !enabled {
                        self.settings.disabled_workers.push(ip);
                    }
                    self.save_settings();
                }
            }
            Command::Inspect { frame, render } => self.inspect(frame, render)?,
            Command::CheckEngine => {
                if self.engine_update_idle() {
                    self.refresh_installation();
                }
            }
            Command::CheckApp => self.updater.check(),
            Command::UpdatePrompt { engine, open } => {
                if engine {
                    self.engine_updater.prompt = open && self.engine_updater.required.is_some();
                } else {
                    self.updater.prompt = open && self.updater.available.is_some();
                }
            }
            Command::CancelEngineUpdate => self.engine_install_requested = false,
            Command::InstallUpdate { engine } => {
                if !cfg!(windows) {
                    return Err("공식 릴리스 페이지에서 설치 파일을 받아 주세요.".into());
                }
                if engine {
                    self.request_engine_install();
                } else if self.engine_update_idle() && !self.engine_updater.installing() && !self.engine_install_requested {
                    self.updater.consent_and_download();
                } else {
                    return Err("진행 중인 작업을 종료한 뒤 업데이트하세요.".into());
                }
            }
            Command::OpenLocation { location, task } => self.open_location(location, task)?,
        }
        Ok(Value::Null)
    }

    fn disconnect(&mut self) {
        if self.connection_active() {
            self.queue_automatic = false;
            self.stop.store(true, Ordering::Relaxed);
            self.stopping = true;
        }
    }

    fn safe_to_close(&self) -> bool {
        self.engine_update_idle()
            && !self.connection_active()
            && !self.engine_updater.installing()
            && !self.updater.installing()
            && !self.engine_install_requested
            && self.project_import.as_ref().is_none_or(|draft| draft.preview_pending.is_none())
    }

    fn task_index(&self, id: u64) -> Result<usize, String> {
        self.settings.queue.iter().position(|task| task.id == id).ok_or_else(|| "작업을 찾을 수 없습니다.".into())
    }

    fn remove_tasks(&mut self, ids: &[u64]) -> Result<(), String> {
        for id in ids {
            let task = &self.settings.queue[self.task_index(*id)?];
            if task.status.in_progress() || self.queue_active == Some(*id) {
                return Err("진행 중인 작업은 제거할 수 없습니다.".into());
            }
        }
        self.change_settings(|settings| settings.queue.retain(|task| !ids.contains(&task.id)))?;
        self.queue_previews.retain(|id, _| !ids.contains(id));
        Ok(())
    }

    fn inspect(&mut self, frame: i64, render: bool) -> Result<(), String> {
        if self.pending_preview.is_some() {
            return Err("프레임을 준비하는 중입니다.".into());
        }
        let (start, end) = self.preview_range();
        if !(start..end).contains(&frame) {
            return Err("작업 범위 안의 프레임을 선택하세요.".into());
        }
        let (tx, rx) = mpsc::channel();
        if render {
            if !self.engine_update_idle() || self.engine_install_requested || self.engine_updater.installing() || self.settings.comp.is_empty() {
                return Err("연결을 종료하고 컴포지션을 준비한 뒤 시험 렌더를 시작하세요.".into());
            }
            let config = self.config();
            thread::spawn(move || {
                let _ = tx.send(farm::render_preview(&config, frame));
            });
        } else {
            let output = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).output.clone();
            if !self.settings.host || output.as_os_str().is_empty() {
                return Err("아직 저장된 프레임이 없습니다.".into());
            }
            thread::spawn(move || {
                let _ = tx.send(farm::preview_frame(&output, frame));
            });
        }
        self.pending_preview = Some(rx);
        self.preview_status = "프레임을 준비하고 있습니다…".into();
        Ok(())
    }

    fn preview_range(&self) -> (i64, i64) {
        self.job
            .as_ref()
            .map(|job| (job.start, job.end))
            .unwrap_or((self.settings.range_start, self.settings.range_end.max(self.settings.range_start.saturating_add(1))))
    }

    fn open_location(&self, location: Location, task: Option<u64>) -> Result<(), String> {
        let dir = match location {
            Location::EngineSource => return webbrowser::open("https://github.com/storytold/effectcraft").map_err(|error| error.to_string()),
            Location::EngineRelease => return webbrowser::open(installation::INSTALL_URL).map_err(|error| error.to_string()),
            Location::FarmRelease => return webbrowser::open(FARM_RELEASES).map_err(|error| error.to_string()),
            Location::LogDirectory => settings::log_path()?.parent().map(Path::to_path_buf),
            Location::OutputDirectory => {
                let output = if let Some(id) = task { &self.settings.queue[self.task_index(id)?].final_video } else { &self.settings.final_video };
                Path::new(output).parent().map(Path::to_path_buf)
            }
            Location::ProjectDirectory => {
                let index = self.task_index(task.ok_or("작업을 선택하세요.")?)?;
                Path::new(&self.settings.queue[index].project).parent().map(Path::to_path_buf)
            }
            Location::FrameDirectory => Some(self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).output.clone()),
        };
        let dir = dir.filter(|path| path.is_dir()).ok_or("아직 저장 폴더를 열 수 없습니다.")?;
        let program = if cfg!(windows) {
            "explorer.exe"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(program).arg(dir).spawn().map_err(|error| format!("폴더 열기: {error}"))?;
        Ok(())
    }
}

fn normalize_address(address: &str) -> Result<String, String> {
    let address = address.trim();
    if address.is_empty() || address.parse::<std::net::SocketAddr>().is_ok_and(|address| address.port() > 0) {
        return Ok(address.into());
    }
    if let Ok(ip) = address.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, 9778).to_string());
    }
    let (host, port) = address.rsplit_once(':').unwrap_or((address, "9778"));
    let valid_host = !host.is_empty() && host.len() <= 253 && host.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'));
    if !valid_host || !port.parse::<u16>().is_ok_and(|number| number > 0) {
        return Err("IP 주소 또는 호스트 이름과 포트를 확인하세요. 예: 192.168.0.10:9778".into());
    }
    Ok(format!("{host}:{port}"))
}

impl App {
    fn desktop_snapshot(&self, logs: bool, media: &Mutex<Media>) -> Value {
        let mut media = media.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (done, total, _, failures) = farm::progress_metrics(&state);
        let preview = |media: &mut Media, key: String, value: Option<&farm::Preview>| value.map(|value| media.describe(key, value));
        let workers: Vec<_> = state
            .worker_info
            .values()
            .map(|worker| {
                json!({
                    "id": worker.id, "ip": worker.ip, "label": worker.label, "enabled": worker.enabled,
                    "connected": worker.connected, "ready": worker.ready, "range": worker.assigned,
                    "done": worker.current_done, "completed": worker.completed_frames, "fps": worker.fps,
                    "bytes": worker.transferred_bytes, "seconds": worker.transfer_seconds,
                    "elapsed": worker.started.map(|started| started.elapsed().as_secs_f64()),
                    "profile": worker.profile, "preview": preview(&mut media, format!("worker-{}", worker.id), worker.preview.as_ref()),
                })
            })
            .collect();
        let tasks: Vec<_> = self
            .settings
            .queue
            .iter()
            .map(|task| {
                let mut value = json!(task);
                value["status_label"] = json!(task.status.label());
                value["preview"] = json!(preview(&mut media, format!("task-{}", task.id), self.queue_previews.get(&task.id)));
                value
            })
            .collect();
        let (range_start, range_end) = self.preview_range();
        let (done, total) = if !self.settings.host { state.local_progress.map_or((0, 0), |(start, end, done)| (done, end - start)) } else { (done, total) };
        let host = self.settings.host;
        let elapsed =
            if host { state.finished_elapsed.or_else(|| state.started.map(|start| start.elapsed())) } else { state.local_started.map(|start| start.elapsed()) }
                .map(|time| time.as_secs_f64());
        let rate = if self.settings.host {
            state
                .worker_info
                .values()
                .filter(|worker| worker.connected && worker.assigned.is_some())
                .map(|worker| worker.started.map_or(worker.fps, |start| worker.current_done as f64 / start.elapsed().as_secs_f64().max(0.001)))
                .sum::<f64>()
        } else {
            elapsed.filter(|seconds| *seconds > 0.0).map_or(0.0, |seconds| done as f64 / seconds)
        };
        let recent_count = if logs { 2000 } else { 12 };
        let messages = &state.messages[state.messages.len().saturating_sub(recent_count)..];
        let settings = &self.settings;
        let import = self.project_import.as_ref().map(|draft| {
            json!({
                "project": draft.project, "compositions": draft.compositions, "selected": draft.selected, "options": self.import_options(),
                "loading": draft.pending.is_some(), "preview_busy": draft.preview_pending.is_some(), "error": draft.error,
                "preview": preview(&mut media, "import".into(), draft.preview.as_ref()),
            })
        });
        let snapshot = json!({
            "version": env!("CARGO_PKG_VERSION"), "windows": cfg!(windows),
            "settings": {
                "theme": settings.theme, "reduced_motion": settings.reduced_motion, "check_updates": settings.check_updates,
                "host": settings.host, "host_address": settings.host_address, "client_address": settings.client_address,
                "token": settings.token, "local_worker": settings.local_worker, "queue_cards": settings.queue_cards,
                "render": settings.render, "gpu": settings.gpu, "distribution": settings.distribution, "chunk_frames": settings.chunk_frames,
                "chunk_percent": settings.chunk_percent, "max_workers": settings.max_workers, "cli": settings.cli,
            },
            "tasks": tasks, "import": import, "workers": workers, "hosts": self.discovered_hosts,
            "connected": self.running && (host || state.worker_comp.is_some()), "preparing": self.pending_start.is_some(), "stopping": self.stopping,
            "locked": self.connection_active(), "dispatch": state.dispatch_enabled, "automatic": self.queue_automatic,
            "active_task": self.queue_active, "blocker": self.start_blocker(), "error": self.last_error,
            "discovering": self.pending_discovery.is_some(), "discovery_status": self.discovery_status,
            "render_state": {
                "done": done, "total": total, "failures": failures, "encoding": state.assembly_started && state.final_result.is_none(),
                "status": if !host && state.worker_comp.is_some() {
                    if state.local_progress.is_some() { "호스트의 렌더 작업에 참여 중" } else { "호스트 연결 완료 · 렌더 지시 대기" }
                } else { &state.final_status },
                "elapsed": elapsed, "fps": rate, "comp": if host { self.job.as_ref().map(|job| &job.comp) } else { state.worker_comp.as_ref() },
                "local_range": state.local_progress, "profile": state.local_profile,
                "output": settings.final_video, "frames": state.output, "messages": messages,
                "timeline": compact_timeline(farm::timeline(&state), range_start, range_end),
                "preview": preview(&mut media, "live".into(), if host { state.latest_preview.as_ref().or(state.local_preview.as_ref()) } else { state.local_preview.as_ref() }),
            },
            "inspect": {
                "start": range_start, "end": range_end, "busy": self.pending_preview.is_some(), "status": self.preview_status,
                "preview": preview(&mut media, "inspect".into(), self.inspected.as_ref()), "can_load": settings.host && !state.output.as_os_str().is_empty(),
                "can_render": settings.host && self.engine_update_idle() && self.engine_updater.local.is_some() && !self.engine_install_requested
                    && !self.engine_updater.installing() && !settings.comp.is_empty(),
            },
            "engine": {
                "local": self.engine_updater.local, "latest": self.engine_updater.latest.as_ref().map(|release| &release.tag_name),
                "target": self.engine_updater.required.as_ref().map(|required| &required.target),
                "peer": self.engine_updater.required.as_ref().and_then(|required| required.peer.as_ref()),
                "prompt": self.engine_updater.prompt, "checking": self.engine_updater.checking(), "installing": self.engine_updater.installing(),
                "queued": self.engine_install_requested, "status": self.engine_updater.status, "missing": self.installation_error,
                "bytes": self.engine_updater.progress.load(Ordering::Relaxed)
            },
            "update": {
                "target": self.updater.available.as_ref().map(|release| &release.tag_name), "body": self.updater.available.as_ref().and_then(|release| release.body.as_ref()),
                "prompt": self.updater.prompt, "status": self.updater.status, "busy": self.updater.busy(), "installing": self.updater.installing(),
                "bytes": self.updater.progress.load(Ordering::Relaxed),
            }
        });
        media.images.retain(|key, _| {
            if let Some(id) = key.strip_prefix("task-").and_then(|id| id.parse::<u64>().ok()) {
                self.queue_previews.contains_key(&id)
            } else if let Some(id) = key.strip_prefix("worker-").and_then(|id| id.parse::<u64>().ok()) {
                state.worker_info.contains_key(&id)
            } else {
                true
            }
        });
        snapshot
    }
}

fn compact_timeline(chunks: impl IntoIterator<Item = (i64, i64, u8)>, start: i64, end: i64) -> Vec<u8> {
    let span = end.saturating_sub(start);
    let columns = span.clamp(0, 96);
    let mut timeline = vec![0; columns as usize];
    if columns == 0 {
        return timeline;
    }
    for (first, last, status) in chunks {
        let from = ((first - start).clamp(0, span) * columns) / span;
        let to = ((last - start).clamp(0, span) * columns + span - 1) / span;
        for cell in &mut timeline[from as usize..to.max(from) as usize] {
            // Failed and running work stay visible when several chunks share a column.
            let rank = [0, 2, 1, 3];
            if rank[status as usize] > rank[*cell as usize] {
                *cell = status;
            }
        }
    }
    timeline
}

enum DesktopEvent {
    Request(RequestMessage),
    DropProject(PathBuf),
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::default();
    app.refresh_installation();
    let event_loop = EventLoopBuilder::<DesktopEvent>::with_user_event().build();
    let image = image::load_from_memory(include_bytes!("../assets/app-icon/farm-256.png"))?.to_rgba8();
    let (width, height) = image.dimensions();
    let icon = tao::window::Icon::from_rgba(image.into_raw(), width, height)?;
    let window = WindowBuilder::new()
        .with_title("EffectCraft Render Farm")
        .with_inner_size(LogicalSize::new(550.0, 558.0))
        .with_min_inner_size(LogicalSize::new(500.0, 500.0))
        .with_window_icon(Some(icon))
        .with_decorations(false)
        .with_transparent(true)
        .with_visible(false)
        .build(&event_loop)?;
    let media = Arc::new(Mutex::new(Media::default()));
    let protocol_media = media.clone();
    let ipc = event_loop.create_proxy();
    let drop_proxy = event_loop.create_proxy();
    let builder = wry::WebViewBuilder::new()
        .with_url(START_URL)
        .with_transparent(true)
        .with_custom_protocol("farm-ui".into(), move |_, request| resource(request, &protocol_media))
        .with_navigation_handler(|url| url == START_URL)
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_ipc_handler(move |request| {
            if request.uri() == START_URL
                && request.body().len() <= 65_536
                && let Ok(request) = serde_json::from_str(request.body())
            {
                let _ = ipc.send_event(DesktopEvent::Request(request));
            }
        })
        .with_drag_drop_handler(move |event| {
            if let wry::DragDropEvent::Drop { paths, .. } = event
                && let Some(path) = paths.into_iter().find(|path| is_project(path))
            {
                let _ = drop_proxy.send_event(DesktopEvent::DropProject(path));
            }
            true
        });
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().ok_or("GTK window container missing")?)?
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window)?;

    window.set_visible(true);
    let mut next_tick = Instant::now();
    let mut close_when_idle = false;
    event_loop.run(move |event, _, flow| {
        if Instant::now() >= next_tick {
            app.poll();
            next_tick = Instant::now() + Duration::from_millis(150);
        }
        *flow = ControlFlow::WaitUntil(next_tick);
        if app.updater.restarting {
            *flow = ControlFlow::Exit;
            return;
        }
        if close_when_idle && app.safe_to_close() {
            app.save_settings();
            *flow = ControlFlow::Exit;
            return;
        }
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if app.safe_to_close() {
                    app.save_settings();
                    *flow = ControlFlow::Exit;
                } else if !close_when_idle {
                    let _ = webview.evaluate_script("window.dispatchEvent(new Event('farm:close-request'))");
                }
            }
            Event::UserEvent(DesktopEvent::DropProject(path)) => {
                if app.settings.host && !app.connection_active() && app.project_import.is_none() {
                    app.begin_project_import(path);
                }
            }
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                let maximized = window.is_maximized();
                let _ = webview.evaluate_script(&format!("window.dispatchEvent(new CustomEvent('farm:window-mode', {{detail:{maximized}}}))"));
            }
            Event::UserEvent(DesktopEvent::Request(request)) => {
                close_when_idle |= matches!(request.command, Command::CloseWhenIdle);
                if let Command::Window { operation } = &request.command {
                    match operation {
                        WindowOperation::Close if app.safe_to_close() => {
                            app.save_settings();
                            *flow = ControlFlow::Exit;
                            return;
                        }
                        WindowOperation::Close if !close_when_idle => {
                            let _ = webview.evaluate_script("window.dispatchEvent(new Event('farm:close-request'))");
                        }
                        WindowOperation::Close => {}
                        mode => control_window(&window, *mode),
                    }
                }
                let payload = match app.handle(request.command, &media, &window) {
                    Ok(value) => json!({"id": request.id, "value": value}),
                    Err(error) => json!({"id": request.id, "error": error}),
                };
                let _ = webview.evaluate_script(&format!("window.dispatchEvent(new CustomEvent('farm:reply', {{detail:{payload}}}))"));
            }
            _ => {}
        }
    })
}

fn control_window(window: &tao::window::Window, operation: WindowOperation) {
    use tao::window::ResizeDirection;
    match operation {
        WindowOperation::Welcome => {
            window.set_maximized(false);
            window.set_inner_size(LogicalSize::new(550.0, 558.0));
        }
        WindowOperation::Workspace => {
            let default = LogicalSize::new(1280.0_f64, 720.0_f64);
            let size = window.current_monitor().map(|monitor| {
                let display = monitor.size().to_logical::<f64>(monitor.scale_factor());
                LogicalSize::new(default.width.min(display.width - 24.0), default.height.min(display.height - 64.0))
            });
            window.set_inner_size(size.unwrap_or(default));
        }
        WindowOperation::Drag => {
            let _ = window.drag_window();
        }
        WindowOperation::Minimize => window.set_minimized(true),
        WindowOperation::Maximize => window.set_maximized(!window.is_maximized()),
        WindowOperation::Close => {}
        edge if !window.is_maximized() => {
            let edge = match edge {
                WindowOperation::North => ResizeDirection::North,
                WindowOperation::South => ResizeDirection::South,
                WindowOperation::East => ResizeDirection::East,
                WindowOperation::West => ResizeDirection::West,
                WindowOperation::NorthEast => ResizeDirection::NorthEast,
                WindowOperation::NorthWest => ResizeDirection::NorthWest,
                WindowOperation::SouthEast => ResizeDirection::SouthEast,
                WindowOperation::SouthWest => ResizeDirection::SouthWest,
                _ => return,
            };
            let _ = window.drag_resize_window(edge);
        }
        _ => {}
    }
}

fn is_project(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ecproj"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn timeline_handles_a_selected_subrange_and_prioritizes_failures() {
        let chunks = [(0, 5, 2), (5, 6, 2), (5, 6, 3), (6, 7, 1), (8, 10, 2)];
        assert_eq!(compact_timeline(chunks, 5, 10), [3, 1, 0, 2, 2]);
        assert!(compact_timeline(chunks, 10, 10).is_empty());
        assert_eq!(compact_timeline([(0, 100_000, 2), (99_999, 100_000, 3)], 0, 100_000).last(), Some(&3));
    }

    #[test]
    fn accepts_lan_addresses_with_an_optional_default_port() {
        assert_eq!(normalize_address(" 127.0.0.1 ").unwrap(), "127.0.0.1:9778");
        assert_eq!(normalize_address("render-host").unwrap(), "render-host:9778");
        assert_eq!(normalize_address("::1").unwrap(), "[::1]:9778");
        assert!(normalize_address("127.0.0.1:0").is_err());
        assert!(normalize_address("https://render-host").is_err());
    }

    #[test]
    fn desktop_workflow_renders_with_the_installed_engine_when_requested() -> Result<(), String> {
        let Some(project) = std::env::var_os("EFFECTCRAFT_FARM_TEST_PROJECT").map(PathBuf::from) else { return Ok(()) };
        let installation = installation::detect("")?;
        let local = effectcraft_update::installed_version(&installation.cli)?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        drop(listener);
        let mut app = App::with_state(Settings::default(), None, Arc::new(Mutex::new(State::default())));
        app.settings_in_memory = true;
        app.settings.check_updates = false;
        app.settings.cli = installation.cli.display().to_string();
        app.settings.host_address = address.to_string();
        app.settings.token = "local-smoke-test".into();
        app.settings.render.automatic = false;
        app.settings.render.audio = false;
        app.settings.gpu = false;
        app.engine_updater.local = Some(local);
        app.begin_project_import(project.clone());
        let wait = |app: &mut App, condition: fn(&App) -> bool| -> Result<(), String> {
            let deadline = Instant::now() + Duration::from_secs(25);
            while !condition(app) && Instant::now() < deadline {
                app.poll();
                if !app.last_error.is_empty() {
                    return Err(app.last_error.clone());
                }
                thread::sleep(Duration::from_millis(30));
            }
            if condition(app) { Ok(()) } else { Err("Render Farm smoke test timed out".into()) }
        };
        wait(&mut app, |app| app.project_import.as_ref().is_some_and(|draft| draft.pending.is_none()))?;
        let draft = app.project_import.as_mut().ok_or("project import did not open")?;
        if draft.compositions.is_empty() {
            return Err(format!("No compositions: {}", draft.error));
        }
        draft.select(0, &app.settings.queue);
        let output = project.with_file_name(format!("farm-smoke-{}.mp4", std::process::id()));
        draft.output = output.display().to_string();
        app.preview_project_import();
        wait(&mut app, |app| app.project_import.as_ref().is_some_and(|draft| draft.preview_pending.is_none()))?;
        let draft = app.project_import.as_ref().ok_or("project import did not remain open")?;
        assert!(draft.preview.is_some(), "{}", draft.error);
        assert!(app.commit_project_import());
        let media = Mutex::new(Media::default());
        assert_eq!(app.desktop_snapshot(false, &media)["tasks"][0]["status"], "Pending");
        app.start_next_queued();
        let result = (|| {
            wait(&mut app, |app| app.running && !app.connected_render_workers().is_empty())?;
            app.begin_current_render();
            wait(&mut app, |app| app.settings.queue[0].status == queue::Status::Completed)?;
            app.inspect(app.preview_range().0, false)?;
            wait(&mut app, |app| app.pending_preview.is_none())?;
            let snapshot = app.desktop_snapshot(true, &media);
            assert_eq!(snapshot["tasks"][0]["status"], "Completed");
            let preview_url = snapshot["inspect"]["preview"]["url"].as_str().ok_or("completed frame preview is unavailable")?;
            assert!(media.lock().unwrap().asset(preview_url).is_some_and(|bytes| bytes.starts_with(&[0xff, 0xd8])));
            assert!(output.metadata().is_ok_and(|metadata| metadata.len() > 0));
            Ok(())
        })();
        app.disconnect();
        result
    }
}
