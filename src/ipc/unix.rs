use crate::db::Database;
use crate::desktop;
use crate::ipc::{IpcRequest, IpcResponse, SharedState};
use chrono::Local;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub fn serve(
    state: Arc<Mutex<SharedState>>,
    db: Arc<Database>,
    quit: Arc<AtomicBool>,
    socket_path: &str,
) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    let path_owned = socket_path.to_string();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle(&state, &db, &quit, stream),
                Err(_) => break,
            }
        }
        let _ = std::fs::remove_file(&path_owned);
    });

    Ok(())
}

fn handle(state: &Arc<Mutex<SharedState>>, db: &Database, quit: &Arc<AtomicBool>, mut stream: UnixStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }

    let req: IpcRequest = match serde_json::from_str(&line) {
        Ok(r) => r,
        Err(e) => {
            reply(&mut stream, &IpcResponse { ok: false, error: e.to_string(), data: None });
            return;
        }
    };

    let resp = match req.cmd.as_str() {
        "status" => handle_status(state, db),
        "get_today" => handle_get_today(db),
        "get_weekly" => handle_get_weekly(db),
        "get_hourly" => handle_get_hourly(db),
        "get_tags" => handle_get_tags(db),
        "shutdown" => {
            quit.store(true, Ordering::SeqCst);
            IpcResponse { ok: true, error: String::new(), data: None }
        }
        _ => IpcResponse { ok: false, error: format!("unknown cmd: {}", req.cmd), data: None },
    };

    reply(&mut stream, &resp);
}

fn handle_status(state: &Arc<Mutex<SharedState>>, db: &Database) -> IpcResponse {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let hourly = if let Ok(data) = db.range_hourly_usage(&today, &today) {
        let mut buckets = [0i64; 12];
        for (_, hour, secs) in &data {
            let b = (*hour as usize) / 2;
            if b < 12 {
                buckets[b] += secs;
            }
        }
        let max = buckets.iter().max().copied().unwrap_or(1);
        buckets.iter().enumerate().map(|(i, v)| {
            serde_json::json!({"bucket": i, "value": v, "ratio": if max > 0 { *v as f64 / max as f64 } else { 0.0 }})
        }).collect::<Vec<_>>()
    } else {
        vec![]
    };

    let s = state.lock().unwrap();
    let usage: Vec<serde_json::Value> = s.today_usage.iter().map(|(a, t)| {
        let name = desktop::friendly_fallback(a);
        serde_json::json!({"app_id": a, "seconds": t, "display": name})
    }).collect();

    IpcResponse {
        ok: true,
        error: String::new(),
        data: Some(serde_json::json!({
            "app_id": s.app_id,
            "title": s.title,
            "is_idle": s.is_idle,
            "today_usage": usage,
            "hourly": hourly,
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
        - chrono::Duration::days(6))
        .format("%Y-%m-%d").to_string();
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
                if b < 12 {
                    per_app.entry(app.clone()).or_insert([0; 12])[b] += secs;
                }
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

fn reply(stream: &mut UnixStream, resp: &IpcResponse) {
    let json = serde_json::to_string(resp).unwrap_or_else(|_| r#"{"ok":false,"error":"serialize failed"}"#.into());
    let _ = stream.write_all(json.as_bytes());
    let _ = stream.write_all(b"\n");
}
