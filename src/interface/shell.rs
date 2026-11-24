use super::*;

/// Native viewport commands behind the Figma window-control slots.
pub(super) fn window_controls(ui: &mut egui::Ui, right: f32, top: f32, full: bool) {
    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
    let count = if full { 3 } else { 2 };
    let left = right - (count as f32 * 32.0 + (count - 1) as f32 * 4.0);
    for index in 0..count {
        let (name, label, bytes): (&str, &str, &'static [u8]) = if index == 0 {
            ("window-minimize", "최소화", include_bytes!("../../assets/ui/figma/window-minimize.svg"))
        } else if full && index == 1 && maximized {
            ("window-collapse", "이전 크기로", include_bytes!("../../assets/ui/figma/window-collapse.svg"))
        } else if full && index == 1 {
            ("window-expand", "최대화", include_bytes!("../../assets/ui/figma/window-expand.svg"))
        } else {
            ("window-close", "닫기", include_bytes!("../../assets/ui/figma/window-close.svg"))
        };
        let rect = egui::Rect::from_min_size(egui::pos2(left + index as f32 * 36.0, top), egui::Vec2::splat(32.0));
        let response = ui.interact(rect, ui.id().with(name), egui::Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
        let hover = motion::highlight(ui, response.id.with("hover"), response.hovered());
        ui.painter().rect_filled(rect, layout::CONTROL_RADIUS, secondary(ui).gamma_multiply(hover));
        if let Some(image) = figma_svg(ui, name, bytes, egui::Vec2::splat(16.0)) {
            image.paint_at(ui, rect.shrink(8.0));
        }
        if response.on_hover_text(label).clicked() {
            let command = if index == 0 {
                egui::ViewportCommand::Minimized(true)
            } else if full && index == 1 {
                egui::ViewportCommand::Maximized(!maximized)
            } else {
                egui::ViewportCommand::Close
            };
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// Invoke platform dragging on the initial press, as required by native window managers.
pub(super) fn window_drag_region(ui: &mut egui::Ui, rect: egui::Rect, id: &'static str) {
    if rect.is_positive() {
        let response = ui.interact(rect, ui.id().with(id), egui::Sense::drag());
        if response.hovered() && ui.input(|input| input.pointer.primary_pressed()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
}

fn sidebar_item(ui: &mut egui::Ui, selected: bool, label: &str, asset: Option<(&'static str, &'static [u8])>) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, label));
    let hover = motion::highlight(ui, response.id.with("hover"), response.hovered());
    let selection = motion::highlight(ui, response.id.with("selected"), selected);
    let fill = secondary(ui).gamma_multiply(hover).lerp_to_gamma(ui.visuals().selection.bg_fill, selection);
    ui.painter().rect_filled(rect, layout::CONTROL_RADIUS, fill);
    let color = if selected { accent(ui) } else { muted(ui) };
    let center = rect.center_top() + egui::vec2(0.0, 16.0);
    if let Some((name, bytes)) = asset {
        if let Some(image) = figma_svg(ui, name, bytes, egui::vec2(16.0, 16.0)) {
            image.paint_at(ui, egui::Rect::from_center_size(center, egui::Vec2::splat(16.0)));
        }
    } else {
        let top = center + egui::vec2(0.0, -5.0);
        let left = center + egui::vec2(-5.0, 4.0);
        let right = center + egui::vec2(5.0, 4.0);
        ui.painter().line_segment([top, left], egui::Stroke::new(1.5, color));
        ui.painter().line_segment([top, right], egui::Stroke::new(1.5, color));
        for node in [top, left, right] {
            ui.painter().circle_filled(node, 2.5, color);
        }
    }
    ui.painter().text(rect.center_top() + egui::vec2(0.0, 36.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl App {
    /// Main workspace header, including the native viewport controls.
    pub(super) fn topbar(&mut self, ui: &mut egui::Ui) {
        let (bounds, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
        let top = bounds.top();
        let home = egui::Rect::from_min_size(egui::pos2(bounds.left(), top), egui::vec2(64.0, 32.0));
        let can_leave =
            self.engine_update_idle() && !self.stopping && !self.engine_install_requested && !self.updater.installing() && !self.engine_updater.installing();
        let home_response = ui.interact(home, ui.id().with("topbar-home"), egui::Sense::click());
        home_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, can_leave, "시작 화면으로"));
        let hover = motion::highlight(ui, home_response.id.with("hover"), can_leave && home_response.hovered());
        ui.painter().rect_filled(home, layout::CONTROL_RADIUS, surface(ui).lerp_to_gamma(secondary(ui), hover));
        ui.painter().rect_stroke(home, layout::CONTROL_RADIUS, egui::Stroke::new(1.0, border(ui)), egui::StrokeKind::Inside);
        for (name, bytes, offset) in [
            ("topbar-back", include_bytes!("../../assets/ui/figma/topbar-back.svg") as &'static [u8], 12.0),
            ("topbar-home", include_bytes!("../../assets/ui/figma/topbar-home.svg") as &'static [u8], 36.0),
        ] {
            if let Some(image) = figma_svg(ui, name, bytes, egui::Vec2::splat(16.0)) {
                image.paint_at(ui, egui::Rect::from_min_size(home.min + egui::vec2(offset, 8.0), egui::Vec2::splat(16.0)));
            }
        }
        if home_response.on_hover_text(if can_leave { "시작 화면으로" } else { "작업과 연결을 종료한 뒤 시작 화면으로 돌아갈 수 있습니다." }).clicked()
            && can_leave
        {
            self.show_welcome = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(550.0, 558.0)));
        }

        let app_icon = egui::Rect::from_min_size(egui::pos2(home.right() + 10.0, top + 6.0), egui::Vec2::splat(20.0));
        if let Some(image) = figma_svg(ui, "welcome-icon", include_bytes!("../../assets/ui/figma-welcome/icon-art.svg"), app_icon.size()) {
            image.corner_radius(4).paint_at(ui, app_icon);
        }
        let label_pos = app_icon.right_center() + egui::vec2(6.0, 0.0);
        let title = "EffectCraft Render Farm";
        let font = egui::FontId::proportional(12.0);
        let title_width = ui.painter().layout_no_wrap(title.into(), font.clone(), ui.visuals().text_color()).size().x;
        ui.painter().text(label_pos, egui::Align2::LEFT_CENTER, title, font.clone(), ui.visuals().text_color());
        let version = format!("R.{}", env!("CARGO_PKG_VERSION"));
        let version_width = ui.painter().layout_no_wrap(version.clone(), font.clone(), muted(ui)).size().x + 10.0;
        let version_rect = egui::Rect::from_min_size(egui::pos2(label_pos.x + title_width + 4.0, top + 8.0), egui::vec2(version_width, 16.0));
        ui.painter().rect_filled(version_rect, 8, surface(ui));
        ui.painter().text(version_rect.center(), egui::Align2::CENTER_CENTER, version, font, muted(ui));

        window_controls(ui, bounds.right(), top, true);
        let theme = egui::Rect::from_min_size(egui::pos2(bounds.right() - 104.0 - 10.0 - 32.0, top), egui::Vec2::splat(32.0));
        let update = theme.translate(egui::vec2(-42.0, 0.0));
        window_drag_region(
            ui,
            egui::Rect::from_min_max(egui::pos2(version_rect.right() + 6.0, bounds.top()), egui::pos2(update.left() - 6.0, bounds.bottom())),
            "workspace-drag",
        );
        let update_response = ui.interact(update, ui.id().with("topbar-update"), egui::Sense::click());
        update_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "업데이트 확인"));
        let available = self.updater.available.is_some() || self.engine_updater.required.is_some();
        let hover = motion::highlight(ui, update_response.id.with("hover"), update_response.hovered());
        let fill = if available { soft(accent(ui)) } else { secondary(ui) };
        ui.painter().rect_filled(update, layout::CONTROL_RADIUS, fill.lerp_to_gamma(ui.visuals().widgets.hovered.bg_fill, hover));
        if let Some(image) = figma_svg(ui, "topbar-update", include_bytes!("../../assets/ui/figma/topbar-update.svg"), egui::Vec2::splat(16.0)) {
            image.tint(if available { accent(ui) } else { ui.visuals().text_color() }).paint_at(ui, update.shrink(8.0));
        }
        if update_response.on_hover_text("업데이트 확인").clicked() {
            if self.engine_updater.required.is_some() {
                self.engine_updater.prompt = true;
            } else if self.updater.available.is_some() {
                self.updater.prompt = true;
            } else {
                self.updater.check();
            }
        }

        let theme_response = ui.interact(theme, ui.id().with("topbar-theme"), egui::Sense::click());
        theme_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "화면 테마 전환"));
        ui.painter().rect_filled(theme, layout::CONTROL_RADIUS, secondary(ui));
        let (name, bytes): (_, &'static [u8]) = match self.settings.theme {
            settings::Theme::Dark => ("topbar-moon", include_bytes!("../../assets/ui/figma/topbar-moon.svg")),
            settings::Theme::Light => ("topbar-sun", include_bytes!("../../assets/ui/figma/topbar-sun.svg")),
            settings::Theme::System => ("theme-system", include_bytes!("../../assets/ui/figma/theme-system.svg")),
        };
        if let Some(image) = figma_svg(ui, name, bytes, egui::Vec2::splat(16.0)) {
            image.paint_at(ui, theme.shrink(8.0));
        }
        let theme_response = theme_response.on_hover_text("화면 테마 선택");
        theme_transition::menu(&theme_response, &mut self.settings.theme);
    }

    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        if self.settings.host
            && sidebar_item(ui, self.monitor && self.queue_view, "목록", Some(("nav-list", include_bytes!("../../assets/ui/figma/nav-list.svg"))))
        {
            self.monitor = true;
            self.queue_view = true;
        }
        if sidebar_item(ui, self.monitor && !self.queue_view, "모니터", Some(("nav-monitor", include_bytes!("../../assets/ui/figma/nav-monitor.svg")))) {
            self.monitor = true;
            self.queue_view = false;
        }
        ui.separator();
        if sidebar_item(ui, !self.monitor, "연결", None) {
            self.monitor = false;
            self.queue_view = false;
        }
        if sidebar_item(ui, false, "기록", Some(("nav-history", include_bytes!("../../assets/ui/icons/clock-arrow-rotate-left.svg")))) {
            self.logs_open = true;
        }
        if sidebar_item(ui, false, "설정", Some(("nav-settings", include_bytes!("../../assets/ui/figma/nav-settings.svg")))) {
            self.settings_open = true;
        }
    }

    pub(super) fn page_heading(&mut self, ui: &mut egui::Ui, compact: bool) {
        let (section, title, description) = if !self.monitor {
            ("프로젝트", "프로젝트 준비", "프로젝트를 불러와 컴포지션을 선택하고 큐에 추가하세요.")
        } else if self.queue_view && self.settings.host {
            ("작업 관리", "렌더 큐", "작업을 검색하고 상태별로 확인하며 렌더 순서를 관리합니다.")
        } else {
            ("실시간 현황", "렌더 모니터", "렌더 진행과 모든 작업 PC의 상태를 확인합니다.")
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 78.0), egui::Sense::hover());
        ui.painter().text(
            rect.min + egui::vec2(0.0, 8.0),
            egui::Align2::LEFT_CENTER,
            format!("워크스페이스  /  {section}"),
            egui::FontId::proportional(11.0),
            muted(ui),
        );
        ui.painter().text(
            rect.min + egui::vec2(0.0, 35.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::TextStyle::Heading.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        ui.painter().text(rect.min + egui::vec2(0.0, 64.0), egui::Align2::LEFT_CENTER, description, egui::FontId::proportional(12.0), muted(ui));
        let tools = egui::Rect::from_min_max(egui::pos2(rect.right() - 220.0, rect.top() + 20.0), egui::pos2(rect.right(), rect.top() + 52.0));
        let mut actions =
            ui.new_child(egui::UiBuilder::new().id_salt("heading-actions").max_rect(tools).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if compact {
            if icon_button(&mut actions, UiIcon::Settings, "설정").clicked() {
                self.settings_open = true;
            }
            if icon_button(&mut actions, UiIcon::History, "작업 기록").clicked() {
                self.logs_open = true;
            }
        }
        if (self.updater.available.is_some() || self.engine_updater.required.is_some()) && icon_button(&mut actions, UiIcon::Update, "업데이트 보기").clicked()
        {
            if self.engine_updater.required.is_some() {
                self.engine_updater.prompt = true;
            } else {
                self.updater.prompt = true;
            }
        }
        let (label, color) = if self.stopping {
            ("종료 중", muted(ui))
        } else if self.pending_start.is_some() {
            ("준비 중", accent(ui))
        } else if self.running {
            let dispatch = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).dispatch_enabled;
            (
                if self.settings.host && dispatch {
                    "렌더링 중"
                } else if self.settings.host {
                    "PC 연결 대기"
                } else {
                    "호스트 연결됨"
                },
                success(ui),
            )
        } else {
            ("연결 대기", muted(ui))
        };
        badge(&mut actions, label, color);
        if self.updater.installing() || self.engine_updater.installing() {
            caption(ui, "업데이트를 다운로드하고 있습니다…");
        } else if self.engine_install_requested {
            caption(ui, "현재 작업을 마친 뒤 EffectCraft를 업데이트합니다.");
        }
    }
}
