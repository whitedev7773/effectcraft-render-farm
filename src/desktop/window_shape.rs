use super::window_corners::{CornerMask, CornerReveal, LayeredCorners, WelcomeCorner};
use super::window_shadow::WindowShadow;
use serde::Deserialize;
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmFlush, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, HRGN, RGN_OR, SetRectRgn, SetWindowRgn};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos, GetWindowRect, IsWindowVisible, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER,
    SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowGeometry {
    corner_radius: u32,
    welcome_corner: WelcomeCorner,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    Rectangular,
    Rounded { width: i32, height: i32, diameter: i32 },
    Antialiased { width: i32, height: i32, scale: u64, welcome: bool },
}

pub(super) struct WindowShape {
    geometry: WindowGeometry,
    applied: Option<Region>,
    shadow: WindowShadow,
    corners: LayeredCorners,
    welcome: bool,
    color: [u8; 3],
    reveal: Option<CornerReveal>,
    mask: Option<((u64, bool), CornerMask)>,
    painted_bounds: Option<(i32, i32)>,
}

impl WindowShape {
    pub(super) fn new(window: &Window) -> Result<Self, serde_json::Error> {
        let geometry: WindowGeometry = serde_json::from_str(include_str!("../../ui/src/window-geometry.json"))?;
        let hwnd = window.hwnd() as HWND;
        // Windows 10 does not recognize these attributes. The window region and WebView clip also work there.
        for (attribute, value) in [(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND as u32), (DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE)] {
            unsafe { DwmSetWindowAttribute(hwnd, attribute as u32, std::ptr::from_ref(&value).cast(), size_of::<u32>() as u32) };
        }
        Ok(Self {
            geometry,
            applied: None,
            shadow: WindowShadow::new(),
            corners: LayeredCorners::new(),
            welcome: true,
            color: [245; 3],
            reveal: None,
            mask: None,
            painted_bounds: None,
        })
    }

    pub(super) fn set_color(&mut self, window: &Window, color: [u8; 3], reveal: Option<CornerReveal>) {
        if self.color != color || self.reveal != reveal {
            self.color = color;
            self.reveal = reveal;
            self.painted_bounds = None;
            self.update_corners(window, true, false);
        }
    }

    pub(super) fn set_welcome(&mut self, window: &Window, welcome: bool) {
        if self.welcome != welcome {
            self.welcome = welcome;
            self.update(window, true);
        }
    }

    pub(super) fn update(&mut self, window: &Window, redraw: bool) {
        self.shadow.sync(window, self.geometry.corner_radius);
        self.update_corners(window, redraw, true);
    }

    fn update_corners(&mut self, window: &Window, redraw: bool, reposition: bool) {
        if window.is_minimized() {
            self.corners.hide();
            return;
        }
        let hwnd = window.hwnd() as HWND;
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut bounds) } == 0 {
            return;
        }
        if window.is_maximized() || self.geometry.corner_radius == 0 {
            self.corners.hide();
            self.apply_region(hwnd, Region::Rectangular, redraw);
            return;
        }

        let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        let scale = window.scale_factor();
        let key = (scale.to_bits(), self.welcome);
        let radius = f64::from(self.geometry.corner_radius);
        if self.mask.as_ref().is_none_or(|(previous, _)| *previous != key) {
            let welcome = self.welcome.then_some(&self.geometry.welcome_corner);
            self.mask = Some((key, CornerMask::new(radius, scale, welcome)));
            self.painted_bounds = None;
        }
        let Some((_, mask)) = &self.mask else { return };
        let size = mask.size;
        if self.painted_bounds.is_none() || (self.reveal.is_some() && self.painted_bounds != Some((width, height))) {
            self.painted_bounds = self.corners.paint(mask, self.color, self.reveal, bounds, scale).then_some((width, height));
        }
        let region = Region::Antialiased { width, height, scale: key.0, welcome: key.1 };
        let reposition = reposition || self.applied != Some(region);
        if self.painted_bounds.is_some() && self.apply_region(hwnd, region, redraw) {
            if reposition && unsafe { IsWindowVisible(hwnd) } != 0 {
                self.corners.position(hwnd, bounds, size);
            }
        } else {
            self.corners.hide();
            let diameter = (2.0 * radius * scale).round() as i32;
            self.apply_region(hwnd, Region::Rounded { width, height, diameter }, redraw);
        }
    }

    pub(super) fn hide_for_close(&self, window: &Window) {
        let [top_left, top_right, bottom_left, bottom_right] = self.corners.handles();
        let handles = [window.hwnd() as HWND, self.shadow.handle(), top_left, top_right, bottom_left, bottom_right];
        let handles = || handles.into_iter().filter(|handle| !handle.is_null());
        let flags = SWP_HIDEWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER;
        let disabled = 1_u32;
        for handle in handles() {
            unsafe { DwmSetWindowAttribute(handle, DWMWA_TRANSITIONS_FORCEDISABLED as u32, std::ptr::from_ref(&disabled).cast(), size_of::<u32>() as u32) };
        }

        // Present the main window, shadow, and edge removal in one desktop update before dropping their handles.
        let mut batch = unsafe { BeginDeferWindowPos(handles().count() as i32) };
        for handle in handles() {
            if batch.is_null() {
                break;
            }
            batch = unsafe { DeferWindowPos(batch, handle, std::ptr::null_mut(), 0, 0, 0, 0, flags) };
        }
        if batch.is_null() || unsafe { EndDeferWindowPos(batch) } == 0 {
            for handle in handles() {
                unsafe { SetWindowPos(handle, std::ptr::null_mut(), 0, 0, 0, 0, flags) };
            }
        }
        unsafe { DwmFlush() };
    }

    fn apply_region(&mut self, hwnd: HWND, region: Region, redraw: bool) -> bool {
        if self.applied == Some(region) {
            return true;
        }
        let handle = match region {
            Region::Rectangular => std::ptr::null_mut(),
            Region::Rounded { width, height, diameter } => {
                // Leave one physical pixel for the WebView's antialiased edge.
                unsafe { CreateRoundRectRgn(-1, -1, width + 1, height + 1, diameter + 2, diameter + 2) }
            }
            Region::Antialiased { width, height, .. } => {
                let Some((_, mask)) = &self.mask else { return false };
                opaque_region(width, height, mask)
            }
        };
        if handle.is_null() && region != Region::Rectangular {
            return false;
        }
        if unsafe { SetWindowRgn(hwnd, handle, i32::from(redraw)) } != 0 {
            self.applied = Some(region);
            return true;
        }
        // SetWindowRgn only takes ownership on success.
        if !handle.is_null() {
            unsafe { DeleteObject(handle) };
        }
        false
    }
}

fn opaque_region(width: i32, height: i32, mask: &CornerMask) -> HRGN {
    let region = unsafe { CreateRectRgn(0, mask.size, width, height - mask.size) };
    let strip = unsafe { CreateRectRgn(0, 0, 0, 0) };
    if region.is_null() || strip.is_null() {
        if !region.is_null() {
            unsafe { DeleteObject(region) };
        }
        if !strip.is_null() {
            unsafe { DeleteObject(strip) };
        }
        return std::ptr::null_mut();
    }
    for (row, &left) in (0..mask.size).zip(&mask.opaque_starts) {
        for y in [row, height - 1 - row] {
            unsafe {
                SetRectRgn(strip, left, y, width - left, y + 1);
                CombineRgn(region, region, strip, RGN_OR);
            }
        }
    }
    unsafe { DeleteObject(strip) };
    region
}
