//! Physical desktop pixels. GDI fallback; no claimed DXGI/120 FPS performance.
use anyhow::{Result, bail};
use std::time::Instant;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x + self.width as i32
            && y < self.y + self.height as i32
    }
    pub fn intersection(self, b: Self) -> Option<Self> {
        let x = self.x.max(b.x);
        let y = self.y.max(b.y);
        let r = (self.x + self.width as i32).min(b.x + b.width as i32);
        let d = (self.y + self.height as i32).min(b.y + b.height as i32);
        (r > x && d > y).then_some(Self {
            x,
            y,
            width: (r - x).max(0) as u32,
            height: (d - y).max(0) as u32,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub rect: Rect,
    pub rgba: Vec<u8>,
    pub at: Instant,
    pub sequence: u64,
}
impl Frame {
    pub fn new(rect: Rect, color: [u8; 4]) -> Self {
        Self {
            rect,
            rgba: color.repeat((rect.width * rect.height) as usize),
            at: Instant::now(),
            sequence: 0,
        }
    }
    pub fn pixel(&self, x: i32, y: i32) -> [u8; 3] {
        if !self.rect.contains(x, y) {
            return [0; 3];
        }
        let i =
            (((y - self.rect.y) as u32 * self.rect.width + (x - self.rect.x) as u32) * 4) as usize;
        self.rgba[i..i + 3].try_into().unwrap()
    }
    pub fn set(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if self.rect.contains(x, y) {
            let i = (((y - self.rect.y) as u32 * self.rect.width + (x - self.rect.x) as u32) * 4)
                as usize;
            self.rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    pub fn fill(&mut self, r: Rect, c: [u8; 4]) {
        if let Some(r) = r.intersection(self.rect) {
            for y in r.y..r.y + r.height as i32 {
                for x in r.x..r.x + r.width as i32 {
                    self.set(x, y, c)
                }
            }
        }
    }
    pub fn gray(&self) -> Vec<u8> {
        self.rgba
            .chunks_exact(4)
            .map(|c| ((77 * c[0] as u32 + 150 * c[1] as u32 + 29 * c[2] as u32) >> 8) as u8)
            .collect()
    }
}
#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub rect: Rect,
}
#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::HiDpi::*};
    pub fn dpi_aware() {
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }
    unsafe extern "system" fn enum_monitor(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(h, &mut info) } != 0 {
            let r = info.rcMonitor;
            let monitors = unsafe { &mut *(data as *mut Vec<Monitor>) };
            monitors.push(Monitor {
                name: format!("Monitor {}", monitors.len() + 1),
                rect: Rect {
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left) as u32,
                    height: (r.bottom - r.top) as u32,
                },
            });
        }
        1
    }
    pub fn monitors() -> Result<Vec<Monitor>> {
        let mut v = Vec::new();
        if unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(enum_monitor),
                &mut v as *mut _ as isize,
            )
        } == 0
        {
            bail!("EnumDisplayMonitors failed")
        }
        Ok(v)
    }
    struct Dc(HDC);
    impl Drop for Dc {
        fn drop(&mut self) {
            unsafe {
                DeleteDC(self.0);
            }
        }
    }
    struct Screen(HDC);
    impl Drop for Screen {
        fn drop(&mut self) {
            unsafe {
                ReleaseDC(std::ptr::null_mut(), self.0);
            }
        }
    }
    struct Bitmap(HBITMAP);
    impl Drop for Bitmap {
        fn drop(&mut self) {
            unsafe {
                DeleteObject(self.0);
            }
        }
    }
    pub fn capture(rect: Rect, sequence: u64) -> Result<Frame> {
        if rect.width == 0 || rect.height == 0 || rect.width > 32768 || rect.height > 32768 {
            bail!("invalid capture rectangle")
        }
        unsafe {
            let screen = Screen(GetDC(std::ptr::null_mut()));
            if screen.0.is_null() {
                bail!("GetDC failed")
            }
            let dc = Dc(CreateCompatibleDC(screen.0));
            if dc.0.is_null() {
                bail!("CreateCompatibleDC failed")
            }
            let bmp = Bitmap(CreateCompatibleBitmap(
                screen.0,
                rect.width as i32,
                rect.height as i32,
            ));
            if bmp.0.is_null() {
                bail!("CreateCompatibleBitmap failed")
            }
            let old = SelectObject(dc.0, bmp.0);
            let copied = BitBlt(
                dc.0,
                0,
                0,
                rect.width as i32,
                rect.height as i32,
                screen.0,
                rect.x,
                rect.y,
                SRCCOPY | CAPTUREBLT,
            );
            SelectObject(dc.0, old);
            if copied == 0 {
                bail!("BitBlt failed")
            }
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = rect.width as i32;
            info.bmiHeader.biHeight = -(rect.height as i32);
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut frame = Frame::new(rect, [0, 0, 0, 255]);
            frame.sequence = sequence;
            if GetDIBits(
                dc.0,
                bmp.0,
                0,
                rect.height,
                frame.rgba.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            ) != rect.height as i32
            {
                bail!("GetDIBits failed")
            }
            for c in frame.rgba.chunks_exact_mut(4) {
                c.swap(0, 2);
                c[3] = 255;
            }
            // Mask our debugger in our own buffers, avoiding capture feedback without
            // hiding the debugger from screenshots or accessibility inspection.
            use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};
            let title: Vec<u16> = "SilhouetteTetris Debugger\0".encode_utf16().collect();
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            let mut wr: RECT = std::mem::zeroed();
            if !hwnd.is_null() && GetWindowRect(hwnd, &mut wr) != 0 {
                frame.fill(
                    Rect {
                        x: wr.left,
                        y: wr.top,
                        width: (wr.right - wr.left).max(0) as u32,
                        height: (wr.bottom - wr.top).max(0) as u32,
                    },
                    [0, 0, 0, 255],
                );
            }
            Ok(frame)
        }
    }
}
#[cfg(windows)]
pub use platform::*;
#[cfg(not(windows))]
pub fn dpi_aware() {}
#[cfg(not(windows))]
pub fn monitors() -> Result<Vec<Monitor>> {
    bail!("Windows capture required")
}
#[cfg(not(windows))]
pub fn capture(_: Rect, _: u64) -> Result<Frame> {
    bail!("Windows capture required")
}
