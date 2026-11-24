use super::*;

const REVEAL_SECONDS: f64 = 0.42;
const CAPTURE_TIMEOUT: Duration = Duration::from_millis(150);
const EDGE_WIDTH: f32 = 5.0;
const SEGMENTS: usize = 128;

#[derive(Clone, Default)]
struct Pending {
    from: settings::Theme,
    to: settings::Theme,
    origin: egui::Pos2,
    selected_frame: u64,
    capture: Option<Capture>,
}

#[derive(Clone)]
struct Capture {
    token: egui::UserData,
    rect: egui::Rect,
    scale: f32,
    deadline: f64,
}

#[derive(Clone)]
struct Reveal {
    texture: egui::TextureHandle,
    rect: egui::Rect,
    scale: f32,
    origin: egui::Pos2,
    started: f64,
}

fn pending_id() -> egui::Id {
    egui::Id::new("theme-transition-pending")
}

fn reveal_id() -> egui::Id {
    egui::Id::new("theme-transition-reveal")
}

pub(super) fn menu(response: &egui::Response, selected: &mut settings::Theme) {
    egui::Popup::menu(response).show(|ui| {
        for (label, theme) in [("라이트", settings::Theme::Light), ("다크", settings::Theme::Dark), ("시스템 설정", settings::Theme::System)] {
            if ui.selectable_label(*selected == theme, label).clicked() {
                choose(ui.ctx(), selected, theme, response.rect.center());
                ui.close();
            }
        }
    });
}

pub(super) fn choose(ctx: &egui::Context, selected: &mut settings::Theme, to: settings::Theme, origin: egui::Pos2) {
    if *selected == to {
        ctx.data_mut(|data| data.remove::<Pending>(pending_id()));
        return;
    }
    let target = match to {
        settings::Theme::Dark => egui::Theme::Dark,
        settings::Theme::Light => egui::Theme::Light,
        settings::Theme::System => ctx.system_theme().unwrap_or_else(|| ctx.options(|options| options.fallback_theme)),
    };
    ctx.data_mut(|data| {
        data.remove::<Pending>(pending_id());
        data.remove::<Reveal>(reveal_id());
    });
    if ctx.global_style().animation_time == 0.0 || target == ctx.theme() {
        *selected = to;
        ctx.set_theme(to.egui());
    } else {
        let selected_frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|data| {
            data.insert_temp(pending_id(), Pending { from: *selected, to, origin, selected_frame, capture: None });
        });
    }
    ctx.request_repaint();
}

pub(crate) fn advance(ui: &mut egui::Ui, selected: &mut settings::Theme, reduced: bool) {
    let ctx = ui.ctx().clone();
    let Some(mut pending) = ctx.data_mut(|data| data.remove_temp::<Pending>(pending_id())) else { return };
    if *selected != pending.from {
        return;
    }

    let now = ctx.input(|input| input.time);
    if !reduced {
        match &pending.capture {
            None => {
                // The selection frame can still contain the closing menu.
                if ctx.cumulative_frame_nr() > pending.selected_frame {
                    let token = egui::UserData::new(());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(token.clone()));
                    pending.capture =
                        Some(Capture { token, rect: ctx.viewport_rect(), scale: ctx.pixels_per_point(), deadline: now + CAPTURE_TIMEOUT.as_secs_f64() });
                    ctx.request_repaint_after(CAPTURE_TIMEOUT);
                } else {
                    ctx.request_repaint();
                }
                ctx.data_mut(|data| data.insert_temp(pending_id(), pending));
                return;
            }
            Some(capture) if capture.rect == ctx.viewport_rect() && capture.scale == ctx.pixels_per_point() => {
                let viewport = ctx.viewport_id();
                let snapshot = ctx.input(|input| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::Screenshot { viewport_id, user_data, image } if *viewport_id == viewport && *user_data == capture.token => {
                            Some(image.clone())
                        }
                        _ => None,
                    })
                });
                if let Some(snapshot) = snapshot {
                    let texture = ctx.load_texture("previous-theme", (*snapshot).clone(), egui::TextureOptions::LINEAR);
                    ctx.data_mut(|data| {
                        data.insert_temp(reveal_id(), Reveal { texture, rect: capture.rect, scale: capture.scale, origin: pending.origin, started: now });
                    });
                } else if now < capture.deadline {
                    ctx.request_repaint_after(Duration::from_secs_f64(capture.deadline - now));
                    ctx.data_mut(|data| data.insert_temp(pending_id(), pending));
                    return;
                }
            }
            Some(_) => {}
        }
    }

    *selected = pending.to;
    ctx.set_theme(selected.egui());
    ui.set_style(ctx.global_style());
}

pub(crate) fn paint(ctx: &egui::Context, reduced: bool) {
    let Some(reveal) = ctx.data(|data| data.get_temp::<Reveal>(reveal_id())) else { return };
    let elapsed = (ctx.input(|input| input.time) - reveal.started) / REVEAL_SECONDS;
    if reduced || elapsed >= 1.0 || reveal.rect != ctx.viewport_rect() || reveal.scale != ctx.pixels_per_point() {
        ctx.data_mut(|data| data.remove::<Reveal>(reveal_id()));
        return;
    }

    let t = elapsed.clamp(0.0, 1.0) as f32;
    let progress = t * t * (3.0 - 2.0 * t);
    let mesh = outside_circle(reveal.rect, reveal.origin, progress, reveal.texture.id());
    ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, reveal_id())).with_clip_rect(reveal.rect).add(egui::Shape::mesh(mesh));
    ctx.request_repaint();
}

fn outside_circle(rect: egui::Rect, origin: egui::Pos2, progress: f32, texture: egui::TextureId) -> egui::Mesh {
    let reach =
        [rect.left_top(), rect.right_top(), rect.left_bottom(), rect.right_bottom()].into_iter().map(|corner| corner.distance(origin)).fold(0.0, f32::max);
    let radius = (reach + EDGE_WIDTH) * progress;
    let feather = EDGE_WIDTH.min(radius);
    let outer = (reach + EDGE_WIDTH) + 1.0;
    let mut mesh = egui::Mesh::with_texture(texture);
    for segment in 0..SEGMENTS {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        let direction = egui::Vec2::angled(angle);
        for (distance, tint) in [(radius - feather, egui::Color32::TRANSPARENT), (radius, egui::Color32::WHITE), (outer, egui::Color32::WHITE)] {
            let pos = origin + direction * distance;
            let uv = egui::pos2((pos.x - rect.left()) / rect.width(), (pos.y - rect.top()) / rect.height());
            mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
        }

        let current = (segment * 3) as u32;
        let next = (((segment + 1) % SEGMENTS) * 3) as u32;
        mesh.add_triangle(current, current + 1, next);
        mesh.add_triangle(next, current + 1, next + 1);
        mesh.add_triangle(current + 1, current + 2, next + 1);
        mesh.add_triangle(next + 1, current + 2, next + 2);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_waits_for_its_snapshot_and_stops_when_motion_is_reduced() {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Dark);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0));
        let origin = egui::pos2(40.0, 430.0);
        let input = |time, events| egui::RawInput { screen_rect: Some(rect), time: Some(time), events, ..Default::default() };
        let mut selected = settings::Theme::Dark;
        ctx.run_ui(input(0.0, vec![]), |ui| choose(ui.ctx(), &mut selected, settings::Theme::Light, origin)).drop_without_applying_deltas();

        let output = ctx.run_ui(input(0.02, vec![]), |ui| advance(ui, &mut selected, false));
        let capture = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .find_map(|command| match command {
                egui::ViewportCommand::Screenshot(token) => Some(token.clone()),
                _ => None,
            })
            .unwrap();
        output.drop_without_applying_deltas();
        assert_eq!(selected, settings::Theme::Dark);

        let screenshot = |user_data| egui::Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            user_data,
            image: Arc::new(egui::ColorImage::filled([2, 2], egui::Color32::BLACK)),
        };
        ctx.run_ui(input(0.03, vec![screenshot(egui::UserData::default())]), |ui| advance(ui, &mut selected, false)).drop_without_applying_deltas();
        assert_eq!(selected, settings::Theme::Dark);

        ctx.run_ui(input(0.04, vec![screenshot(capture)]), |ui| advance(ui, &mut selected, false)).drop_without_applying_deltas();
        assert_eq!(selected, settings::Theme::Light);
        let reveal = ctx.data(|data| data.get_temp::<Reveal>(reveal_id())).unwrap();
        assert_eq!(reveal.origin, origin);
        let output = ctx.run_ui(input(0.24, vec![]), |ui| paint(ui.ctx(), false));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == reveal.texture.id())));
        output.drop_without_applying_deltas();

        ctx.run_ui(input(0.25, vec![]), |ui| paint(ui.ctx(), true)).drop_without_applying_deltas();
        assert!(ctx.data(|data| data.get_temp::<Reveal>(reveal_id())).is_none());

        motion::apply_style(&ctx, true);
        choose(&ctx, &mut selected, settings::Theme::Dark, origin);
        assert_eq!(selected, settings::Theme::Dark);
        assert!(ctx.data(|data| data.get_temp::<Pending>(pending_id())).is_none());
    }
}
