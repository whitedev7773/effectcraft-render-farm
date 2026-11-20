use super::*;

#[derive(Clone, Copy)]
enum NavIcon {
    Project,
    Monitor,
    Queue,
}

fn nav_icon(ui: &egui::Ui, rect: egui::Rect, icon: NavIcon, color: egui::Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5, color);
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(17.0, 15.0));
    match icon {
        NavIcon::Project => {
            painter.rect_stroke(r, 2, stroke, egui::StrokeKind::Inside);
            painter.line_segment([r.left_top() + egui::vec2(0.0, 5.0), r.right_top() + egui::vec2(0.0, 5.0)], stroke);
            painter.line_segment([r.center() + egui::vec2(-3.0, 2.0), r.center() + egui::vec2(3.0, 2.0)], stroke);
            painter.line_segment([r.center() + egui::vec2(0.0, -1.0), r.center() + egui::vec2(0.0, 5.0)], stroke);
        }
        NavIcon::Monitor => {
            painter.rect_stroke(r.translate(egui::vec2(0.0, -2.0)), 2, stroke, egui::StrokeKind::Inside);
            painter.line_segment([r.center_bottom() + egui::vec2(0.0, -1.0), r.center_bottom() + egui::vec2(0.0, 3.0)], stroke);
            painter.line_segment([r.center_bottom() + egui::vec2(-4.0, 3.0), r.center_bottom() + egui::vec2(4.0, 3.0)], stroke);
        }
        NavIcon::Queue => {
            for y in [0.0, 6.0, 12.0] {
                let start = r.left_top() + egui::vec2(0.0, y + 1.0);
                painter.circle_filled(start + egui::vec2(1.0, 0.0), 1.5, color);
                painter.line_segment([start + egui::vec2(6.0, 0.0), start + egui::vec2(17.0, 0.0)], stroke);
            }
        }
    }
}

fn sidebar_item(ui: &mut egui::Ui, selected: bool, icon: NavIcon, label: &str, count: Option<usize>) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, label));
    let color = if selected {
        accent(ui)
    } else if response.hovered() {
        ui.visuals().text_color()
    } else {
        muted(ui)
    };
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 6, if selected { ui.visuals().selection.bg_fill } else { secondary(ui) });
    }
    nav_icon(ui, egui::Rect::from_min_size(rect.min + egui::vec2(12.0, 9.0), egui::vec2(20.0, 20.0)), icon, color);
    ui.painter().text(rect.left_center() + egui::vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), color);
    if let Some(count) = count {
        ui.painter().text(rect.right_center() - egui::vec2(12.0, 0.0), egui::Align2::RIGHT_CENTER, count, egui::FontId::monospace(12.0), color);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl App {
    pub(super) fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 32.0), egui::Sense::hover());
            let color = accent(ui);
            for (x, heights) in [(0.0, (7.0, 23.0)), (8.0, (0.0, 30.0)), (16.0, (12.0, 26.0))] {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(rect.min + egui::vec2(x + 3.0, heights.0), rect.min + egui::vec2(x + 8.0, heights.1)),
                    2,
                    color,
                );
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new("EffectCraft").size(16.0).strong());
                ui.label(egui::RichText::new("분산 렌더 워크스페이스").size(10.0).color(muted(ui)));
            });
        });
        ui.add_space(24.0);
        caption(ui, "워크스페이스");
        ui.add_space(4.0);
        if sidebar_item(ui, !self.monitor, NavIcon::Project, "프로젝트 준비", None) {
            self.monitor = false;
            self.queue_view = false;
        }
        if sidebar_item(ui, self.monitor && !self.queue_view, NavIcon::Monitor, "렌더 모니터", None) {
            self.monitor = true;
            self.queue_view = false;
        }
        if self.settings.host && sidebar_item(ui, self.monitor && self.queue_view, NavIcon::Queue, "렌더 큐", Some(self.settings.queue.len())) {
            self.monitor = true;
            self.queue_view = true;
        }
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(8.0);
        if ui.add_sized([ui.available_width(), 36.0], egui::Button::image_and_text(icon_image(ui, UiIcon::History), "작업 기록").frame(false)).clicked() {
            self.logs_open = true;
        }
        if ui.add_sized([ui.available_width(), 36.0], egui::Button::image_and_text(icon_image(ui, UiIcon::Settings), "설정").frame(false)).clicked() {
            self.settings_open = true;
        }
        let bottom = ui.max_rect().bottom();
        ui.add_space((bottom - ui.cursor().top() - 124.0).max(8.0));
        egui::Frame::new().fill(secondary(ui)).corner_radius(6).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.0, if self.running { success(ui) } else { muted(ui) });
                ui.label(egui::RichText::new(if self.settings.host { "호스트 모드" } else { "작업 PC 모드" }).size(12.0));
            });
            caption(ui, format!("엔진 {}", self.engine_updater.local.as_deref().unwrap_or("확인 중")));
        });
        ui.add_space(8.0);
        caption(ui, format!("Render Farm  /  v{}", env!("CARGO_PKG_VERSION")));
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
        ui.painter().text(rect.min + egui::vec2(0.0, 35.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(23.0), ui.visuals().text_color());
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
