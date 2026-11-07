//! Explicit pulse/release input, foreground binding and F10 cancellation.
use anyhow::{Result, bail};
use movement::{Action, MovementProfile};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StopReason {
    Requested = 1,
    TrackingLost = 2,
    CaptureFailed = 3,
    EmergencyStop = 4,
    Shutdown = 5,
}
impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Requested => "stop requested",
            Self::TrackingLost => "board tracking lost",
            Self::CaptureFailed => "screen capture failed",
            Self::EmergencyStop => "emergency stop (F10/button)",
            Self::Shutdown => "application shutting down",
        })
    }
}
#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicU8>);
impl Cancellation {
    pub fn stop(&self) {
        self.stop_with(StopReason::Requested);
    }
    pub fn stop_with(&self, reason: StopReason) {
        // Keep the initiating cause when later cleanup also requests a stop.
        let _ = self
            .0
            .compare_exchange(0, reason as u8, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn reason(&self) -> Option<StopReason> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(StopReason::Requested),
            2 => Some(StopReason::TrackingLost),
            3 => Some(StopReason::CaptureFailed),
            4 => Some(StopReason::EmergencyStop),
            5 => Some(StopReason::Shutdown),
            _ => None,
        }
    }
    pub fn reset(&self) {
        self.0.store(0, Ordering::SeqCst)
    }
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst) != 0
    }
}
pub fn key(a: Action) -> u16 {
    match a {
        Action::Left => 0x25,
        Action::Right => 0x27,
        Action::Cw => 0x58,
        Action::Ccw => 0x5A,
        Action::Down => 0x28,
        Action::HardDrop => 0x20,
        Action::Hold => 0x43,
    }
}
#[cfg(windows)]
mod platform {
    use super::*;
    use windows_sys::Win32::{
        Foundation::*,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };
    pub fn f10() -> bool {
        unsafe { GetAsyncKeyState(0x79) < 0 }
    }
    pub fn foreground() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }
    pub fn window_at(x: i32, y: i32) -> isize {
        unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) as isize }
    }
    pub fn title(hwnd: isize) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd as HWND, buf.as_mut_ptr(), 512) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
    pub fn exclude_debugger() {
        let title: Vec<u16> = "SilhouetteTetris Debugger\0".encode_utf16().collect();
        unsafe {
            let h = FindWindowW(std::ptr::null(), title.as_ptr());
            if !h.is_null() {
                SetWindowDisplayAffinity(h, 0x11);
            }
        }
    }
    pub fn send(vk: u16, up: bool) -> Result<()> {
        let mut event: INPUT = unsafe { std::mem::zeroed() };
        event.r#type = INPUT_KEYBOARD;
        event.Anonymous.ki = KEYBDINPUT {
            wVk: vk,
            dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
            ..Default::default()
        };
        if unsafe { SendInput(1, &event, std::mem::size_of::<INPUT>() as i32) } != 1 {
            bail!("SendInput rejected (focus/integrity level)")
        }
        Ok(())
    }
}
#[cfg(windows)]
pub use platform::*;
#[cfg(not(windows))]
pub fn f10() -> bool {
    false
}
#[cfg(not(windows))]
pub fn foreground() -> isize {
    0
}
#[cfg(not(windows))]
pub fn window_at(_: i32, _: i32) -> isize {
    0
}
#[cfg(not(windows))]
pub fn title(_: isize) -> String {
    String::new()
}
#[cfg(not(windows))]
pub fn exclude_debugger() {}
#[cfg(not(windows))]
fn send(_: u16, _: bool) -> Result<()> {
    bail!("SendInput requires Windows")
}
pub struct Keyboard {
    pressed: Vec<u16>,
    pub cancel: Cancellation,
    pub target: isize,
}
impl Keyboard {
    pub fn new(cancel: Cancellation, target: isize) -> Self {
        Self {
            pressed: vec![],
            cancel,
            target,
        }
    }
    pub fn check(&self) -> Result<()> {
        if let Some(reason) = self.cancel.reason() {
            bail!("input canceled: {reason}")
        }
        if f10() {
            self.cancel.stop_with(StopReason::EmergencyStop);
            bail!("emergency stop (F10)")
        }
        if self.target == 0 || foreground() != self.target {
            bail!(
                "target window is not foreground (target={}, foreground={})",
                self.target,
                foreground()
            )
        }
        Ok(())
    }
    fn wait(&self, ms: u32) -> Result<()> {
        let end = Instant::now() + Duration::from_millis(ms as u64);
        while Instant::now() < end {
            self.check()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
    pub fn pulse(&mut self, a: Action, p: &MovementProfile) -> Result<()> {
        self.check()?;
        let vk = key(a);
        send(vk, false)?;
        self.pressed.push(vk);
        let result = self.wait(p.pulse_ms);
        let release = send(vk, true);
        if release.is_ok() {
            self.pressed.retain(|&k| k != vk);
        }
        result?;
        release?;
        self.wait(p.gap_ms)
    }
    pub fn release_all(&mut self) {
        for vk in self.pressed.drain(..) {
            let _ = send(vk, true);
        }
    }
}
impl Drop for Keyboard {
    fn drop(&mut self) {
        self.release_all()
    }
}
