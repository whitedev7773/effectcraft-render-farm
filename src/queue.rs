use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Status {
    #[default]
    Pending,
    Preparing,
    Rendering,
    Encoding,
    Completed,
    Failed,
    Interrupted,
}

impl Status {
    pub fn in_progress(self) -> bool {
        matches!(self, Self::Preparing | Self::Rendering | Self::Encoding)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "대기",
            Self::Preparing => "준비 중",
            Self::Rendering => "렌더링 중",
            Self::Encoding => "MP4 출력 중",
            Self::Completed => "완료",
            Self::Failed => "실패",
            Self::Interrupted => "중단",
        }
    }
}

/// Only job-specific settings are captured. Connection credentials stay in Settings.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Task {
    pub id: u64,
    pub project: String,
    pub composition: CompInfo,
    pub output: String,
    pub final_video: String,
    pub range_start: i64,
    pub range_end: i64,
    pub distribution: Distribution,
    pub chunk_frames: i64,
    pub chunk_percent: f64,
    pub max_workers: usize,
    pub render: settings::RenderOptions,
    pub gpu: bool,
    pub status: Status,
    pub done: i64,
    pub detail: String,
}

impl Task {
    pub fn capture(id: u64, settings: &Settings, composition: CompInfo) -> Self {
        Self {
            id,
            project: settings.project.clone(),
            composition,
            output: settings.output.clone(),
            final_video: settings.final_video.clone(),
            range_start: settings.range_start,
            range_end: settings.range_end,
            distribution: settings.distribution,
            chunk_frames: settings.chunk_frames,
            chunk_percent: settings.chunk_percent,
            max_workers: settings.max_workers,
            render: settings.render.clone(),
            gpu: settings.gpu,
            status: Status::Pending,
            done: 0,
            detail: String::new(),
        }
    }

    pub fn apply(&self, settings: &mut Settings) {
        settings.project.clone_from(&self.project);
        settings.comp.clone_from(&self.composition.name);
        settings.range_comp.clone_from(&self.composition.name);
        settings.output.clone_from(&self.output);
        settings.final_video.clone_from(&self.final_video);
        settings.range_start = self.range_start;
        settings.range_end = self.range_end;
        settings.distribution = self.distribution;
        settings.chunk_frames = self.chunk_frames;
        settings.chunk_percent = self.chunk_percent;
        settings.max_workers = self.max_workers;
        settings.render = self.render.clone();
        settings.gpu = self.gpu;
    }

    pub fn validate(&self) -> Result<(), String> {
        self.render.validate()?;
        let mut settings = Settings::default();
        self.apply(&mut settings);
        settings.effective_chunk_frames(self.composition.frames)?;
        if !PathBuf::from(&self.project).is_file() {
            return Err("프로젝트 파일을 찾을 수 없습니다".into());
        }
        let video = PathBuf::from(&self.final_video);
        if !video.is_absolute() || !video.parent().is_some_and(|path| path.is_dir()) || video.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("mp4")) {
            return Err("최종 MP4의 저장 위치와 .mp4 확장자를 확인하세요".into());
        }
        if video.exists() {
            return Err("최종 MP4가 이미 있습니다. 새 파일 이름을 선택하세요".into());
        }
        let output = PathBuf::from(&self.output);
        if !output.is_absolute() || output.is_file() {
            return Err("프레임 저장 폴더의 절대 경로를 지정하세요".into());
        }
        Ok(())
    }
}

fn path_key(value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    let path = path.canonicalize().unwrap_or_else(|_| {
        path.parent().and_then(|parent| parent.canonicalize().ok()).and_then(|parent| path.file_name().map(|name| parent.join(name))).unwrap_or(path)
    });
    #[cfg(windows)]
    let path = PathBuf::from(path.to_string_lossy().to_lowercase());
    path
}

pub(crate) fn validate_unique(task: &Task, tasks: &[Task]) -> Result<(), String> {
    task.validate()?;
    for other in tasks.iter().filter(|other| other.id != task.id) {
        if path_key(&other.output) == path_key(&task.output) || path_key(&other.final_video) == path_key(&task.final_video) {
            return Err(format!("‘{}’ 작업과 출력 경로가 겹칩니다. 작업마다 별도 MP4와 프레임 폴더를 지정하세요.", other.composition.name));
        }
    }
    Ok(())
}

impl App {
    pub(crate) fn enqueue_current(&mut self) {
        if self.settings.queue.len() >= 200 {
            self.last_error = "큐에는 최대 200개 작업을 등록할 수 있습니다".into();
            return;
        }
        let Some(comp) = self.catalog.iter().find(|comp| comp.name == self.settings.comp).cloned() else {
            self.last_error = "프로젝트와 컴포지션을 먼저 선택하세요".into();
            return;
        };
        let id = self.settings.queue.iter().map(|task| task.id).max().unwrap_or(0) + 1;
        let task = Task::capture(id, &self.settings, comp);
        match validate_unique(&task, &self.settings.queue) {
            Ok(()) => {
                self.settings.queue.push(task);
                self.queue_detail = Some(id);
                self.last_error.clear();
            }
            Err(error) => self.last_error = error,
        }
    }

    pub(crate) fn start_next_queued(&mut self) {
        if self.running || self.pending_start.is_some() || self.worker_active.load(Ordering::Relaxed) || self.pending_preview.is_some() {
            return;
        }
        let Some(task) = self.settings.queue.iter().find(|task| task.status == Status::Pending).cloned() else {
            self.queue_automatic = false;
            return;
        };
        if let Err(error) = validate_unique(&task, &self.settings.queue) {
            if let Some(entry) = self.settings.queue.iter_mut().find(|entry| entry.id == task.id) {
                entry.status = Status::Failed;
                entry.detail = error.clone();
            }
            self.last_error = error;
            // Stop on errors so an unattended queue does not hide a failed job.
            self.queue_automatic = false;
            self.save_settings();
            return;
        }
        task.apply(&mut self.settings);
        self.catalog_path = task.project.clone();
        self.catalog_changed = None;
        self.pending_catalog = None;
        self.catalog = vec![task.composition];
        self.queue_active = Some(task.id);
        if let Some(entry) = self.settings.queue.iter_mut().find(|entry| entry.id == task.id) {
            entry.status = Status::Preparing;
            entry.detail.clear();
        }
        self.start();
        if self.pending_start.is_none() {
            self.fail_queued(self.last_error.clone());
        }
    }

    pub(crate) fn fail_queued(&mut self, error: String) {
        if let Some(id) = self.queue_active.take()
            && let Some(task) = self.settings.queue.iter_mut().find(|task| task.id == id)
        {
            task.status = Status::Failed;
            task.detail = error;
        }
        self.queue_automatic = false;
        self.save_settings();
    }

    pub(crate) fn poll_queue(&mut self) {
        if let Some(id) = self.queue_active {
            let (done, _, _, failed) = farm::progress_snapshot(&self.state);
            let (dispatch, encoding, result, preview, detail) = {
                let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let preview = state.latest_preview.clone().or_else(|| state.local_preview.clone());
                (state.dispatch_enabled, state.assembly_started, state.final_result.clone(), preview, state.final_status.clone())
            };
            if self.pending_start.is_none()
                && self.running
                && let Some(preview) = preview
            {
                self.queue_previews.insert(id, preview);
            }
            if self.pending_start.is_none()
                && let Some(task) = self.settings.queue.iter_mut().find(|task| task.id == id)
            {
                task.done = done;
                task.detail = detail;
                if let Some(result) = result {
                    task.status = if result.is_ok() { Status::Completed } else { Status::Failed };
                    if let Err(error) = result {
                        self.last_error = error;
                        self.queue_automatic = false;
                    }
                    self.queue_active = None;
                    self.stop.store(true, Ordering::Relaxed);
                    self.stopping = true;
                    self.save_settings();
                } else if failed > 0 && dispatch && !encoding {
                    task.status = Status::Failed;
                    task.detail = format!("실패한 렌더 구간 {failed}개 · 작업 기록을 확인하세요");
                    self.queue_automatic = false;
                    self.stop.store(true, Ordering::Relaxed);
                    self.stopping = true;
                    self.queue_active = None;
                    self.save_settings();
                } else if !self.stopping {
                    task.status = if encoding {
                        Status::Encoding
                    } else if dispatch {
                        Status::Rendering
                    } else {
                        Status::Preparing
                    };
                }
            }
            if !self.running && self.pending_start.is_none() {
                if let Some(task) = self.settings.queue.iter_mut().find(|task| task.id == id)
                    && task.status.in_progress()
                {
                    task.status = Status::Interrupted;
                    task.detail = "사용자가 큐를 중지했습니다. 재대기하면 저장된 프레임을 이어서 사용합니다.".into();
                }
                self.queue_active = None;
                self.save_settings();
            }
        }
        if self.queue_automatic && self.queue_active.is_none() && !self.running && self.pending_start.is_none() {
            self.start_next_queued();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: u64) -> Task {
        let settings = Settings { project: "missing.ecproj".into(), range_start: 5, range_end: 60, ..Settings::default() };
        Task::capture(
            id,
            &settings,
            CompInfo {
                name: format!("Comp {id}"),
                frames: 90,
                width: 1920,
                height: 1080,
                rate: effectcraft_time::FrameRate::FPS_60,
                work_start: 5,
                work_end: 60,
            },
        )
    }

    fn app() -> App {
        let settings = Settings { queue: vec![task(1), task(2)], queue_cards: true, ..Settings::default() };
        let mut app = App::with_state(settings, None, Arc::new(Mutex::new(State::default())));
        app.settings_in_memory = true;
        app
    }

    #[test]
    fn queue_roundtrip_keeps_independent_settings_and_connection_credentials() {
        let mut app = app();
        app.settings.queue[0].render.audio = false;
        app.settings.queue[0].render.draft = true;
        app.settings.queue[1].range_start = 20;
        let bytes = serde_json::to_vec(&app.settings).unwrap();
        let loaded: Settings = serde_json::from_slice(&bytes).unwrap();
        assert!(loaded.queue_cards);
        assert!(!loaded.queue[0].render.audio);
        assert!(loaded.queue[1].render.audio);
        assert_eq!(loaded.queue[1].range_start, 20);
        let mut target = Settings { token: "keep-password".into(), cli: "keep-cli".into(), host_address: "127.0.0.1:9778".into(), ..Settings::default() };
        loaded.queue[0].apply(&mut target);
        assert_eq!(target.token, "keep-password");
        assert_eq!(target.cli, "keep-cli");
        assert_eq!(target.host_address, "127.0.0.1:9778");
        assert_eq!(target.range_start, 5);
        assert!(target.render.draft);
        assert!(target.queue.is_empty());
    }

    #[test]
    fn restart_marks_only_in_progress_jobs_interrupted() {
        let mut settings = app().settings;
        settings.queue[0].status = Status::Encoding;
        settings.queue[1].status = Status::Completed;
        let restored = App::with_state(settings, None, Arc::new(Mutex::new(State::default())));
        assert_eq!(restored.settings.queue[0].status.label(), "중단");
        assert_eq!(restored.settings.queue[1].status.label(), "완료");
        assert!(!restored.queue_automatic);
    }

    #[test]
    fn queue_waits_for_mp4_result_and_old_session_shutdown() {
        let mut app = app();
        app.running = true;
        app.queue_active = Some(1);
        app.queue_automatic = true;
        {
            let mut state = app.state.lock().unwrap();
            state.dispatch_enabled = true;
            state.assembly_started = true;
            state.coordinator_active = true;
            state.local_preview = Some(farm::Preview { frame: 49, jpeg: Arc::new(vec![1, 2, 3]) });
        }
        app.poll_queue();
        assert_eq!(app.settings.queue[0].status.label(), "MP4 출력 중");
        assert_eq!(app.queue_active, Some(1));
        assert_eq!(app.queue_previews[&1].frame, 49);
        assert!(app.pending_start.is_none());
        app.state.lock().unwrap().final_result = Some(Ok(()));
        app.poll_queue();
        assert_eq!(app.settings.queue[0].status.label(), "완료");
        assert_eq!(app.settings.queue[1].status.label(), "대기");
        assert!(app.queue_active.is_none());
        assert!(app.stopping && app.stop.load(Ordering::Relaxed));
        assert!(app.queue_automatic);
        assert!(app.pending_start.is_none());
    }

    #[test]
    fn encoding_failure_stops_queue_and_manual_stop_preserves_pending_jobs() {
        let mut app = app();
        app.running = true;
        app.queue_active = Some(1);
        app.queue_automatic = true;
        app.state.lock().unwrap().final_result = Some(Err("encoder failed".into()));
        app.poll_queue();
        assert_eq!(app.settings.queue[0].status.label(), "실패");
        assert_eq!(app.settings.queue[1].status.label(), "대기");
        assert!(!app.queue_automatic);
        assert_eq!(app.last_error, "encoder failed");

        app.state.lock().unwrap().final_result = None;
        app.queue_active = Some(2);
        app.running = false;
        app.stopping = false;
        app.poll_queue();
        assert_eq!(app.settings.queue[1].status.label(), "중단");
        assert!(app.queue_active.is_none());
    }

    #[test]
    fn invalid_pending_job_blocks_automatic_advance() {
        let mut app = app();
        app.queue_automatic = true;
        app.poll_queue();
        assert_eq!(app.settings.queue[0].status.label(), "실패");
        assert_eq!(app.settings.queue[1].status.label(), "대기");
        assert!(!app.queue_automatic);
        assert!(app.pending_start.is_none());
    }

    #[test]
    fn conflicting_outputs_and_invalid_ranges_are_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "farm-queue-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut first = task(1);
        first.project = dir.join("project.ecproj").display().to_string();
        first.final_video = dir.join("one.mp4").display().to_string();
        first.output = dir.join("one.frames").display().to_string();
        std::fs::write(&first.project, "fixture").unwrap();
        assert!(first.validate().is_ok());
        let mut second = first.clone();
        second.id = 2;
        assert!(validate_unique(&second, &[first.clone()]).is_err());
        second.final_video = dir.join("two.mp4").display().to_string();
        assert!(validate_unique(&second, &[first.clone()]).is_err());
        second.output = dir.join("two.frames").display().to_string();
        assert!(validate_unique(&second, &[first]).is_ok());
        second.range_end = second.range_start;
        assert!(second.validate().is_err());
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn queue_client_reconnects_to_next_composition_and_stops_on_request() {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut app = app();
        app.settings.host = false;
        app.settings.cli = std::env::current_exe().unwrap().display().to_string();
        app.settings.client_address = listener.local_addr().unwrap().to_string();
        let stop = app.stop.clone();
        let server = thread::spawn(move || {
            for comp in ["First project", "Second project"] {
                let deadline = Instant::now() + Duration::from_secs(10);
                let (mut stream, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                        Err(error) => panic!("queue client did not connect: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"], "hello");
                writeln!(stream, "{}", serde_json::json!({"type": "ready", "comp": comp, "queue": true})).unwrap();
                line.clear();
                reader.read_line(&mut line).unwrap();
                assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap()["type"], "request");
                writeln!(stream, "{}", serde_json::json!({"type": "finished"})).unwrap();
            }
            stop.store(true, Ordering::Relaxed);
        });
        app.spawn_worker(app.config());
        server.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.worker_active.load(Ordering::Relaxed) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.worker_active.load(Ordering::Relaxed));
        let state = app.state.lock().unwrap();
        assert!(state.remote_queue);
        assert!(state.messages.iter().any(|message| message.contains("First project")));
        assert!(state.messages.iter().any(|message| message.contains("Second project")));
    }

    #[test]
    fn completed_queue_job_starts_next_real_project_with_its_saved_range() {
        use effectcraft_color::Label;
        use effectcraft_project::{Comp, ItemKind, Project};
        use effectcraft_time::{FrameRate, Tick};
        let dir = std::env::temp_dir().join(format!(
            "farm-queue-transition-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let mut app = app();
        app.settings.cli = std::env::current_exe().unwrap().display().to_string();
        app.settings.host_address = "127.0.0.1:0".into();
        app.settings.token = "queue-test-password".into();
        app.settings.local_worker = false;
        for task in &mut app.settings.queue {
            let mut project = Project::default();
            let comp = Comp::new(1920, 1080, FrameRate::FPS_60, Tick::from_seconds_f64(1.5));
            project.add_item(&task.composition.name, Label::None, None, ItemKind::Comp(Arc::new(comp)));
            task.project = dir.join(format!("{}.ecproj", task.id)).display().to_string();
            task.output = dir.join(format!("{}.frames", task.id)).display().to_string();
            task.final_video = dir.join(format!("{}.mp4", task.id)).display().to_string();
            std::fs::write(&task.project, project.to_json()).unwrap();
        }
        app.queue_active = Some(1);
        app.queue_automatic = true;
        app.running = true;
        app.state.lock().unwrap().final_result = Some(Ok(()));
        app.poll_queue();
        assert_eq!(app.settings.queue[0].status.label(), "완료");
        assert!(app.pending_start.is_none());
        app.running = false; // The previous listener and workers have now shut down.
        app.poll_queue();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.pending_start.is_some() && Instant::now() < deadline {
            app.poll_start();
            thread::sleep(Duration::from_millis(10));
        }
        // Always stop the listener, even if an assertion below fails.
        app.queue_automatic = false;
        app.stop.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.state.lock().unwrap().coordinator_active && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(app.last_error.is_empty(), "{}", app.last_error);
        assert_eq!(app.queue_active, Some(2));
        let job = app.job.as_ref().unwrap();
        assert_eq!(job.comp, "Comp 2");
        assert_eq!((job.start, job.end), (5, 60));
        assert_eq!(job.output, dir.join("2.frames"));
        assert!(app.state.lock().unwrap().dispatch_enabled);
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
