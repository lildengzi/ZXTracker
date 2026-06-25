use crate::db::Database;
use crate::desktop::{self, DesktopDB};
use crate::idle;
use crate::ipc::SharedState;
use chrono::Local;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const IDLE_THRESHOLD_SECS: u64 = 300;
const DEBOUNCE_SECS: i64 = 2;

struct ActiveSession {
    app_id: String,
    #[allow(dead_code)]
    pid: i64,
    #[allow(dead_code)]
    path: String,
    db_id: i64,
}

struct PendingSession {
    app_id: String,
    pid: i64,
    path: String,
    started: i64,
}

fn resolve_display_name(db: &Database, desktop: &DesktopDB, app_id: &str, path: &str) -> String {
    if let Ok(Some((name, _))) = db.get_app_meta(app_id) {
        if !name.is_empty() {
            return name;
        }
    }
    let empty = HashMap::new();
    let name = desktop::resolve(app_id, path, desktop, &empty);
    let _ = db.upsert_app_meta(app_id, &name, "");
    name
}

pub fn run(db_path: &str, socket_path: &str) -> anyhow::Result<()> {
    let quit = Arc::new(AtomicBool::new(false));
    ctrlc::set_handler({
        let q = quit.clone();
        move || q.store(true, Ordering::SeqCst)
    })?;

    let db = Arc::new(Database::open(std::path::Path::new(db_path))?);
    db.close_active_sessions(chrono::Utc::now().timestamp())?;

    let shared = Arc::new(Mutex::new(SharedState {
        app_id: String::new(), title: String::new(), is_idle: false,
        today_usage: Vec::new(), weekly_usage: Vec::new(), tags: Vec::new(),
    }));

    crate::ipc::serve(shared.clone(), db.clone(), socket_path)?;

    let desktop_db = DesktopDB::new();
    let app_meta = db.get_all_app_meta().unwrap_or_default();

    let mut pending: Option<PendingSession> = None;
    let mut active: Option<ActiveSession> = None;
    let mut is_idle = false;
    let mut today_seconds: HashMap<String, i64> = load_today(&db);
    let mut last_app_pid: Option<(String, i64)> = None;

    while !quit.load(Ordering::SeqCst) {
        let now = chrono::Utc::now().timestamp();
        let idle_secs = idle::idle_seconds();
        let was_idle = is_idle;
        is_idle = idle_secs >= IDLE_THRESHOLD_SECS;

        if is_idle && !was_idle {
            commit(&db, &desktop_db, &mut pending, &mut active, now);
            today_seconds = load_today(&db);
            last_app_pid = None;
        }
        if !is_idle && was_idle {
            pending = None;
            active = None;
            last_app_pid = None;
        }
        if is_idle {
            update_shared(&shared, &desktop_db, &app_meta, today_seconds.clone(), "", true);
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }

        if let Some(event) = crate::tracker::poll_focused_window() {
            let new_key = (event.app_id.clone(), event.pid);
            if Some(&new_key) != last_app_pid.as_ref() {
                last_app_pid = Some(new_key.clone());
                commit(&db, &desktop_db, &mut pending, &mut active, now);
                pending = Some(PendingSession {
                    app_id: event.app_id.clone(),
                    pid: event.pid,
                    path: event.path.clone(),
                    started: now,
                });
            }
            today_seconds = load_today(&db);
        }

        if let Some(ref p) = &pending {
            if now - p.started >= DEBOUNCE_SECS {
                resolve_display_name(&db, &desktop_db, &p.app_id, &p.path);
                match db.start_session(&p.app_id, p.pid, &p.path, p.started) {
                    Ok(db_id) => {
                        active = Some(ActiveSession {
                            app_id: p.app_id.clone(), pid: p.pid,
                            path: p.path.clone(), db_id,
                        });
                        pending = None;
                    }
                    Err(e) => eprintln!("[zxtracker] session insert error: {}", e),
                }
            }
        }

        if let Some(ref a) = &active {
            *today_seconds.entry(a.app_id.clone()).or_insert(0) += 1;
        }

        let label = active.as_ref().map(|a| a.app_id.as_str()).unwrap_or("");
        update_shared(&shared, &desktop_db, &app_meta, today_seconds.clone(), label, is_idle);

        std::thread::sleep(Duration::from_secs(1));
    }

    commit(&db, &desktop_db, &mut pending, &mut active, chrono::Utc::now().timestamp());
    eprintln!("[zxtracker] daemon stopped");
    Ok(())
}

fn commit(db: &Database, desktop: &DesktopDB, pending: &mut Option<PendingSession>, active: &mut Option<ActiveSession>, now: i64) {
    if let Some(ref p) = pending.take() {
        let duration = now - p.started;
        if duration >= DEBOUNCE_SECS {
            resolve_display_name(db, desktop, &p.app_id, &p.path);
            if let Ok(id) = db.start_session(&p.app_id, p.pid, &p.path, p.started) {
                if let Err(e) = db.end_session(id, now) {
                    eprintln!("[zxtracker] db end error: {}", e);
                }
            }
        }
    }
    if let Some(ref a) = active.take() {
        if let Err(e) = db.end_session(a.db_id, now) {
            eprintln!("[zxtracker] db end error: {}", e);
        }
    }
}

fn load_today(db: &Database) -> HashMap<String, i64> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    db.today_usage(&today).unwrap_or_default()
        .into_iter().map(|(app, secs, _)| (app, secs)).collect()
}

fn update_shared(
    shared: &Arc<Mutex<SharedState>>,
    desktop: &DesktopDB,
    app_meta: &HashMap<String, String>,
    today_seconds: HashMap<String, i64>,
    label: &str,
    is_idle: bool,
) {
    let mut sorted: Vec<(String, i64)> = today_seconds.iter().map(|(k, v)| (k.clone(), *v)).collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
    let display = if label.is_empty() { "[idle]".into() } else { desktop::resolve(label, "", desktop, app_meta) };
    if let Ok(mut s) = shared.lock() {
        s.app_id = label.to_string();
        s.title = display;
        s.is_idle = is_idle;
        s.today_usage = sorted;
    }
}
