use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    HBITMAP, HDC, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SW_HIDE, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

#[derive(serde::Deserialize)]
pub(super) struct WelcomeCorner {
    extent: f64,
    segments: [[f64; 6]; 3],
}

#[derive(Clone, Copy, PartialEq, serde::Deserialize)]
pub(super) struct CornerReveal {
    from: [u8; 3],
    x: f64,
    y: f64,
    radius: f64,
}

impl CornerReveal {
    fn color(self, to: [u8; 3], scale: f64, x: i32, y: i32) -> [u8; 3] {
        let distance = (f64::from(x) + 0.5 - self.x * scale).hypot(f64::from(y) + 0.5 - self.y * scale);
        let amount = (self.radius * scale - distance + 0.5).clamp(0.0, 1.0);
        std::array::from_fn(|channel| {
            let start = f64::from(self.from[channel]);
            let end = f64::from(to[channel]);
            (start + (end - start) * amount).round() as u8
        })
    }
}

pub(super) struct CornerMask {
    pub(super) size: i32,
    pub(super) opaque_starts: Vec<i32>,
    coverage: Vec<u8>,
}

impl CornerMask {
    pub(super) fn new(radius: f64, scale: f64, welcome: Option<&WelcomeCorner>) -> Self {
        let extent = welcome.map_or(radius, |curve| curve.extent);
        let size = (extent * scale).ceil() as i32;
        let mut coverage = Vec::with_capacity((size * size) as usize);
        let mut opaque_starts = Vec::with_capacity(size as usize);
        let curve = welcome.map(WelcomeCorner::sample);

        for y in 0..size {
            let boundaries = std::array::from_fn::<_, 32, _>(|step| {
                let y = (f64::from(y) + (step as f64 + 0.5) / 32.0) / scale;
                let x = match &curve {
                    Some(points) => interpolate_x(points, y),
                    None => radius - (radius.powi(2) - (radius - y.min(radius)).powi(2)).sqrt(),
                };
                x * scale
            });
            let mut first_opaque = size;
            for x in 0..size {
                let area = boundaries.iter().map(|left| (f64::from(x + 1) - left).clamp(0.0, 1.0)).sum::<f64>() / 32.0;
                let alpha = (area * 255.0).round() as u8;
                if alpha == 255 {
                    first_opaque = first_opaque.min(x);
                }
                coverage.push(alpha);
            }
            opaque_starts.push(first_opaque);
        }
        Self { size, opaque_starts, coverage }
    }
}

impl WelcomeCorner {
    // Interpolate the exported Figma Béziers at subpixel precision.
    fn sample(&self) -> Vec<[f64; 2]> {
        let mut start = [0.0, self.extent];
        let mut points = vec![start];
        for [a, b, c, d, x, y] in self.segments {
            for step in 1..=64 {
                let t = f64::from(step) / 64.0;
                let u = 1.0 - t;
                let component =
                    |start: f64, first: f64, second: f64, end: f64| u * u * u * start + 3.0 * u * u * t * first + 3.0 * u * t * t * second + t * t * t * end;
                points.push([component(start[0], a, c, x), component(start[1], b, d, y)]);
            }
            start = [x, y];
        }
        points.reverse();
        points
    }
}

fn interpolate_x(points: &[[f64; 2]], y: f64) -> f64 {
    let next = points.partition_point(|point| point[1] < y);
    if next == 0 {
        return points[0][0];
    }
    if next == points.len() {
        return 0.0;
    }
    let [x0, y0] = points[next - 1];
    let [x1, y1] = points[next];
    x0 + (x1 - x0) * (y - y0) / (y1 - y0)
}

pub(super) struct LayeredCorners([HWND; 4]);

impl LayeredCorners {
    pub(super) fn new() -> Self {
        Self(std::array::from_fn(|_| unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                windows_sys::w!("STATIC"),
                null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            )
        }))
    }

    pub(super) fn hide(&self) {
        for handle in self.0.into_iter().filter(|handle| !handle.is_null()) {
            unsafe { ShowWindow(handle, SW_HIDE) };
        }
    }

    pub(super) fn handles(&self) -> [HWND; 4] {
        self.0
    }

    pub(super) fn paint(&self, mask: &CornerMask, color: [u8; 3], reveal: Option<CornerReveal>, bounds: RECT, scale: f64) -> bool {
        if self.0.iter().any(|handle| handle.is_null()) {
            return false;
        }
        let Some(bitmap) = CornerBitmap::new(mask.size) else {
            return false;
        };
        let size = SIZE { cx: mask.size, cy: mask.size };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let width = mask.size as usize;
        let positions = corner_positions(bounds.right - bounds.left, bounds.bottom - bounds.top, mask.size);

        self.0.iter().zip(positions).enumerate().all(|(corner, (handle, (left, top)))| {
            for y in 0..width {
                for x in 0..width {
                    let column = if corner % 2 == 0 { x } else { width - 1 - x };
                    let row = if corner < 2 { y } else { width - 1 - y };
                    let alpha = u32::from(mask.coverage[column + row * width]);
                    let pixel = if alpha == 0 || alpha == 255 {
                        0
                    } else {
                        let color = reveal.map_or(color, |reveal| reveal.color(color, scale, left + x as i32, top + y as i32));
                        premultiplied(color, alpha)
                    };
                    unsafe { bitmap.pixels.add(x + y * width).write(pixel) };
                }
            }
            // WebView2 draws the opaque client. Windows composites only this small, premultiplied edge.
            unsafe { UpdateLayeredWindow(*handle, null_mut(), null(), &size, bitmap.dc, &origin, 0, &blend, ULW_ALPHA) != 0 }
        })
    }

    pub(super) fn position(&self, main: HWND, bounds: RECT, size: i32) {
        let positions = corner_positions(bounds.right - bounds.left, bounds.bottom - bounds.top, size);
        for (handle, (x, y)) in self.0.into_iter().zip(positions) {
            unsafe { SetWindowPos(handle, main, bounds.left + x, bounds.top + y, size, size, SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOOWNERZORDER) };
        }
    }
}

fn corner_positions(width: i32, height: i32, size: i32) -> [(i32, i32); 4] {
    [(0, 0), (width - size, 0), (0, height - size), (width - size, height - size)]
}

impl Drop for LayeredCorners {
    fn drop(&mut self) {
        for handle in self.0.into_iter().filter(|handle| !handle.is_null()) {
            unsafe { DestroyWindow(handle) };
        }
    }
}

fn premultiplied([red, green, blue]: [u8; 3], alpha: u32) -> u32 {
    let channel = |value: u8| (u32::from(value) * alpha + 127) / 255;
    alpha << 24 | channel(red) << 16 | channel(green) << 8 | channel(blue)
}

struct CornerBitmap {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HBITMAP,
    pixels: *mut u32,
}

impl CornerBitmap {
    fn new(size: i32) -> Option<Self> {
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        if dc.is_null() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = null_mut();
        let bitmap = unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0) };
        if bitmap.is_null() || pixels.is_null() {
            unsafe {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                DeleteDC(dc);
            }
            return None;
        }
        let previous = unsafe { SelectObject(dc, bitmap) };
        if previous.is_null() {
            unsafe {
                DeleteObject(bitmap);
                DeleteDC(dc);
            }
            return None;
        }
        Some(Self { dc, bitmap, previous, pixels: pixels.cast() })
    }
}

impl Drop for CornerBitmap {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_keeps_fractional_pixels_outside_the_opaque_region() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mask = CornerMask::new(20.0, scale, None);
            let width = mask.size as usize;
            assert_eq!(f64::from(mask.size), 20.0 * scale);
            assert_eq!(mask.coverage[0], 0);
            assert!(mask.coverage[..width].iter().any(|alpha| (1..255).contains(alpha)));
            for (row, &start) in mask.coverage.chunks_exact(width).zip(&mask.opaque_starts) {
                let (edge, opaque) = row.split_at(start as usize);
                assert!(edge.iter().all(|&alpha| alpha < 255));
                assert!(opaque.iter().all(|&alpha| alpha == 255));
            }
        }
    }

    #[test]
    fn layered_pixels_use_premultiplied_bgra() {
        assert_eq!(premultiplied([240, 128, 32], 128), 0x8078_4010);
    }

    #[test]
    fn theme_reveal_blends_only_within_the_scaled_circle() {
        let reveal = CornerReveal { from: [40, 60, 80], x: 20.25, y: 0.25, radius: 5.0 };
        let to = [140, 160, 180];
        assert_eq!(reveal.color(to, 2.0, 51, 0), reveal.from);
        assert_eq!(reveal.color(to, 2.0, 50, 0), [90, 110, 130]);
        assert_eq!(reveal.color(to, 2.0, 49, 0), to);
    }
}
