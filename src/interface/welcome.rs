//! Startup role chooser, adapted from Figma's 550 × 558 welcome screen.
use super::*;

fn image_at(ui: &mut egui::Ui, name: &'static str, bytes: &'static [u8], rect: egui::Rect, tint: egui::Color32, radius: u8) {
    if let Some(image) = figma_svg(ui, name, bytes, rect.size()) {
        let mut image = image.tint(tint);
        if radius != 0 {
            image = image.corner_radius(radius);
        }
        ui.put(rect, image.sense(egui::Sense::hover()));
    }
}

fn text(ui: &egui::Ui, pos: egui::Pos2, align: egui::Align2, value: &str, size: f32, color: egui::Color32) {
    paint_text(ui, pos, align, value, egui::FontId::proportional(size), color);
}

fn medium_text(ui: &egui::Ui, pos: egui::Pos2, align: egui::Align2, value: &str, size: f32, color: egui::Color32) {
    paint_text(ui, pos, align, value, figma_font(size, false), color);
}

fn paint_text(ui: &egui::Ui, pos: egui::Pos2, align: egui::Align2, value: &str, font: egui::FontId, color: egui::Color32) {
    let line_height = match font.size as u32 {
        12 => 16.0,
        14 => 20.0,
        16 => 24.0,
        24 => 32.0,
        _ => font.size,
    };
    let format = egui::TextFormat { font_id: font, color, extra_letter_spacing: -0.4, line_height: Some(line_height), ..Default::default() };
    let galley = ui.painter().layout_job(egui::text::LayoutJob::single_section(value.to_owned(), format));
    ui.painter().galley(align.anchor_size(pos, galley.size()).min, galley, color);
}

/// Figma's role cards use a subtle diagonal blue or green wash on hover.
fn hover_gradient(ui: &egui::Ui, rect: egui::Rect, background: egui::Color32, highlight: egui::Color32, amount: f32) {
    let target = [highlight.r() as f32, highlight.g() as f32, highlight.b() as f32];
    let radius = f32::from(layout::RADIUS);
    let base = [background.r() as f32, background.g() as f32, background.b() as f32];
    let mut mesh = egui::Mesh::default();
    const ROWS: usize = 24;
    const COLS: usize = 16;
    let angle = 104.611_8_f32.to_radians();
    let direction = egui::vec2(angle.sin(), -angle.cos());
    let length = rect.width() * direction.x.abs() + rect.height() * direction.y.abs();
    for row in 0..=ROWS {
        let y = rect.height() * row as f32 / ROWS as f32;
        let edge = y.min(rect.height() - y);
        let inset = if edge < radius { radius - (radius.powi(2) - (radius - edge).powi(2)).max(0.0).sqrt() } else { 0.0 };
        for col in 0..=COLS {
            let x = inset + (rect.width() - 2.0 * inset) * col as f32 / COLS as f32;
            let projection = (x - rect.width() / 2.0) * direction.x + (y - rect.height() / 2.0) * direction.y;
            let t = ((projection / length + 0.5) / 0.94017).clamp(0.0, 1.0) * 0.08 * amount;
            let channel = |i: usize| (base[i] * (1.0 - t) + target[i] * t).round() as u8;
            mesh.colored_vertex(rect.min + egui::vec2(x, y), egui::Color32::from_rgb(channel(0), channel(1), channel(2)));
        }
    }
    for row in 0..ROWS {
        for col in 0..COLS {
            let first = (row * (COLS + 1) + col) as u32;
            let below = first + (COLS + 1) as u32;
            mesh.add_triangle(first, below, first + 1);
            mesh.add_triangle(first + 1, below, below + 1);
        }
    }
    ui.painter().add(mesh);
}

/// Preserve the Figma hover icon's native colors in either theme.
fn hover_icon(ui: &egui::Ui, host: bool, rect: egui::Rect, amount: f32) {
    let (name, bytes): (_, &'static [u8]) = if host {
        ("welcome-server-hover", include_bytes!("../../assets/ui/figma-welcome/server-hover.svg"))
    } else {
        ("welcome-node-hover", include_bytes!("../../assets/ui/figma-welcome/node-hover.svg"))
    };
    let id = egui::Id::new(("welcome-hover-icon", name));
    let texture = ui.ctx().data(|data| data.get_temp::<egui::TextureHandle>(id)).or_else(|| {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(48, 48)?;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(2.0, 2.0), &mut pixmap.as_mut());
        let image = egui::ColorImage::from_rgba_premultiplied([48, 48], pixmap.data());
        let texture = ui.ctx().load_texture(name, image, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|data| data.insert_temp(id, texture.clone()));
        Some(texture)
    });
    if let Some(texture) = texture {
        egui::Image::new((texture.id(), rect.size())).fit_to_exact_size(rect.size()).tint(egui::Color32::WHITE.gamma_multiply(amount)).paint_at(ui, rect);
    }
}

fn role_card(ui: &mut egui::Ui, rect: egui::Rect, host: bool, palette: &layout::Palette) -> bool {
    let foreground = palette.text;
    let muted = palette.muted;
    let fill = palette.surface;
    let outline = if host { palette.accent } else { palette.success };
    let title = if host { "서버로 실행" } else { "노드로 실행" };
    let response = ui.interact(rect, ui.id().with(("welcome-role", host)), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title));
    let hover = motion::highlight(ui, response.id.with("hover"), response.hovered() || response.has_focus());
    let card = ui.new_child(egui::UiBuilder::new().id_salt(("welcome-role-paint", host)).max_rect(rect));
    card.painter().rect_filled(rect, layout::RADIUS, fill);
    if hover > 0.0 {
        hover_gradient(&card, rect, fill, outline, hover);
    }
    let stroke = if response.has_focus() { outline } else { palette.border.lerp_to_gamma(outline, hover * 0.72) };
    card.painter().rect_stroke(rect, layout::RADIUS, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let icon_rect = egui::Rect::from_min_size(rect.min + egui::vec2(16.0, 16.0), egui::Vec2::splat(24.0));
    if hover < 1.0 {
        let (name, bytes): (_, &'static [u8]) = if host {
            ("server", include_bytes!("../../assets/ui/figma-welcome/server.svg"))
        } else {
            ("node", include_bytes!("../../assets/ui/figma-welcome/node.svg"))
        };
        if let Some(icon) = figma_svg(&card, name, bytes, icon_rect.size()) {
            icon.tint(foreground.gamma_multiply(1.0 - hover)).paint_at(&card, icon_rect);
        }
    }
    if hover > 0.0 {
        hover_icon(&card, host, icon_rect, hover);
    }
    medium_text(&card, rect.min + egui::vec2(16.0, 52.0), egui::Align2::LEFT_TOP, title, 14.0, foreground);
    let (first, second) = if host {
        ("프로젝트의 렌더링 작업을 관리하고", "연결된 PC에 맞춰 렌더링 범위를 분배해요")
    } else {
        ("이 PC의 자원을 활용하여 다른 기기의", "렌더링 작업을 함께 처리해요.")
    };
    text(&card, rect.min + egui::vec2(16.0, 82.0), egui::Align2::LEFT_TOP, first, 12.0, muted);
    text(&card, rect.min + egui::vec2(16.0, 100.0), egui::Align2::LEFT_TOP, second, 12.0, muted);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Draw the welcome screen. The returned value is the chosen host role.
pub(super) fn show(ui: &mut egui::Ui, theme: &mut settings::Theme) -> Option<bool> {
    let palette = layout::Palette::new(ui.ctx().theme() == egui::Theme::Dark);
    let foreground = palette.text;
    let muted = palette.muted;
    let all = ui.max_rect();
    ui.painter().rect_filled(all, 0, palette.canvas);
    let controls_right = all.right() - 10.0;
    let controls_top = all.top() + 10.0;
    shell::window_drag_region(ui, egui::Rect::from_min_max(all.min, egui::pos2((controls_right - 78.0).max(all.left()), all.top() + 52.0)), "welcome-drag");
    shell::window_controls(ui, controls_right, controls_top, false);
    let content = egui::Rect::from_center_size(all.center(), egui::vec2(510.0, 348.0));

    image_at(
        ui,
        "icon",
        include_bytes!("../../assets/ui/figma-welcome/icon-art.svg"),
        egui::Rect::from_center_size(content.center_top() + egui::vec2(0.0, 48.0), egui::Vec2::splat(96.0)),
        egui::Color32::WHITE,
        20,
    );
    medium_text(ui, content.center_top() + egui::vec2(0.0, 112.0), egui::Align2::CENTER_TOP, "EffectCraft Render Farm", 24.0, foreground);
    text(ui, content.center_top() + egui::vec2(0.0, 148.0), egui::Align2::CENTER_TOP, "더 많은 PC로, 더 빠른 렌더링", 16.0, muted);

    let left_rect = egui::Align2::LEFT_BOTTOM.anchor_size(content.left_bottom(), egui::vec2(249.0, 132.0));
    let right_rect = egui::Align2::RIGHT_BOTTOM.anchor_size(content.right_bottom(), egui::vec2(249.0, 132.0));
    let left = role_card(ui, left_rect, true, &palette);
    let right = role_card(ui, right_rect, false, &palette);

    let theme_rect = egui::Align2::LEFT_BOTTOM.anchor_size(all.shrink(20.0).left_bottom(), egui::Vec2::splat(32.0));
    let theme_response = ui.interact(theme_rect, ui.id().with("welcome-theme"), egui::Sense::click());
    theme_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "화면 테마 전환"));
    ui.painter().rect_filled(theme_rect, layout::CONTROL_RADIUS, palette.inset);
    let (name, bytes): (_, &'static [u8]) = match *theme {
        settings::Theme::Dark => ("moon", include_bytes!("../../assets/ui/figma-welcome/moon.svg")),
        settings::Theme::Light => ("topbar-sun", include_bytes!("../../assets/ui/figma/topbar-sun.svg")),
        settings::Theme::System => ("theme-system", include_bytes!("../../assets/ui/figma/theme-system.svg")),
    };
    image_at(ui, name, bytes, theme_rect.shrink(8.0), foreground, 0);
    let theme_response = theme_response.on_hover_text("화면 테마 선택").on_hover_cursor(egui::CursorIcon::PointingHand);
    theme_transition::menu(&theme_response, theme);
    medium_text(ui, theme_rect.right_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, &format!("R.{}", env!("CARGO_PKG_VERSION")), 12.0, muted);

    let github = egui::Align2::RIGHT_BOTTOM.anchor_size(all.shrink(20.0).right_bottom(), egui::vec2(115.0, 32.0));
    let response = ui.interact(github, ui.id().with("welcome-github"), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "EffectCraft GitHub"));
    ui.painter().rect_stroke(github, layout::CONTROL_RADIUS, egui::Stroke::new(1.0, palette.border), egui::StrokeKind::Inside);
    image_at(
        ui,
        "github",
        include_bytes!("../../assets/ui/figma-welcome/github.svg"),
        egui::Rect::from_min_size(github.min + egui::vec2(12.0, 8.0), egui::vec2(15.2381, 14.8618)),
        foreground,
        0,
    );
    medium_text(ui, github.left_center() + egui::vec2(32.0, 0.0), egui::Align2::LEFT_CENTER, "EffectCraft", 14.0, foreground);
    if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        let _ = webbrowser::open("https://github.com/storytold/effectcraft");
    }
    if left {
        Some(true)
    } else if right {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(ctx: &egui::Context, theme: &mut settings::Theme, size: egui::Vec2, events: Vec<egui::Event>) -> Option<bool> {
        let mut choice = None;
        ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() }, |ui| {
            choice = show(ui, theme)
        })
        .drop_without_applying_deltas();
        choice
    }

    #[test]
    fn role_cards_choose_distinct_roles() {
        for size in [egui::vec2(550.0, 520.0), egui::vec2(550.0, 558.0), egui::vec2(840.0, 720.0)] {
            for (offset, expected) in [(-130.0, true), (130.0, false)] {
                let ctx = egui::Context::default();
                install_ui_font(&ctx).unwrap();
                let mut theme = settings::Theme::Light;
                draw(&ctx, &mut theme, size, vec![]);
                // Click near the new bottom edge to catch a regression to the original upward offset.
                let point = egui::pos2(size.x / 2.0 + offset, size.y / 2.0 + 170.0);
                let button =
                    |pressed| egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
                assert_eq!(draw(&ctx, &mut theme, size, vec![egui::Event::PointerMoved(point), button(true)]), None);
                assert_eq!(draw(&ctx, &mut theme, size, vec![button(false)]), Some(expected));
            }
        }
    }
}
