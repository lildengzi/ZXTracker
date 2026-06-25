use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FocusEvent {
    pub app_id: String,
    pub title: String,
    pub pid: i64,
    pub path: String,
    pub timestamp: i64,
}

#[cfg(target_os = "linux")]
pub mod niri;
#[cfg(target_os = "linux")]
pub use niri::poll_focused_window;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::poll_focused_window;
