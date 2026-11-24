//! Isolated visual regression harness. Never compiled into the release installer.
use super::*;

fn fixture() -> App {
    let settings = Settings {
        project: "D:/Studio/Projects/2026/Autumn Campaign/Title Sequence.ecproj".into(),
        final_video: "D:/Studio/Renders/Title Sequence/final.mp4".into(),
        comp: "Title Sequence · Final".into(),
        range_start: 0,
        range_end: 240,
        range_comp: "Title Sequence · Final".into(),
        token: "fixture-password".into(),
        check_updates: false,
        cli: "C:/Program Files/EffectCraft/effectcraft-cli.exe".into(),
        ..Settings::default()
    };
    let mut image = image::RgbImage::new(480, 270);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let wave = ((x as f32 * 0.018 + y as f32 * 0.011).sin() * 0.5 + 0.5) * 180.0;
        *pixel = image::Rgb([(25.0 + wave * 0.4) as u8, (35.0 + wave * 0.65) as u8, (60.0 + wave) as u8]);
    }
    let mut jpeg = Vec::new();
    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode_image(&image);
    let preview = farm::Preview { frame: 47, jpeg: Arc::new(jpeg) };
    let mut state = State::default();
    state.total = 240;
    state.workers = 6;
    state.final_status = "프레임 렌더링 중".into();
    state.dispatch_enabled = true;
    state.started = Some(Instant::now() - Duration::from_secs(18));
    for (index, name) in ["Studio Desktop", "Ryzen Notebook", "Mac Studio Worker", "Editing PC", "Render Node 02", "Render Node 03"].into_iter().enumerate() {
        let id = index as u64 + 1;
        state.worker_info.insert(
            id,
            farm::WorkerInfo {
                id,
                ip: format!("192.168.0.{}", index + 10),
                label: format!("{name} @ 192.168.0.{}", index + 10),
                enabled: true,
                connected: true,
                assigned: Some((index as i64 * 40, index as i64 * 40 + 40)),
                current_done: (index as i64 * 7 + 10).min(39),
                completed_frames: 32 + index as i64 * 8,
                started: Some(Instant::now() - Duration::from_secs(3)),
                fps: 12.5,
                transferred_bytes: 120 * 1024 * 1024,
                transfer_seconds: 2.0,
                preview: Some(preview.clone()),
                ready: true,
                profile: "CPU 12개 · 작업 스레드 10 · RAM 여유 8.1/16.0 GiB · 렌더 GPU".into(),
            },
        );
    }
    state.messages =
        vec!["[INFO] [autotune] GPU 렌더링 기본 사용 · 오류 발생 시 CPU로 전환".into(), "[WARN] [encoder] 지원하지 않는 인코더를 제외했습니다".into()];
    let mut app = App::with_state(settings, None, Arc::new(Mutex::new(state)));
    app.engine_updater.local = Some("0.23.1".into());
    app.catalog = vec![CompInfo {
        name: "Title Sequence · Final".into(),
        frames: 240,
        rate: effectcraft_time::FrameRate::FPS_60,
        width: 1920,
        height: 1080,
        work_start: 0,
        work_end: 240,
    }];
    app.job = Some(Job {
        output_guard: Default::default(),
        draft: false,
        hash: String::new(),
        comp: app.settings.comp.clone(),
        rate: effectcraft_time::FrameRate::FPS_60,
        total: 240,
        start: 0,
        end: 240,
        width: 1920,
        height: 1080,
        output: PathBuf::new(),
    });
    app.running = true;
    app.monitor = true;
    let mut task = crate::queue::Task::capture(1, &app.settings, app.catalog[0].clone());
    task.status = crate::queue::Status::Rendering;
    task.done = 86;
    task.detail = "프레임 렌더링 중".into();
    app.settings.queue.push(task.clone());
    app.queue_active = Some(1);
    app.queue_previews.insert(1, preview);
    for id in 2..=4 {
        let mut waiting = task.clone();
        waiting.id = id;
        waiting.composition.name = format!("Scene {id} · Final");
        waiting.status = crate::queue::Status::Pending;
        waiting.done = 0;
        waiting.detail.clear();
        app.settings.queue.push(waiting);
    }
    app
}

fn import_compositions() -> Vec<CompInfo> {
    [("인트로", 360, 0, 300), ("메인 타이틀", 480, 24, 360), ("아웃트로", 240, 0, 240)]
        .into_iter()
        .map(|(name, frames, work_start, work_end)| CompInfo {
            name: name.into(),
            frames,
            rate: effectcraft_time::FrameRate::FPS_60,
            width: 1920,
            height: 1080,
            work_start,
            work_end,
        })
        .collect()
}

#[cfg(debug_assertions)]
pub(crate) fn run_requested() -> bool {
    let Some(directory) = std::env::var_os("EFFECTCRAFT_FARM_UI_CAPTURE").map(PathBuf::from) else { return false };
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("UI capture folder: {error}");
        return true;
    }
    let filter = std::env::var("EFFECTCRAFT_FARM_UI_CAPTURE_ONLY").unwrap_or_default();
    let names: Vec<_> = filter.split(',').map(str::trim).filter(|name| !name.is_empty()).collect();
    let cases: Vec<_> = CASES.iter().copied().filter(|(name, _, _)| names.is_empty() || names.contains(name)).collect();
    if cases.is_empty() {
        eprintln!("UI capture: no matching cases");
        return true;
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Render Farm · UI review").with_inner_size([1440.0, 940.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Render Farm · UI review",
        options,
        Box::new(move |cc| {
            let _ = install_ui_font(&cc.egui_ctx);
            Ok(Box::new(Capture { app: fixture(), directory, cases, stage: 0, frames: 0, requested: false, started: Instant::now() }))
        }),
    );
    if let Err(error) = result {
        eprintln!("UI capture: {error}");
    }
    true
}

#[cfg(debug_assertions)]
struct Capture {
    app: App,
    directory: PathBuf,
    cases: Vec<(&'static str, f32, f32)>,
    stage: usize,
    frames: u32,
    requested: bool,
    started: Instant,
}

#[cfg(debug_assertions)]
const CASES: &[(&str, f32, f32)] = &[
    ("welcome", 550.0, 558.0),
    ("queue-list", 1440.0, 940.0),
    ("host-scan", 1280.0, 720.0),
    ("desktop", 1440.0, 940.0),
    ("fleet-filtered", 1440.0, 940.0),
    ("wide", 1920.0, 1080.0),
    ("ultrawide", 2560.0, 1440.0),
    ("compact-monitor", 780.0, 850.0),
    ("compact-project", 560.0, 740.0),
    ("compact-render", 560.0, 740.0),
    ("settings-general", 1440.0, 940.0),
    ("settings-render", 560.0, 740.0),
    ("logs", 1440.0, 940.0),
    ("logs-compact", 560.0, 740.0),
    ("worker-compact", 560.0, 740.0),
    ("preview-compact", 780.0, 850.0),
    ("queue-cards", 1440.0, 940.0),
    ("queue-cards-compact", 560.0, 740.0),
    ("queue-list-compact", 560.0, 740.0),
    ("queue-filtered", 1440.0, 940.0),
    ("queue-no-results", 560.0, 740.0),
    ("import-list", 1440.0, 940.0),
    ("import-list-compact", 560.0, 740.0),
    ("import-selected", 1440.0, 940.0),
    ("import-selected-compact", 560.0, 740.0),
    ("queue-detail", 1440.0, 940.0),
    ("queue-detail-compact", 560.0, 740.0),
    ("queue-connect", 1440.0, 940.0),
    ("queue-connect-compact", 560.0, 740.0),
    ("engine-update", 1440.0, 940.0),
    ("engine-update-compact", 560.0, 740.0),
    ("engine-update-connected", 560.0, 520.0),
    ("empty-host", 1440.0, 940.0),
    ("empty-host-project", 1440.0, 940.0),
    ("empty-host-project-compact", 560.0, 740.0),
    ("empty-client-project", 1440.0, 940.0),
    ("empty-client-project-compact", 560.0, 740.0),
    ("prepared-host-compact", 560.0, 740.0),
    ("farm-update", 560.0, 520.0),
];

#[cfg(debug_assertions)]
impl eframe::App for Capture {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.started.elapsed() > Duration::from_secs(180) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let captured =
            ctx.input(|input| input.events.iter().find_map(|event| if let egui::Event::Screenshot { image, .. } = event { Some(image.clone()) } else { None }));
        if let Some(image) = captured {
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
            let theme = if self.stage < self.cases.len() { "dark" } else { "light" };
            let path = self.directory.join(format!("{}-{}.png", theme, self.cases[self.stage % self.cases.len()].0));
            if let Err(error) = image::save_buffer(&path, &bytes, image.width() as u32, image.height() as u32, image::ColorType::Rgba8) {
                eprintln!("UI capture save: {error}");
            }
            println!("{}: {}x{}", path.display(), image.width(), image.height());
            self.stage += 1;
            self.frames = 0;
            self.requested = false;
        }
        if self.stage >= self.cases.len() * 2 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames == 0 {
            let (name, width, height) = self.cases[self.stage % self.cases.len()];
            self.app = fixture();
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            self.app.monitor = name != "compact-project";
            self.app.queue_view = name.starts_with("queue-cards") || name.starts_with("queue-list") || matches!(name, "queue-filtered" | "queue-no-results");
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("fleet-search"), if name == "fleet-filtered" { "Render Node" } else { "" }.to_owned());
                data.insert_temp(egui::Id::new("fleet-filter"), 0_u8);
                data.insert_temp(
                    egui::Id::new("queue-search"),
                    match name {
                        "queue-filtered" => "Scene 3",
                        "queue-no-results" => "없는 프로젝트",
                        _ => "",
                    }
                    .to_owned(),
                );
                data.insert_temp(egui::Id::new("queue-status-filter"), u8::from(name == "queue-filtered"));
            });
            self.app.running = !name.starts_with("settings");
            self.app.settings_open = name.starts_with("settings");
            self.app.settings_tab = u8::from(name == "settings-render");
            self.app.logs_open = name.starts_with("logs");
            self.app.worker_details = (name == "worker-compact").then_some(2);
            self.app.preview_open = name == "preview-compact";
            self.app.settings.queue_cards = name.starts_with("queue-cards");
            self.app.queue_detail = name.starts_with("queue-detail").then_some(2);
            if name == "host-scan" {
                let (_tx, rx) = mpsc::channel();
                self.app.pending_start = Some(rx);
                self.app.running = false;
                self.app.queue_view = true;
                self.app.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).final_status = "기존 프레임 확인 중 · 512 / 15754".into();
            }
            if name.starts_with("import-") {
                self.app.running = false;
                self.app.monitor = true;
                self.app.queue_view = true;
                self.app.settings.queue_cards = true;
                let mut draft = import::ProjectImport::ready(self.app.settings.project.clone(), import_compositions(), self.app.settings.render.clone());
                if name.starts_with("import-selected") {
                    draft.select(1);
                }
                self.app.project_import = Some(draft);
            }
            if name.starts_with("engine-update") {
                self.app.running = name == "engine-update-connected";
                self.app.engine_updater.required = crate::effectcraft_update::requirement("0.22.0", Some("0.23.0"), Some("0.23.1")).ok().flatten();
                self.app.engine_updater.prompt = true;
                self.app.engine_updater.status = "공식 EffectCraft 최신 버전 v0.23.1".into();
            }
            if name.starts_with("empty-") {
                self.app = App::with_state(Settings { check_updates: false, ..Settings::default() }, None, Arc::new(Mutex::new(State::default())));
                self.app.engine_updater.local = Some("0.23.1".into());
                self.app.settings.host = !name.starts_with("empty-client");
                self.app.monitor = name == "empty-host";
            }
            if name == "farm-update" {
                self.app = fixture();
                self.app.running = false;
                self.app.updater.prompt = true;
                self.app.updater.available = serde_json::from_value(serde_json::json!({
                    "tag_name": "v0.12.0", "html_url": "https://github.com/whitedev7773/effectcraft-render-farm/releases/tag/v0.12.0",
                    "draft": false, "prerelease": false, "assets": [], "body": "렌더링과 작업 관리가 개선되었습니다."
                }))
                .ok();
            }
            if name == "prepared-host-compact" {
                self.app = fixture();
                self.app.running = false;
                self.app.monitor = false;
            }
            if name.starts_with("queue-connect") {
                let mut state = self.app.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.dispatch_enabled = false;
                state.started = None;
                state.final_status = "작업 PC 연결 완료 · 렌더 시작 대기".into();
                self.app.settings.queue[0].status = crate::queue::Status::Waiting;
                self.app.settings.queue[0].done = 0;
                self.app.settings.queue[0].detail = state.final_status.clone();
                self.app.queue_automatic = false;
            }
            self.app.inspected = (name == "preview-compact")
                .then(|| {
                    self.app.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).worker_info.values().find_map(|worker| worker.preview.clone())
                })
                .flatten();
            self.app.settings.theme = if self.stage < self.cases.len() { settings::Theme::Dark } else { settings::Theme::Light };
        }
        if self.cases[self.stage % self.cases.len()].0 == "welcome" {
            self.app.draw_welcome(ui);
        } else {
            self.app.draw_interface(ui);
        }
        if self.app.updater.prompt {
            self.app.farm_update_ui(ui, true);
        }
        self.app.engine_update_ui(ui);
        self.frames += 1;
        if self.frames >= 24 && !self.requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible_text_rect(shape: &egui::Shape, clip: egui::Rect, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text.lines().next() == Some(label) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                clip.contains_rect(rect.shrink(0.5)).then_some(rect)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| visible_text_rect(shape, clip, label)),
            _ => None,
        }
    }

    fn frame_with_events(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 940.0))), events, ..Default::default() },
            |ui| app.draw_interface(ui),
        )
    }

    fn settled_frame(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
        for _ in 0..3 {
            frame_with_events(app, ctx, Vec::new()).drop_without_applying_deltas();
        }
        frame_with_events(app, ctx, Vec::new())
    }

    fn click_position(app: &mut App, ctx: &egui::Context, point: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame_with_events(app, ctx, vec![egui::Event::PointerMoved(point), button(true)]).drop_without_applying_deltas();
        frame_with_events(app, ctx, vec![button(false)]).drop_without_applying_deltas();
    }

    fn click_label(app: &mut App, ctx: &egui::Context, label: &str) {
        let output = settled_frame(app, ctx);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| visible_text_rect(&shape.shape, shape.clip_rect, label))
            .map(|rect| rect.center())
            .unwrap_or_else(|| panic!("missing visible label {label}"));
        output.drop_without_applying_deltas();
        click_position(app, ctx, point);
    }

    struct ImportFiles {
        directory: PathBuf,
        project: PathBuf,
    }

    impl ImportFiles {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "farm-ui-import-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            std::fs::create_dir(&directory).unwrap();
            let project = directory.join("Selected Project.ecproj");
            // Ready metadata isolates modal behavior from the asynchronous project parser.
            std::fs::write(&project, b"isolated UI import fixture").unwrap();
            Self { directory, project }
        }
    }

    impl Drop for ImportFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.project);
            let _ = std::fs::remove_dir(&self.directory);
        }
    }

    #[test]
    fn import_modal_requires_explicit_composition_selection_and_cancel_discards_the_draft() {
        let files = ImportFiles::new();
        for escape in [false, true] {
            let mut app = fixture();
            app.running = false;
            app.settings.reduced_motion = true;
            let before = app.settings.clone();
            app.project_import = Some(import::ProjectImport::ready(files.project.display().to_string(), import_compositions(), app.settings.render.clone()));
            let ctx = egui::Context::default();
            install_ui_font(&ctx).unwrap();
            settled_frame(&mut app, &ctx).drop_without_applying_deltas();
            assert!(app.project_import.as_ref().unwrap().selected.is_none());
            click_label(&mut app, &ctx, "대기열에 추가");
            assert!(app.settings == before, "disabled add must not enqueue the first composition implicitly");
            assert!(app.project_import.as_ref().unwrap().selected.is_none());
            click_label(&mut app, &ctx, "선택하세요");
            click_label(&mut app, &ctx, "메인 타이틀");
            assert_eq!(app.project_import.as_ref().unwrap().selected, Some(1));
            assert_eq!((app.project_import.as_ref().unwrap().range_start, app.project_import.as_ref().unwrap().range_end), (24, 360));
            if escape {
                frame_with_events(
                    &mut app,
                    &ctx,
                    vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
                )
                .drop_without_applying_deltas();
            } else {
                click_label(&mut app, &ctx, "취소");
            }
            assert!(app.project_import.is_none());
            assert!(app.settings == before, "cancel must preserve the live project, queue, and render settings");
        }
    }

    #[test]
    fn import_modal_validates_output_then_enqueues_only_the_selected_composition() {
        let files = ImportFiles::new();
        let mut app = fixture();
        app.running = false;
        app.settings.reduced_motion = true;
        let before = app.settings.clone();
        app.project_import = Some(import::ProjectImport::ready(files.project.display().to_string(), import_compositions(), app.settings.render.clone()));
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        click_label(&mut app, &ctx, "선택하세요");
        click_label(&mut app, &ctx, "메인 타이틀");
        let output = app.project_import.as_ref().unwrap().output.clone();
        app.project_import.as_mut().unwrap().output = files.directory.join("missing-directory/render.mp4").display().to_string();
        click_label(&mut app, &ctx, "대기열에 추가");
        assert!(app.settings == before);
        assert!(!app.project_import.as_ref().unwrap().error.is_empty(), "invalid output keeps the modal open and reports the error");
        app.project_import.as_mut().unwrap().output = output.clone();
        app.project_import.as_mut().unwrap().render.draft = true;
        click_label(&mut app, &ctx, "대기열에 추가");
        assert!(app.project_import.is_none());
        assert_eq!(app.settings.queue.len(), before.queue.len() + 1);
        let task = app.settings.queue.last().unwrap();
        assert_eq!(task.composition.name, "메인 타이틀");
        assert_eq!(task.project, files.project.display().to_string());
        assert_eq!(task.final_video, output);
        assert_eq!((task.range_start, task.range_end), (24, 360));
        assert!(task.render.draft && task.status == crate::queue::Status::Pending);
        assert_eq!(app.settings.project, before.project);
        assert_eq!(app.settings.comp, before.comp);
        assert!(app.settings.render == before.render);
        assert!(app.settings.queue_cards && app.monitor && app.queue_view);
        assert_eq!(app.queue_active, Some(1));
    }

    #[test]
    fn selected_project_file_is_read_asynchronously_before_the_user_chooses_and_enqueues_a_composition() {
        use effectcraft_color::Label;
        use effectcraft_project::{Comp, ItemKind, Project};
        use effectcraft_time::FrameRate;
        let files = ImportFiles::new();
        let mut project = Project::default();
        for name in ["Imported Intro", "Imported Main"] {
            let mut comp = Comp::new(1920, 1080, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(240));
            comp.work_area = (FrameRate::FPS_60.tick_of(30), FrameRate::FPS_60.tick_of(180));
            project.add_item(name, Label::None, None, ItemKind::Comp(Arc::new(comp)));
        }
        std::fs::write(&files.project, project.to_json()).unwrap();
        let mut app = fixture();
        app.running = false;
        app.settings.reduced_motion = true;
        let before = app.settings.clone();
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        app.begin_project_import(files.project.clone());
        assert!(app.project_import.is_some(), "the chooser opens as soon as a file is selected");
        let deadline = Instant::now() + Duration::from_secs(2);
        while app.project_import.as_ref().unwrap().compositions.is_empty() && Instant::now() < deadline {
            frame_with_events(&mut app, &ctx, Vec::new()).drop_without_applying_deltas();
            assert!(app.project_import.as_ref().unwrap().error.is_empty());
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.project_import.as_ref().unwrap().compositions.len(), 2);
        assert!(app.project_import.as_ref().unwrap().selected.is_none(), "loading metadata must not select or queue the first composition");
        assert!(app.settings == before);
        click_label(&mut app, &ctx, "선택하세요");
        click_label(&mut app, &ctx, "Imported Main");
        click_label(&mut app, &ctx, "대기열에 추가");
        assert!(app.project_import.is_none());
        assert_eq!(app.settings.queue.len(), before.queue.len() + 1);
        let task = app.settings.queue.last().unwrap();
        assert_eq!(task.composition.name, "Imported Main");
        assert_eq!(task.composition.frames, 240);
        assert_eq!((task.range_start, task.range_end), (30, 180));
        assert_eq!(app.settings.project, before.project);
        assert_eq!(app.settings.comp, before.comp);
    }

    #[test]
    fn invalid_project_import_reports_an_error_and_cannot_add_a_task() {
        let files = ImportFiles::new();
        let mut app = fixture();
        app.running = false;
        app.settings.reduced_motion = true;
        let before = app.settings.clone();
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        app.begin_project_import(files.project.clone());
        let deadline = Instant::now() + Duration::from_secs(2);
        while app.project_import.as_ref().unwrap().error.is_empty() && Instant::now() < deadline {
            frame_with_events(&mut app, &ctx, Vec::new()).drop_without_applying_deltas();
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.project_import.as_ref().unwrap().error.is_empty());
        assert!(app.project_import.as_ref().unwrap().compositions.is_empty());
        click_label(&mut app, &ctx, "대기열에 추가");
        assert!(app.settings == before);
        click_label(&mut app, &ctx, "취소");
        assert!(app.project_import.is_none());
    }

    #[test]
    fn imported_card_settings_can_be_changed_and_saved() {
        let files = ImportFiles::new();
        let mut app = fixture();
        app.running = false;
        app.settings.queue.clear();
        app.queue_previews.clear();
        app.queue_active = None;
        app.settings.reduced_motion = true;
        app.project_import = Some(import::ProjectImport::ready(files.project.display().to_string(), import_compositions(), app.settings.render.clone()));
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        click_label(&mut app, &ctx, "선택하세요");
        click_label(&mut app, &ctx, "메인 타이틀");
        click_label(&mut app, &ctx, "대기열에 추가");
        assert_eq!(app.settings.queue.len(), 1);
        settled_frame(&mut app, &ctx).drop_without_applying_deltas();
        let gear = ctx.read_response(egui::Id::new(("queue-settings", app.settings.queue[0].id))).unwrap().rect.center();
        click_position(&mut app, &ctx, gear);
        assert_eq!(app.queue_detail, Some(app.settings.queue[0].id));
        assert!(!app.settings.queue[0].render.draft);
        click_label(&mut app, &ctx, "Draft 품질");
        click_label(&mut app, &ctx, "설정 저장");
        assert!(app.queue_detail.is_none());
        assert!(app.settings.queue[0].render.draft);
        assert!(!app.settings.render.draft);
    }

    #[test]
    fn queue_settings_gear_is_revealed_on_item_hover_and_opens_only_that_task() {
        fn settings_image(shape: &egui::Shape, texture: egui::TextureId, rect: egui::Rect) -> bool {
            match shape {
                egui::Shape::Mesh(mesh) => mesh.texture_id == texture && rect.contains_rect(mesh.calc_bounds().shrink(0.5)),
                egui::Shape::Rect(image) => {
                    image.brush.as_ref().is_some_and(|brush| brush.fill_texture_id == texture) && rect.contains_rect(image.rect.shrink(0.5))
                }
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| settings_image(shape, texture, rect)),
                _ => false,
            }
        }
        for cards in [false, true] {
            let mut app = fixture();
            app.queue_view = true;
            app.settings.queue_cards = cards;
            app.settings.reduced_motion = true;
            let ctx = egui::Context::default();
            install_ui_font(&ctx).unwrap();
            frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))]).drop_without_applying_deltas();
            let output = settled_frame(&mut app, &ctx);
            let gear = ctx.read_response(egui::Id::new(("queue-settings", 2_u64))).unwrap().rect;
            let texture = ctx.data(|data| data.get_temp::<egui::TextureHandle>(egui::Id::new(("gravity-icon", "settings")))).unwrap().id();
            assert!(!output.shapes.iter().any(|shape| settings_image(&shape.shape, texture, gear)), "gear must be hidden before item hover");
            output.drop_without_applying_deltas();

            click_label(&mut app, &ctx, "Scene 2 · Final");
            assert!(app.queue_detail.is_none(), "the title is not the settings trigger");
            let output = settled_frame(&mut app, &ctx);
            assert!(output.shapes.iter().any(|shape| settings_image(&shape.shape, texture, gear)), "hovering the item reveals its settings gear");
            output.drop_without_applying_deltas();

            frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))]).drop_without_applying_deltas();
            let output = settled_frame(&mut app, &ctx);
            assert!(!output.shapes.iter().any(|shape| settings_image(&shape.shape, texture, gear)), "leaving the item hides its gear");
            output.drop_without_applying_deltas();
            frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(gear.center())]).drop_without_applying_deltas();
            click_position(&mut app, &ctx, gear.center());
            assert_eq!(app.queue_detail, Some(2));
            assert_eq!(app.queue_edit.as_ref().map(|task| task.id), Some(2));
            assert_eq!(app.queue_edit.as_ref().map(|task| task.composition.name.as_str()), Some("Scene 2 · Final"));
            assert_eq!(app.queue_active, Some(1));
            assert!(app.state.lock().unwrap().dispatch_enabled);
        }
    }

    #[test]
    fn desktop_fleet_keeps_all_six_computers_visible_without_scrolling() {
        for theme in [settings::Theme::Dark, settings::Theme::Light] {
            let mut app = fixture();
            app.settings.theme = theme;
            app.settings.reduced_motion = true;
            let ctx = egui::Context::default();
            install_ui_font(&ctx).unwrap();
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 940.0));
            for _ in 0..3 {
                ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            }
            let output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| app.draw_interface(ui));
            for name in ["Studio Desktop", "Ryzen Notebook", "Mac Studio Worker", "Editing PC", "Render Node 02", "Render Node 03"] {
                let visible = output.shapes.iter().any(|shape| visible_text_rect(&shape.shape, shape.clip_rect.intersect(viewport), name).is_some());
                assert!(visible, "computer {name} must be fully visible in the desktop fleet table ({theme:?})");
            }
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn queue_toolbar_search_and_status_filters_preserve_the_render_schedule() {
        fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
            ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 940.0))), events, ..Default::default() },
                |ui| app.draw_interface(ui),
            )
        }
        fn settle(app: &mut App, ctx: &egui::Context) -> egui::FullOutput {
            for _ in 0..2 {
                frame(app, ctx, Vec::new()).drop_without_applying_deltas();
            }
            frame(app, ctx, Vec::new())
        }
        fn click(app: &mut App, ctx: &egui::Context, label: &str) {
            let output = settle(app, ctx);
            let point = output
                .shapes
                .iter()
                .find_map(|shape| visible_text_rect(&shape.shape, shape.clip_rect, label))
                .map(|rect| rect.center())
                .unwrap_or_else(|| panic!("missing visible control {label}"));
            output.drop_without_applying_deltas();
            let button = |pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(app, ctx, vec![egui::Event::PointerMoved(point), button(true)]).drop_without_applying_deltas();
            frame(app, ctx, vec![button(false)]).drop_without_applying_deltas();
        }
        fn assert_tasks(app: &mut App, ctx: &egui::Context, expected: &[u64]) {
            let output = settle(app, ctx);
            for task in &app.settings.queue {
                // The current composition also appears in the independent live
                // status card below the list. Filter assertions concern rows.
                let visible = output
                    .shapes
                    .iter()
                    .any(|shape| visible_text_rect(&shape.shape, shape.clip_rect, &task.composition.name).is_some_and(|rect| rect.center().y < 580.0));
                assert_eq!(visible, expected.contains(&task.id), "visibility of {}", task.composition.name);
            }
            assert_eq!(app.settings.queue.iter().map(|task| task.id).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
            assert_eq!(app.queue_active, Some(1));
            assert!(app.running && app.state.lock().unwrap().dispatch_enabled);
            assert!(!app.stopping && app.queue_detail.is_none());
            output.drop_without_applying_deltas();
        }
        let mut app = fixture();
        app.queue_view = true;
        app.settings.queue_cards = false;
        app.settings.reduced_motion = true;
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        assert_tasks(&mut app, &ctx, &[1, 2, 3, 4]);
        click(&mut app, &ctx, "프로젝트, 컴포지션, 출력 파일 검색…");
        frame(&mut app, &ctx, vec![egui::Event::Text("sCeNe 3".into())]).drop_without_applying_deltas();
        assert_tasks(&mut app, &ctx, &[3]);
        click(&mut app, &ctx, "필터 초기화");
        assert_tasks(&mut app, &ctx, &[1, 2, 3, 4]);
        click(&mut app, &ctx, "모든 상태");
        click(&mut app, &ctx, "대기 작업");
        assert_tasks(&mut app, &ctx, &[2, 3, 4]);
        click(&mut app, &ctx, "대기 작업");
        click(&mut app, &ctx, "완료");
        assert_tasks(&mut app, &ctx, &[]);
        click(&mut app, &ctx, "전체 작업 보기");
        assert_tasks(&mut app, &ctx, &[1, 2, 3, 4]);
    }

    #[test]
    fn modal_opening_stays_in_sync_and_closing_clears_the_overlay() {
        let ctx = egui::Context::default();
        let check_frame = |time, open, reduced| {
            let mut opacity = None;
            ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |_| {
                if let Some((modal, reveal)) = animated_modal(&ctx, "closing-regression", open, reduced, card(&ctx)) {
                    assert!(modal.backdrop_color.a().abs_diff((160.0 * reveal).round() as u8) <= 1);
                    assert_eq!(modal.frame.unwrap().fill, card(&ctx).fill.gamma_multiply(reveal));
                    modal.show(&ctx, |ui| ui.label("Dialog"));
                    opacity = Some(reveal);
                }
            })
            .drop_without_applying_deltas();
            opacity
        };
        assert_eq!(check_frame(0.0, true, false), Some(0.0));
        assert!((0.0..1.0).contains(&check_frame(0.05, true, false).unwrap()));
        assert_eq!(check_frame(0.25, true, false), Some(1.0));
        assert_eq!(check_frame(0.26, false, false), None);
        assert_eq!(check_frame(0.27, true, false), Some(0.0));
        assert_eq!(check_frame(0.28, true, true), Some(1.0));
    }

    #[test]
    fn compact_project_page_starts_with_project_import_without_inline_composition_editor() {
        let mut app = fixture();
        app.running = false;
        app.monitor = false;
        app.settings.reduced_motion = true;
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        let mut labels = String::new();
        fn collect(shape: &egui::Shape, labels: &mut String) {
            match shape {
                egui::Shape::Text(text) => labels.push_str(&text.galley.job.text),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, labels);
                    }
                }
                _ => {}
            }
        }
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 740.0))), ..Default::default() };
            let output = ctx.run_ui(input, |ui| app.draw_interface(ui));
            labels.clear();
            for shape in &output.shapes {
                collect(&shape.shape, &mut labels);
            }
            output.drop_without_applying_deltas();
        }
        assert!(labels.contains("프로젝트 준비"));
        assert!(labels.contains("컴포지션 추가"));
        assert!(!labels.contains("컴포지션 선택"));
        assert!(!labels.contains("최종 MP4"));
        assert!(!labels.contains("상세 설정"));
    }

    #[test]
    fn sidebar_and_compact_navigation_switch_views_without_interrupting_render() {
        fn navigation_position(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text)
                    if (text.galley.job.text == label || text.galley.job.text.starts_with(&format!("{label}  ")))
                        && text.galley.job.sections.iter().all(|section| section.format.font_id.size <= 15.0) =>
                {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| navigation_position(shape, label)),
                _ => None,
            }
        }

        for width in [560.0, 1440.0] {
            let mut app = fixture();
            app.settings.reduced_motion = true;
            let ctx = egui::Context::default();
            install_ui_font(&ctx).unwrap();
            let input = |events| egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 740.0))),
                events,
                ..Default::default()
            };
            let labels = if width < 720.0 { ["렌더 큐", "프로젝트 준비", "렌더 모니터"] } else { ["목록", "연결", "모니터"] };
            for (label, monitor, queue_view) in [(labels[0], true, true), (labels[1], false, false), (labels[2], true, false)] {
                let mut position = None;
                for _ in 0..3 {
                    let output = ctx.run_ui(input(Vec::new()), |ui| app.draw_interface(ui));
                    position = output.shapes.iter().find_map(|shape| navigation_position(&shape.shape, label));
                    output.drop_without_applying_deltas();
                }
                let position = position.unwrap_or_else(|| panic!("missing navigation {label} at width {width}"));
                let button =
                    |pressed| egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
                ctx.run_ui(input(vec![egui::Event::PointerMoved(position), button(true)]), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
                ctx.run_ui(input(vec![button(false)]), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
                assert_eq!((app.monitor, app.queue_view), (monitor, queue_view), "navigation {label} at width {width}");
                assert!(app.running && app.state.lock().unwrap().dispatch_enabled);
                assert!(!app.stopping && !app.stop.load(Ordering::Relaxed));
                assert!(app.pending_start.is_none() && !app.settings_open && !app.logs_open && app.queue_detail.is_none());
                assert_eq!(app.queue_active, Some(1));
                assert_eq!(app.settings.queue.len(), 4);
            }
        }
    }

    #[test]
    fn automatic_render_keeps_gpu_enabled_when_manual_cpu_choice_was_saved() {
        let mut app = fixture();
        app.settings.gpu = false;
        app.settings.render.automatic = true;
        assert!(app.config().gpu);
        app.settings.render.automatic = false;
        assert!(!app.config().gpu);
        app.settings.gpu = true;
        assert!(app.config().gpu);
    }

    #[test]
    fn update_consent_finishes_current_work_and_can_be_cancelled_before_install() {
        let mut app = fixture();
        app.queue_automatic = true;
        app.engine_updater.required = crate::effectcraft_update::requirement("0.22.0", Some("0.23.1"), None).unwrap();
        let assignment = app.state.lock().unwrap().worker_info.get(&1).unwrap().assigned;
        app.request_engine_install();
        assert!(app.engine_install_requested && app.stopping && app.stop.load(Ordering::Relaxed));
        assert!(!app.queue_automatic && !app.engine_updater.installing());
        assert_eq!(app.state.lock().unwrap().worker_info.get(&1).unwrap().assigned, assignment);
        app.engine_install_requested = false;
        app.poll_engine_update();
        assert!(!app.engine_updater.installing());
    }

    #[test]
    fn participation_toggle_updates_scheduler_and_remembered_pc_choice() {
        let mut app = fixture();
        let worker = app.state.lock().unwrap().worker_info.get(&2).unwrap().clone();
        app.set_worker_participation(&worker, false);
        assert!(!app.state.lock().unwrap().worker_info.get(&2).unwrap().enabled);
        assert!(app.settings.disabled_workers.contains(&worker.ip));
        app.set_worker_participation(&worker, true);
        assert!(app.state.lock().unwrap().worker_info.get(&2).unwrap().enabled);
        assert!(!app.settings.disabled_workers.contains(&worker.ip));
    }

    #[test]
    fn queue_actions_offer_host_connection_before_render_start() {
        fn text(shape: &egui::Shape, result: &mut String) {
            match shape {
                egui::Shape::Text(shape) => result.push_str(&shape.galley.job.text),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, result);
                    }
                }
                _ => {}
            }
        }
        for width in [560.0, 1440.0] {
            for started in [None, Some(false), Some(true)] {
                let mut app = fixture();
                app.running = started.is_some();
                app.queue_automatic = started == Some(true);
                app.state.lock().unwrap().dispatch_enabled = started == Some(true);
                let ctx = egui::Context::default();
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 740.0))), ..Default::default() };
                let output = ctx.run_ui(input, |root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        app.action_bar(ui);
                    });
                    assert!(root.min_rect().width() <= width + 1.0);
                });
                let mut labels = String::new();
                for shape in &output.shapes {
                    text(&shape.shape, &mut labels);
                }
                match started {
                    None => {
                        assert!(labels.contains("호스트 열기"));
                        assert!(!labels.contains("큐 시작"));
                    }
                    Some(false) => {
                        assert!(labels.contains("렌더 시작"));
                        assert!(labels.contains("사용할 연결 PC"));
                    }
                    Some(true) => {
                        assert!(!labels.contains("렌더 시작"));
                        assert!(labels.contains("연결 종료"));
                    }
                }
                assert_eq!(app.state.lock().unwrap().dispatch_enabled, started == Some(true));
                output.drop_without_applying_deltas();
            }
        }
    }

    #[test]
    fn host_preparation_displays_scan_progress_and_can_be_cancelled() {
        let mut app = fixture();
        app.running = false;
        app.queue_view = true;
        app.settings.reduced_motion = true;
        let (_tx, rx) = mpsc::channel();
        app.pending_start = Some(rx);
        app.state.lock().unwrap().final_status = "기존 프레임 확인 중 · 64 / 100".into();
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        let output = settled_frame(&mut app, &ctx);
        assert!(output.shapes.iter().any(|shape| visible_text_rect(&shape.shape, shape.clip_rect, "기존 프레임 확인 중 · 64 / 100").is_some()));
        assert!(output.shapes.iter().any(|shape| visible_text_rect(&shape.shape, shape.clip_rect, "렌더링 준비 중").is_some()));
        output.drop_without_applying_deltas();
        click_label(&mut app, &ctx, "준비 취소");
        assert!(app.stopping && app.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn responsive_views_and_modals_fit_at_supported_sizes() {
        let prototype = fixture();
        for theme in [settings::Theme::Dark, settings::Theme::Light] {
            for width in [560.0, 780.0, 1120.0, 1440.0, 1920.0, 2560.0] {
                let ctx = egui::Context::default();
                install_ui_font(&ctx).unwrap();
                ctx.set_theme(theme.egui());
                for view in 0_u8..13 {
                    let mut app = App::with_state(prototype.settings.clone(), None, prototype.state.clone());
                    app.catalog = prototype.catalog.clone();
                    app.job = prototype.job.clone();
                    app.running = true;
                    app.monitor = view != 0;
                    app.settings.reduced_motion = true;
                    app.settings.theme = theme;
                    app.settings_open = (2..=5).contains(&view);
                    app.settings_tab = view.saturating_sub(2);
                    app.logs_open = view == 6;
                    app.worker_details = (view == 7).then_some(1);
                    app.preview_open = view == 8;
                    app.settings.queue_cards = view >= 10;
                    app.queue_view = view >= 9;
                    app.queue_previews = prototype.queue_previews.clone();
                    app.queue_detail = (view >= 11).then_some(if view == 11 { 1 } else { 2 });
                    for frame in 0..3 {
                        let input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 740.0))),
                            time: Some((view as f64 * 3.0 + frame as f64) / 60.0),
                            ..Default::default()
                        };
                        let output = ctx.run_ui(input, |ui| {
                            app.draw_interface(ui);
                            assert!(ui.min_rect().width() <= width + 1.0, "horizontal overflow at {width}, view {view}: {:?}", ui.min_rect());
                        });
                        assert_eq!(ctx.theme(), if theme == settings::Theme::Dark { egui::Theme::Dark } else { egui::Theme::Light });
                        assert!(!output.shapes.is_empty());
                        output.drop_without_applying_deltas();
                    }
                }
            }
        }
    }

    #[test]
    fn selecting_theme_in_settings_updates_the_visible_theme() {
        fn text_position(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos + text.galley.size() * 0.5),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_position(shape, label)),
                _ => None,
            }
        }
        fn contains_fill(shape: &egui::Shape, expected: egui::Color32) -> bool {
            match shape {
                egui::Shape::Rect(rect) => rect.fill == expected,
                egui::Shape::Vec(shapes) => shapes.iter().any(|shape| contains_fill(shape, expected)),
                _ => false,
            }
        }

        let mut app = fixture();
        app.settings.reduced_motion = true;
        app.settings_open = true;
        app.settings_tab = 0;
        app.settings.theme = settings::Theme::Dark;
        let ctx = egui::Context::default();
        install_ui_font(&ctx).unwrap();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            system_theme: Some(egui::Theme::Light),
            events,
            ..Default::default()
        };
        for (label, expected) in [("라이트", settings::Theme::Light), ("다크", settings::Theme::Dark), ("시스템 설정", settings::Theme::System)] {
            let mut point = None;
            for _ in 0..3 {
                let output = ctx.run_ui(input(Vec::new()), |ui| app.draw_interface(ui));
                point = output.shapes.iter().find_map(|shape| text_position(&shape.shape, label));
                output.drop_without_applying_deltas();
            }
            let point = point.unwrap();
            let button = |pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            ctx.run_ui(input(vec![egui::Event::PointerMoved(point), button(true)]), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            ctx.run_ui(input(vec![button(false)]), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            // The root Ui is created before draw_interface changes the context theme.
            ctx.run_ui(input(Vec::new()), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            let output = ctx.run_ui(input(Vec::new()), |ui| app.draw_interface(ui));
            assert_eq!(app.settings.theme, expected);
            let resolved = if expected == settings::Theme::Dark { egui::Theme::Dark } else { egui::Theme::Light };
            assert_eq!(ctx.theme(), resolved);
            let expected_background = ctx.style_of(resolved).visuals.panel_fill;
            assert!(output.shapes.iter().any(|shape| contains_fill(&shape.shape, expected_background)));
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn advanced_controls_are_hidden_and_modals_close_with_escape() {
        let mut app = fixture();
        app.settings.reduced_motion = true;
        let ctx = egui::Context::default();
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 940.0))), ..Default::default() };
        fn text(shape: &egui::Shape, result: &mut String) {
            match shape {
                egui::Shape::Text(shape) => result.push_str(&shape.galley.job.text),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, result);
                    }
                }
                _ => {}
            }
        }
        let output = ctx.run_ui(input(), |ui| app.draw_interface(ui));
        let mut labels = String::new();
        for shape in &output.shapes {
            text(&shape.shape, &mut labels);
        }
        assert!(!labels.contains("FFmpeg 경로"));
        assert!(!labels.contains("디스크 동기화"));
        output.drop_without_applying_deltas();
        for modal in 0..4 {
            app.settings_open = modal == 0;
            app.settings_tab = 1;
            app.logs_open = modal == 1;
            app.worker_details = (modal == 2).then_some(1);
            app.preview_open = modal == 3;
            for _ in 0..2 {
                ctx.run_ui(input(), |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            }
            let mut escape = input();
            escape.events.push(egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
            ctx.run_ui(escape, |ui| app.draw_interface(ui)).drop_without_applying_deltas();
            assert!(!app.settings_open && !app.logs_open && app.worker_details.is_none() && !app.preview_open, "modal {modal} did not close");
            assert!(!app.stopping && !app.stop.load(Ordering::Relaxed));
        }
    }
}
