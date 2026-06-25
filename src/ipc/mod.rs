use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IpcResponse {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct SharedState {
    pub app_id: String,
    pub title: String,
    pub is_idle: bool,
    pub today_usage: Vec<(String, i64)>,
    #[allow(dead_code)]
    pub weekly_usage: Vec<(String, i64)>,
    #[allow(dead_code)]
    pub tags: Vec<(String, Vec<String>)>,
}

#[cfg(target_os = "linux")]
pub mod unix;
#[cfg(target_os = "linux")]
pub use unix::serve;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::serve;
