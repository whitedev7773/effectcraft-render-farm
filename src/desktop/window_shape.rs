use serde::Deserialize;
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowGeometry {
    corner_radius: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    Rectangular,
    Rounded { width: i32, height: i32, diameter: i32 },
}

pub(super) struct WindowShape {
    corner_radius: u32,
    applied: Option<Region>,
}

impl WindowShape {
    pub(super) fn new(window: &Window) -> Result<Self, serde_json::Error> {
        let geometry: WindowGeometry = serde_json::from_str(include_str!("../../ui/src/window-geometry.json"))?;
        let hwnd = window.hwnd() as HWND;
        // Windows 10 does not recognize these attributes. The window region and WebView clip also work there.
        for (attribute, value) in [(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND as u32), (DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE)] {
            unsafe { DwmSetWindowAttribute(hwnd, attribute as u32, std::ptr::from_ref(&value).cast(), size_of::<u32>() as u32) };
        }
        Ok(Self { corner_radius: geometry.corner_radius, applied: None })
    }

    pub(super) fn update(&mut self, window: &Window, redraw: bool) {
        if window.is_minimized() {
            return;
        }
        let outer = window.outer_size();
        let (width, height) = (outer.width as i32, outer.height as i32);
        if width <= 0 || height <= 0 {
            return;
        }
        let region = if window.is_maximized() || self.corner_radius == 0 {
            Region::Rectangular
        } else {
            let diameter = (2.0 * f64::from(self.corner_radius) * window.scale_factor()).round() as i32;
            Region::Rounded { width, height, diameter }
        };
        if self.applied == Some(region) {
            return;
        }

        if apply_region(window, region, redraw) {
            self.applied = Some(region);
        }
    }
}

fn apply_region(window: &Window, region: Region, redraw: bool) -> bool {
    let handle = match region {
        Region::Rectangular => std::ptr::null_mut(),
        Region::Rounded { width, height, diameter } => {
            // Leave one physical pixel for the WebView's antialiased edge.
            let handle = unsafe { CreateRoundRectRgn(-1, -1, width + 1, height + 1, diameter + 2, diameter + 2) };
            if handle.is_null() {
                return false;
            }
            handle
        }
    };
    if unsafe { SetWindowRgn(window.hwnd() as HWND, handle, i32::from(redraw)) } != 0 {
        return true;
    }
    // SetWindowRgn only takes ownership on success.
    if !handle.is_null() {
        unsafe { DeleteObject(handle) };
    }
    false
}
