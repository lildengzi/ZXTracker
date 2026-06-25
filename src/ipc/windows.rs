use crate::db::Database;
use crate::ipc::{IpcRequest, IpcResponse, SharedState};
use chrono::Local;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeA, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};

pub fn serve(
    state: Arc<Mutex<SharedState>>,
    db: Arc<Database>,
    pipe_name: &str,
) -> anyhow::Result<()> {
    let name = format!("\\\\.\\pipe\\{}", pipe_name);
    let cname = std::ffi::CString::new(name.clone()).unwrap();

    std::thread::spawn(move || loop {
        unsafe {
            let handle = CreateNamedPipeA(
                cname.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                4096, 4096, 0, std::ptr::null(),
            );
            if handle == INVALID_HANDLE_VALUE {
                break;
            }
            if ConnectNamedPipe(handle, std::ptr::null()) == 0 {
                CloseHandle(handle);
                continue;
            }

            let mut buf = vec![0u8; 4096];
            let mut stream = NamedPipeStream { handle };

            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                CloseHandle(handle);
                continue;
            }

            let resp = handle_request(&state, &db, &line);
            let json = serde_json::to_string(&resp).unwrap_or_default();
            let _ = (&mut stream as &mut dyn Write).write_all(json.as_bytes());
            let _ = (&mut stream as &mut dyn Write).write_all(b"\n");

            CloseHandle(handle);
        }
    });

    Ok(())
}

fn handle_request(state: &Arc<Mutex<SharedState>>, db: &Database, line: &str) -> IpcResponse {
    let req: IpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => return IpcResponse { ok: false, error: e.to_string(), data: None },
    };
    match req.cmd.as_str() {
        "status" => handle_status(state, db),
        "get_today" => handle_get_today(db),
        "get_weekly" => handle_get_weekly(db),
        "get_hourly" => handle_get_hourly(db),
        "get_tags" => handle_get_tags(db),
        _ => IpcResponse { ok: false, error: format!("unknown: {}", req.cmd), data: None },
    }
}

fn handle_status(state: &Arc<Mutex<SharedState>>, db: &Database) -> IpcResponse {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let hourly = if let Ok(data) = db.range_hourly_usage(&today, &today) {
        let mut buckets = vec![0i64; 12];
        for (_, hour, secs) in &data {
            let b = (*hour as usize) / 2;
            if b < 12 { buckets[b] += secs; }
        }
        let max = buckets.iter().max().copied().unwrap_or(1);
        buckets.iter().enumerate().map(|(i, v)| {
            serde_json::json!({"bucket": i, "value": v, "ratio": if max > 0 { *v as f64 / max as f64 } else { 0.0 }})
        }).collect::<Vec<_>>()
    } else { vec![] };

    let s = state.lock().unwrap();
    let usage: Vec<serde_json::Value> = s.today_usage.iter().map(|(a, t)| {
        let name = crate::desktop::friendly_fallback(a);
        serde_json::json!({"app_id": a, "seconds": t, "display": name})
    }).collect();

    IpcResponse {
        ok: true, error: String::new(),
        data: Some(serde_json::json!({
            "app_id": s.app_id, "title": s.title, "is_idle": s.is_idle,
            "today_usage": usage, "hourly": hourly,
        })),
    }
}

fn handle_get_today(db: &Database) -> IpcResponse {
    let today = Local::now().format("%Y-%m-%d").to_string();
    match db.today_usage(&today) {
        Ok(usage) => {
            let data: Vec<serde_json::Value> = usage.iter().map(|(a, t, c)| {
                serde_json::json!({"app_id": a, "seconds": t, "count": c})
            }).collect();
            IpcResponse { ok: true, error: String::new(), data: Some(serde_json::json!({"date": today, "usage": data})) }
        }
        Err(e) => IpcResponse { ok: false, error: e.to_string(), data: None },
    }
}

fn handle_get_weekly(db: &Database) -> IpcResponse {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let from = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d").unwrap()
        - chrono::Duration::days(6)).format("%Y-%m-%d").to_string();
    match db.range_usage(&from, &today) {
        Ok(usage) => {
            let data: Vec<serde_json::Value> = usage.iter().map(|(a, t, c)| {
                serde_json::json!({"app_id": a, "seconds": t, "count": c})
            }).collect();
            IpcResponse { ok: true, error: String::new(), data: Some(serde_json::json!({"from": from, "to": today, "usage": data})) }
        }
        Err(e) => IpcResponse { ok: false, error: e.to_string(), data: None },
    }
}

fn handle_get_hourly(db: &Database) -> IpcResponse {
    let today = Local::now().format("%Y-%m-%d").to_string();
    match db.range_hourly_usage(&today, &today) {
        Ok(data) => {
            let mut per_app: std::collections::HashMap<String, [i64; 12]> = std::collections::HashMap::new();
            for (app, hour, secs) in &data {
                let b = (*hour as usize) / 2;
                if b < 12 { per_app.entry(app.clone()).or_insert([0; 12])[b] += secs; }
            }
            let result: Vec<serde_json::Value> = per_app.iter().map(|(app, buckets)| {
                serde_json::json!({"app_id": app, "hourly": buckets.to_vec()})
            }).collect();
            IpcResponse { ok: true, error: String::new(), data: Some(serde_json::json!({"hourly": result})) }
        }
        Err(e) => IpcResponse { ok: false, error: e.to_string(), data: None },
    }
}

fn handle_get_tags(db: &Database) -> IpcResponse {
    match db.list_tags() {
        Ok(tags) => {
            let data: Vec<serde_json::Value> = tags.iter().map(|(name, apps)| {
                serde_json::json!({"name": name, "apps": apps})
            }).collect();
            IpcResponse { ok: true, error: String::new(), data: Some(serde_json::json!({"tags": data})) }
        }
        Err(e) => IpcResponse { ok: false, error: e.to_string(), data: None },
    }
}

struct NamedPipeStream {
    handle: isize,
}

impl Read for NamedPipeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Simple wrapper - read from pipe buffer
        let mut bytes_read: u32 = 0;
        unsafe {
            windows_sys::Win32::Storage::FileSystem::ReadFile(
                self.handle as _,
                buf.as_mut_ptr() as _,
                buf.len() as u32,
                &mut bytes_read,
                std::ptr::null_mut(),
            );
        }
        Ok(bytes_read as usize)
    }
}

impl Write for NamedPipeStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut written: u32 = 0;
        unsafe {
            windows_sys::Win32::Storage::FileSystem::WriteFile(
                self.handle as _,
                buf.as_ptr() as _,
                buf.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            );
        }
        Ok(written as usize)
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

use std::io::Read;
