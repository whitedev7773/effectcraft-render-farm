use super::*;

pub(crate) struct ProjectImport {
    pub project: String,
    pub compositions: Vec<CompInfo>,
    pub selected: Option<usize>,
    pub output: String,
    pub range_start: i64,
    pub range_end: i64,
    pub render: settings::RenderOptions,
    pub distribution: Distribution,
    pub chunk_frames: i64,
    pub chunk_percent: f64,
    pub max_workers: usize,
    pub gpu: bool,
    pub error: String,
    pub(crate) pending: Option<Receiver<Result<Vec<CompInfo>, String>>>,
    pub(crate) preview: Option<farm::Preview>,
    pub(crate) preview_pending: Option<Receiver<Result<farm::Preview, String>>>,
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
            distribution: Distribution::default(),
            chunk_frames: 300,
            chunk_percent: 50.0,
            max_workers: 2,
            gpu: true,
            error: String::new(),
            pending: None,
            preview: None,
            preview_pending: None,
        }
    }

    pub fn select(&mut self, index: usize, tasks: &[queue::Task]) {
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
        let reserved: std::collections::BTreeSet<_> = tasks.iter().map(|task| queue::path_key(&task.final_video)).collect();
        while output.exists() || reserved.contains(&queue::path_key(&output)) {
            output = parent.join(format!("{base}-{suffix}.mp4"));
            suffix += 1;
        }
        self.output = output.display().to_string();
        self.error.clear();
    }

    pub(crate) fn poll(&mut self) {
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
    pub(crate) fn begin_project_import(&mut self, path: PathBuf) {
        if self.running || self.pending_start.is_some() {
            return;
        }
        let mut draft = ProjectImport::ready(path.display().to_string(), Vec::new(), self.settings.render.clone());
        let (sender, receiver) = mpsc::channel();
        draft.distribution = self.settings.distribution;
        draft.chunk_frames = self.settings.chunk_frames;
        draft.chunk_percent = self.settings.chunk_percent;
        draft.max_workers = self.settings.max_workers;
        draft.gpu = self.settings.gpu;
        draft.pending = Some(receiver);
        self.project_import = Some(draft);
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
            settings.distribution = draft.distribution;
            settings.chunk_frames = draft.chunk_frames;
            settings.chunk_percent = draft.chunk_percent;
            settings.max_workers = draft.max_workers;
            settings.gpu = draft.gpu;
            let id = self.settings.queue.iter().map(|task| task.id).max().unwrap_or(0).checked_add(1).ok_or("작업 ID를 만들 수 없습니다")?;
            let task = crate::queue::Task::capture(id, &settings, comp);
            crate::queue::validate_unique(&task, &self.settings.queue)?;
            Ok(task)
        })();
        match result {
            Ok(task) => {
                self.settings.queue.push(task);
                if let Err(error) = self.persist_settings() {
                    self.settings.queue.pop();
                    if let Some(draft) = &mut self.project_import {
                        draft.error = error;
                    }
                    return false;
                }
                self.project_import = None;
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

    pub(crate) fn preview_project_import(&mut self) {
        let mut config = self.config();
        let Some(draft) = &mut self.project_import else { return };
        let Some(comp) = draft.selected.and_then(|index| draft.compositions.get(index)) else { return };
        if draft.preview_pending.is_some() || self.running || self.pending_start.is_some() || self.engine_updater.installing() {
            return;
        }
        config.project = PathBuf::from(&draft.project);
        config.comp = comp.name.clone();
        config.range_start = draft.range_start;
        config.range_end = draft.range_end;
        config.gpu = draft.render.automatic || draft.gpu;
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
}
