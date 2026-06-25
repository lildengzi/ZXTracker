use crate::tracker::FocusEvent;
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub(crate) struct NiriOutput {
    #[allow(dead_code)]
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

pub fn poll_focused_window() -> Option<FocusEvent> {
    let output = Command::new("niri")
        .args(["msg", "--json", "focused-window"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let w: NiriOutput = serde_json::from_slice(&output.stdout).ok()?;
    let pid = w.pid as i64;
    let resolved = resolve_process_path(pid);
    Some(FocusEvent {
        app_id: w.app_id,
        title: w.title,
        pid,
        path: resolved,
        timestamp: chrono::Utc::now().timestamp(),
    })
}

fn resolve_process_path(pid: i64) -> String {
    let path = format!("/proc/{}/exe", pid);
    std::fs::read_link(&path)
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}
