mod analyze;
mod daemon;
mod db;
mod desktop;
mod idle;
mod ipc;
mod render;
mod tracker;

use crate::db::Database;
use crate::ipc::IpcRequest;
use crate::render::format_duration;
use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("zxtracker")
}

fn db_path() -> PathBuf {
    data_dir().join("zxtracker.db")
}

fn socket_path() -> String {
    "/tmp/zxtracker.sock".to_string()
}

#[derive(Parser)]
#[command(name = "zxtracker", version = "0.3.0", about = "Window focus time tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Daemon,
    Status,
    Today,
    Report {
        #[arg(short, long, default_value_t = false)]
        month: bool,
        #[arg(short, long, default_value_t = false)]
        week: bool,
    },
    Analyze {
        /// Number of days to analyze (default: 7)
        #[arg(short = 'd', long = "days", default_value_t = 7, value_name = "N")]
        days: u32,
        /// Alias for --days 7
        #[arg(long = "7d", default_value_t = false, action = clap::ArgAction::SetTrue)]
        seven: bool,
    },
    Tag {
        app_id: String,
        label: String,
    },
    Untag {
        app_id: String,
        label: String,
    },
    Tags,
    Label {
        name: String,
        #[arg(short, long, default_value_t = false)]
        week: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Daemon => {
            let db = db_path();
            #[cfg(target_os = "linux")]
            let endpoint = socket_path();
            #[cfg(target_os = "windows")]
            let endpoint = "zxtracker_pipe".to_string();
            eprintln!("[zxtracker] daemon starting...");
            eprintln!("[zxtracker] db: {}", db.display());
            eprintln!("[zxtracker] endpoint: {}", endpoint);
            daemon::run(db.to_str().unwrap(), &endpoint)
        }
        #[cfg(target_os = "linux")]
        Command::Status => cmd_status(),
        #[cfg(target_os = "windows")]
        Command::Status => {
            eprintln!("status: Windows CLI IPC via named pipe not yet implemented");
            eprintln!("use 'today' / 'analyze' / 'report' instead (they read DB directly)");
            Ok(())
        }
        Command::Today => {
        let db = Database::open(&db_path())?;
        let today = Local::now().format("%Y-%m-%d").to_string();
        print_usage_table("Today", &db.today_usage(&today)?);
        Ok(())
    }
        Command::Report { month, week: _ } => cmd_report(month),
        Command::Analyze { days, seven } => {
            let d = if seven { 7u32 } else { days };
            analyze::run(&Database::open(&db_path())?, d)
        }
        Command::Tag { app_id, label } => {
            let db = Database::open(&db_path())?;
            db.add_tag(&app_id, &label)?;
            println!("tagged '{}' → '{}'", app_id, label);
            Ok(())
        }
        Command::Untag { app_id, label } => {
            let db = Database::open(&db_path())?;
            db.remove_tag(&app_id, &label)?;
            println!("untagged '{}' from '{}'", label, app_id);
            Ok(())
        }
        Command::Tags => {
            let db = Database::open(&db_path())?;
            for (name, apps) in db.list_tags()? {
                println!("[{}]", name);
                for a in apps { println!("  - {}", a); }
            }
            Ok(())
        }
        Command::Label { name, week } => {
            let db = Database::open(&db_path())?;
            let today = Local::now().format("%Y-%m-%d").to_string();
            if week {
                let from = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")?
                    - chrono::Duration::days(6)).format("%Y-%m-%d").to_string();
                print_label_usage(&name, &db.labeled_range_usage(&name, &from, &today)?);
            } else {
                print_label_usage(&name, &db.labeled_usage(&name, &today)?);
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn cmd_status() -> Result<()> {
    let quit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    ctrlc::set_handler({
        let q = quit.clone();
        move || q.store(true, Ordering::SeqCst)
    })?;

    let mut stdout = std::io::stdout();
    render::init_screen(&mut stdout)?;

    while !quit.load(Ordering::SeqCst) {
        match ipc_send("status", &serde_json::json!({})) {
            Ok(data) => {
                let app_id = data["app_id"].as_str().unwrap_or("");
                let title = data["title"].as_str().unwrap_or("");
                let is_idle = data["is_idle"].as_bool().unwrap_or(false);
                let usage: Vec<(String, i64)> = data["today_usage"]
                    .as_array().map(|a| a.iter().filter_map(|v| {
                        Some((v["app_id"].as_str()?.to_string(), v["seconds"].as_i64()?))
                    }).collect()).unwrap_or_default();
                render::render_dashboard(&mut stdout, title, app_id, is_idle, &usage)?;
            }
            Err(_) => {
                let sock = socket_path();
                write!(stdout, "\x1b[H\x1b[Jdaemon not running ({} not found)\x1b[K", sock)?;
                stdout.flush()?;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    render::restore_screen(&mut stdout)?;
    Ok(())
}


fn cmd_report(month: bool) -> Result<()> {
    let db = Database::open(&db_path())?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let (from, label) = if month {
        ((chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")?
            - chrono::Duration::days(29)).format("%Y-%m-%d").to_string(), "Month")
    } else {
        ((chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")?
            - chrono::Duration::days(6)).format("%Y-%m-%d").to_string(), "Week")
    };
    print_usage_table(label, &db.range_usage(&from, &today)?);
    Ok(())
}

#[cfg(target_os = "linux")]
fn ipc_send(cmd: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(socket_path())?;
    let req = serde_json::to_string(&IpcRequest { cmd: cmd.to_string(), args: args.clone() })?;
    stream.write_all(req.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let resp: crate::ipc::IpcResponse = serde_json::from_str(&line)?;
    if resp.ok { Ok(resp.data.unwrap_or(serde_json::json!({}))) } else { anyhow::bail!(resp.error) }
}

fn print_usage_table(label: &str, usage: &[(String, i64, i64)]) {
    println!("{}", label);
    println!("{:-<60}", "");
    println!("{:<32} {:>12} {:>8}", "APP", "TIME", "TIMES");
    println!("{:-<60}", "");
    let mut total = 0i64;
    for (app, secs, ct) in usage {
        println!("  {:<30} {:>12} {:>8}", trun(app, 30), format_duration(*secs), ct);
        total += secs;
    }
    println!("{:-<60}", "");
    println!("  Total: {}", format_duration(total));
}

fn print_label_usage(label: &str, usage: &[(String, i64)]) {
    println!("[{}]", label);
    println!("{:-<50}", "");
    let mut total = 0i64;
    for (app, secs) in usage {
        println!("  {:<30} {}", trun(app, 30), format_duration(*secs));
        total += secs;
    }
    println!("{:-<50}", "");
    println!("  Total: {}", format_duration(total));
}

fn trun(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
