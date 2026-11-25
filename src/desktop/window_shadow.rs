use std::ptr::null_mut;
use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowRect, IsWindowVisible, SW_HIDE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SendMessageW, SetWindowPos,
    ShowWindow, WM_NCACTIVATE, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_THICKFRAME,
};

pub(super) struct WindowShadow {
    handle: HWND,
    active: Option<bool>,
}

impl WindowShadow {
    pub(super) fn new() -> Self {
        // A standard framed window lets DWM render its shadow, including the desktop's visual settings.
        let handle = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                windows_sys::w!("STATIC"),
                std::ptr::null(),
                WS_POPUP | WS_CAPTION | WS_THICKFRAME,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            )
        };
        Self { handle, active: None }
    }

    pub(super) fn sync(&mut self, window: &Window, radius: u32) {
        if self.handle.is_null() {
            return;
        }
        let main = window.hwnd() as HWND;
        if window.is_maximized() || window.is_minimized() || unsafe { IsWindowVisible(main) } == 0 {
            if unsafe { IsWindowVisible(self.handle) } != 0 {
                unsafe { ShowWindow(self.handle, SW_HIDE) };
            }
            self.active = None;
            return;
        }
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(main, &mut bounds) } == 0 {
            return;
        }

        // Keep the shadow caster's corners just inside the visible R20 curve.
        let inset = (f64::from(radius) * window.scale_factor() * (1.0 - std::f64::consts::FRAC_1_SQRT_2)).ceil() as i32 + 1;
        let (width, height) = (bounds.right - bounds.left - inset * 2, bounds.bottom - bounds.top - inset * 2);
        let positioned = unsafe {
            SetWindowPos(self.handle, main, bounds.left + inset, bounds.top + inset, width, height, SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW)
        };
        if positioned == 0 {
            return;
        }

        let active = window.is_focused();
        if self.active != Some(active) {
            // Update DWM's active frame appearance without moving keyboard focus.
            unsafe { SendMessageW(self.handle, WM_NCACTIVATE, usize::from(active), 0) };
            self.active = Some(active);
        }
    }

    pub(super) fn handle(&self) -> HWND {
        self.handle
    }
}

impl Drop for WindowShadow {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { DestroyWindow(self.handle) };
        }
    }
}
