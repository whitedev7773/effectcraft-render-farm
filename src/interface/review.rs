//! Isolated visual regression harness. Never compiled into the release installer.
use super::*;

fn fixture() -> App {
    let settings = Settings {
        project: "D:/Studio/Projects/2026/Autumn Campaign/Title Sequence.ecproj".into(),
        output: "D:/Studio/Renders/Title Sequence/frames".into(),
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
    state.messages = vec!["[INFO] [autotune] CPU/GPU 자동 비교 완료 · GPU 선택".into(), "[WARN] [encoder] 지원하지 않는 인코더를 제외했습니다".into()];
    let mut app = App::with_state(settings, None, Arc::new(Mutex::new(state)));
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

#[cfg(debug_assertions)]
pub(crate) fn run_requested() -> bool {
    let Some(directory) = std::env::var_os("EFFECTCRAFT_FARM_UI_CAPTURE").map(PathBuf::from) else { return false };
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("UI capture folder: {error}");
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
            Ok(Box::new(Capture { app: fixture(), directory, stage: 0, frames: 0, requested: false, started: Instant::now() }))
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
    stage: usize,
    frames: u32,
    requested: bool,
    started: Instant,
}

#[cfg(debug_assertions)]
const CASES: &[(&str, f32, f32)] = &[
    ("desktop", 1440.0, 940.0),
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
    ("queue-detail", 1440.0, 940.0),
    ("queue-detail-compact", 560.0, 740.0),
];

#[cfg(debug_assertions)]
impl eframe::App for Capture {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.started.elapsed() > Duration::from_secs(60) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let captured =
            ctx.input(|input| input.events.iter().find_map(|event| if let egui::Event::Screenshot { image, .. } = event { Some(image.clone()) } else { None }));
        if let Some(image) = captured {
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
            let path = self.directory.join(format!("{}.png", CASES[self.stage].0));
            if let Err(error) = image::save_buffer(&path, &bytes, image.width() as u32, image.height() as u32, image::ColorType::Rgba8) {
                eprintln!("UI capture save: {error}");
            }
            println!("{}: {}x{}", path.display(), image.width(), image.height());
            self.stage += 1;
            self.frames = 0;
            self.requested = false;
        }
        if self.stage >= CASES.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames == 0 {
            let (name, width, height) = CASES[self.stage];
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            self.app.monitor = name != "compact-project";
            self.app.running = !name.starts_with("settings");
            self.app.settings_open = name.starts_with("settings");
            self.app.settings_tab = u8::from(name == "settings-render");
            self.app.logs_open = name.starts_with("logs");
            self.app.worker_details = (name == "worker-compact").then_some(2);
            self.app.preview_open = name == "preview-compact";
            self.app.settings.queue_cards = name.starts_with("queue-cards");
            self.app.queue_detail = name.starts_with("queue-detail").then_some(2);
            self.app.inspected = (name == "preview-compact")
                .then(|| {
                    self.app.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).worker_info.values().find_map(|worker| worker.preview.clone())
                })
                .flatten();
        }
        self.app.draw_interface(ui);
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

    #[test]
    fn responsive_views_and_modals_fit_at_supported_sizes() {
        let prototype = fixture();
        for width in [560.0, 780.0, 1120.0, 1440.0, 1920.0, 2560.0] {
            for view in 0_u8..12 {
                let mut app = App::with_state(prototype.settings.clone(), None, prototype.state.clone());
                app.catalog = prototype.catalog.clone();
                app.job = prototype.job.clone();
                app.running = true;
                app.monitor = view != 0;
                app.settings.reduced_motion = true;
                app.settings_open = (2..=5).contains(&view);
                app.settings_tab = view.saturating_sub(2);
                app.logs_open = view == 6;
                app.worker_details = (view == 7).then_some(1);
                app.preview_open = view == 8;
                app.settings.queue_cards = view >= 9;
                app.queue_previews = prototype.queue_previews.clone();
                app.queue_detail = (view >= 10).then_some(if view == 10 { 1 } else { 2 });
                let ctx = egui::Context::default();
                for frame in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 740.0))),
                        time: Some(frame as f64 / 60.0),
                        ..Default::default()
                    };
                    let output = ctx.run_ui(input, |ui| {
                        app.draw_interface(ui);
                        assert!(ui.min_rect().width() <= width + 1.0, "horizontal overflow at {width}, view {view}: {:?}", ui.min_rect());
                    });
                    assert!(!output.shapes.is_empty());
                    output.drop_without_applying_deltas();
                }
            }
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
