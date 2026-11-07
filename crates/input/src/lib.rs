//! Explicit pulse/release input, foreground binding and F10 cancellation.
use anyhow::{Result, bail};
use movement::{Action, MovementProfile};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
/// Configurable key bindings for different Tetris games. The names are
/// converted to Windows virtual-key codes at startup, not per keypress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    pub left: String,
    pub right: String,
    pub cw: String,
    pub ccw: String,
    pub down: String,
    pub hard_drop: String,
    pub hold: String,
}
impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            left: "Left".into(),
            right: "Right".into(),
            cw: "X".into(),
            ccw: "Z".into(),
            down: "Down".into(),
            hard_drop: "Space".into(),
            hold: "C".into(),
        }
    }
}
/// Common, layout-independent Windows virtual-key names. Unsupported keys
/// are rejected instead of silently sending some other input.
pub fn virtual_key(name: &str) -> Result<u16> {
    let name = name.trim().to_ascii_uppercase().replace([' ', '_', '-'], "");
    let vk = match name.as_str() {
        "LEFT" | "ARROWLEFT" => 0x25,
        "RIGHT" | "ARROWRIGHT" => 0x27,
        "UP" | "ARROWUP" => 0x26,
        "DOWN" | "ARROWDOWN" => 0x28,
        "SPACE" | "SPACEBAR" => 0x20,
        "SHIFT" => 0x10,
        "LSHIFT" | "LEFTSHIFT" => 0xA0,
        "RSHIFT" | "RIGHTSHIFT" => 0xA1,
        "CTRL" | "CONTROL" => 0x11,
        "LCTRL" | "LEFTCTRL" | "LEFTCONTROL" => 0xA2,
        "RCTRL" | "RIGHTCTRL" | "RIGHTCONTROL" => 0xA3,
        "ENTER" | "RETURN" => 0x0D,
        "TAB" => 0x09,
        "BACKSPACE" => 0x08,
        "ESC" | "ESCAPE" => 0x1B,
        "COMMA" => 0xBC,
        "PERIOD" | "DOT" => 0xBE,
        "SLASH" => 0xBF,
        "SEMICOLON" => 0xBA,
        "APOSTROPHE" | "QUOTE" => 0xDE,
        "MINUS" => 0xBD,
        "EQUALS" | "EQUAL" => 0xBB,
        "INSERT" => 0x2D,
        "DELETE" => 0x2E,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        _ => {
            let chars: Vec<char> = name.chars().collect();
            if chars.len() == 1 && chars[0].is_ascii_alphanumeric() {
                chars[0] as u16
            } else if let Some(n) = name.strip_prefix('F').and_then(|s| s.parse::<u16>().ok()) {
                if (1..=12).contains(&n) { 0x70 + n - 1 } else { bail!("unsupported key: {name}") }
            } else {
                bail!("unsupported key: {name}");
            }
        }
    };
    Ok(vk)
}
impl KeyBindings {
    pub fn name(&self, action: Action) -> &str {
        match action {
            Action::Left => &self.left,
            Action::Right => &self.right,
            Action::Cw => &self.cw,
            Action::Ccw => &self.ccw,
            Action::Down => &self.down,
            Action::HardDrop => &self.hard_drop,
            Action::Hold => &self.hold,
        }
    }
    pub fn key(&self, action: Action) -> Result<u16> {
        virtual_key(self.name(action))
    }
    pub fn validate(&self) -> Result<()> {
        let mut used = HashSet::new();
        for a in [Action::Left, Action::Right, Action::Cw, Action::Ccw,
                  Action::Down, Action::HardDrop, Action::Hold] {
            let vk = self.key(a)?;
            if vk == 0x79 { bail!("F10 is reserved for emergency stop"); }
            if !used.insert(vk) { bail!("duplicate control binding: {}", self.name(a)); }
        }
        Ok(())
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
    bindings: KeyBindings,
}
impl Keyboard {
    pub fn new(cancel: Cancellation, target: isize) -> Self {
        Self::with_bindings(cancel, target, KeyBindings::default())
    }
    pub fn with_bindings(cancel: Cancellation, target: isize, bindings: KeyBindings) -> Self {
        Self { pressed: vec![], cancel, target, bindings }
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
        let vk = self.bindings.key(a)?;
        send(vk, false)?;
        self.pressed.push(vk);
        let result = self.wait(p.pulse_ms);
        let release = send(vk, true);
        if release.is_ok() {
            self.pressed.retain(|&k| k != vk);
        }
        result?;
        release?;
        // The locked piece is verified on a fresh frame before another
        // action is sent, so a second post-drop key gap only adds latency.
        if a == Action::HardDrop {
            Ok(())
        } else {
            self.wait(p.gap_ms)
        }
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
