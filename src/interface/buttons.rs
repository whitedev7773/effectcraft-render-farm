use super::*;

#[derive(Clone, Copy)]
pub(super) enum ButtonKind {
    Primary,
    Normal,
    Quiet,
    Destructive,
}

#[derive(Clone, Copy)]
pub(super) struct ButtonSpec<'a> {
    pub label: &'a str,
    pub enabled: bool,
    pub kind: ButtonKind,
    pub min_width: f32,
}

impl<'a> ButtonSpec<'a> {
    pub fn new(label: &'a str, kind: ButtonKind, enabled: bool) -> Self {
        Self { label, enabled, kind, min_width: 96.0 }
    }

    fn width(self, ui: &egui::Ui) -> f32 {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let text = ui.painter().layout_no_wrap(self.label.into(), font, egui::Color32::WHITE);
        (text.size().x + 24.0).ceil().max(self.min_width)
    }
}

pub(super) fn button_at(ui: &mut egui::Ui, rect: egui::Rect, id: &str, spec: ButtonSpec<'_>) -> egui::Response {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    if rect.height() < layout::CONTROL {
        child.spacing_mut().button_padding = egui::vec2(4.0, 4.0);
    }
    if !spec.enabled || !ui.is_enabled() {
        child.disable();
    }
    let button = match spec.kind {
        ButtonKind::Primary => primary_button(spec.label),
        ButtonKind::Normal => egui::Button::new(spec.label),
        ButtonKind::Quiet => egui::Button::new(spec.label).frame(false),
        ButtonKind::Destructive => egui::Button::new(egui::RichText::new(spec.label).color(egui::Color32::LIGHT_RED)).frame(false),
    };
    child.add_sized(rect.size(), button)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FooterGeometry {
    pub leading: Option<egui::Rect>,
    pub primary: Option<egui::Rect>,
    pub dismiss: Option<egui::Rect>,
    pub free_left: egui::Rect,
}

fn footer_geometry(rect: egui::Rect, widths: [Option<f32>; 3], height: f32, wrap: bool) -> FooterGeometry {
    let dismiss = widths[2].map(|width| egui::Rect::from_min_size(egui::pos2(rect.right() - width, rect.bottom() - height), egui::vec2(width, height)));
    let primary_right = dismiss.map_or(rect.right(), |button| button.left() - layout::SPACE);
    let primary = widths[1].map(|width| egui::Rect::from_min_size(egui::pos2(primary_right - width, rect.bottom() - height), egui::vec2(width, height)));
    let leading = widths[0].map(|width| egui::Rect::from_min_size(rect.min, egui::vec2(width, height)));
    let free_right = primary.or(dismiss).map_or(rect.right(), |button| button.left() - layout::SPACE);
    let free_left = if wrap {
        egui::Rect::NOTHING
    } else {
        egui::Rect::from_min_max(
            egui::pos2(leading.map_or(rect.left(), |button| button.right() + layout::SPACE), rect.top()),
            egui::pos2(free_right, rect.bottom()),
        )
    };
    FooterGeometry { leading, primary, dismiss, free_left }
}

#[derive(Default)]
pub(super) struct FooterClicks {
    pub leading: bool,
    pub primary: bool,
    pub dismiss: bool,
    pub free_left: Option<egui::Rect>,
}

/// Windows order: the commit action precedes Cancel; the group is right-aligned.
pub(super) fn footer_buttons(
    ui: &mut egui::Ui,
    leading: Option<ButtonSpec<'_>>,
    primary: Option<ButtonSpec<'_>>,
    dismiss: Option<ButtonSpec<'_>>,
    height: f32,
) -> FooterClicks {
    let widths = [leading.map(|button| button.width(ui)), primary.map(|button| button.width(ui)), dismiss.map(|button| button.width(ui))];
    let trailing_width = widths[1].unwrap_or(0.0) + widths[2].unwrap_or(0.0) + if primary.is_some() && dismiss.is_some() { layout::SPACE } else { 0.0 };
    let wrap = leading.is_some() && widths[0].unwrap_or(0.0) + layout::SPACE + trailing_width > ui.available_width();
    let row_height = if wrap { height * 2.0 + layout::SPACE } else { height };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_height), egui::Sense::hover());
    let geometry = footer_geometry(rect, widths, height, wrap);
    let mut result = FooterClicks { free_left: geometry.free_left.is_positive().then_some(geometry.free_left), ..Default::default() };
    if let (Some(rect), Some(spec)) = (geometry.leading, leading) {
        result.leading = button_at(ui, rect, "leading-action", spec).clicked();
    }
    if let (Some(rect), Some(spec)) = (geometry.primary, primary) {
        result.primary = button_at(ui, rect, "primary-action", spec).clicked();
    }
    if let (Some(rect), Some(spec)) = (geometry.dismiss, dismiss) {
        result.dismiss = button_at(ui, rect, "dismiss-action", spec).clicked();
    }
    result
}

pub(super) fn dialog_header(ui: &mut egui::Ui, title: &str, close_label: &str) -> bool {
    ui.horizontal(|ui| {
        let width = (ui.available_width() - layout::CONTROL - layout::SPACE).max(1.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, layout::CONTROL), egui::Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child.add(egui::Label::new(egui::RichText::new(title).size(18.0).strong()).truncate()).on_hover_text(title);
        icon_button(ui, UiIcon::Close, close_label).clicked()
    })
    .inner
}

pub(super) fn footer_status(ui: &mut egui::Ui, rect: egui::Rect, text: &str, busy: bool) {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt("footer-status").max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    if busy {
        child.add_sized([16.0, 16.0], egui::Spinner::new());
    }
    child.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(MUTED)).truncate()).on_hover_text(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_commands_keep_windows_order_and_right_edge() {
        for width in [280.0, 460.0, 650.0] {
            let rect = egui::Rect::from_min_size(egui::pos2(16.0, 24.0), egui::vec2(width, layout::CONTROL));
            let geometry = footer_geometry(rect, [None, Some(128.0), Some(96.0)], layout::CONTROL, false);
            let primary = geometry.primary.unwrap();
            let cancel = geometry.dismiss.unwrap();
            assert_eq!(cancel.right(), rect.right());
            assert_eq!(primary.right() + layout::SPACE, cancel.left());
            assert_eq!(primary.top(), cancel.top());
            assert_eq!(primary.height(), layout::CONTROL);
            assert!(geometry.free_left.right() < primary.left());
        }
    }

    #[test]
    fn removal_is_separate_and_compact_footers_can_wrap() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, layout::CONTROL));
        let geometry = footer_geometry(rect, [Some(104.0), Some(128.0), Some(96.0)], layout::CONTROL, false);
        assert_eq!(geometry.leading.unwrap().left(), rect.left());
        assert!(geometry.leading.unwrap().right() + layout::SPACE < geometry.primary.unwrap().left());
        let compact = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(280.0, layout::CONTROL * 2.0 + layout::SPACE));
        let geometry = footer_geometry(compact, [Some(104.0), Some(128.0), Some(96.0)], layout::CONTROL, true);
        assert!(geometry.leading.unwrap().bottom() < geometry.primary.unwrap().top());
        assert_eq!(geometry.dismiss.unwrap().right(), compact.right());
    }

    #[test]
    fn footer_hit_targets_commit_only_the_enabled_primary_action() {
        let ctx = egui::Context::default();
        let point = egui::pos2(292.0, 16.0);
        let render = |enabled, events| {
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(460.0, 200.0))), events, ..Default::default() };
            let mut clicks = FooterClicks::default();
            ctx.run_ui(input, |root| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(root, |ui| {
                    let mut primary = ButtonSpec::new("설치", ButtonKind::Primary, enabled);
                    primary.min_width = 128.0;
                    clicks = footer_buttons(ui, None, Some(primary), Some(ButtonSpec::new("취소", ButtonKind::Normal, true)), layout::CONTROL);
                });
            })
            .drop_without_applying_deltas();
            clicks
        };
        render(false, Vec::new());
        for enabled in [false, true] {
            // Present the new enabled state before sending a pointer press.
            render(enabled, Vec::new());
            render(
                enabled,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
                ],
            );
            let clicks = render(
                enabled,
                vec![egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }],
            );
            assert_eq!(clicks.primary, enabled);
            assert!(!clicks.dismiss && !clicks.leading);
        }
    }
}
