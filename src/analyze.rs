use crate::db::Database;
use crate::desktop::{self, DesktopDB};
use crate::render::format_duration;
use chrono::Local;
use std::collections::HashMap;

pub fn run(db: &Database, days: u32) -> anyhow::Result<()> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let from_date = (chrono::NaiveDate::parse_from_str(&today, "%Y-%m-%d")?
        - chrono::Duration::days(days as i64 - 1))
    .format("%Y-%m-%d")
    .to_string();

    let stats = db.range_usage(&from_date, &today)?;
    let hourly = db.range_hourly_usage(&from_date, &today)?;
    let first_seen = db.range_first_seen(&from_date, &today)?;

    let desktop = DesktopDB::new();
    let app_meta = db.get_all_app_meta().unwrap_or_default();

    let total_secs: i64 = stats.iter().map(|(_, s, _)| s).sum();

    println!();
    println!(
        "  ══════ {}-Day Analysis ({} ~ {}) ══════",
        days, from_date, today
    );
    println!("  Total tracked: {}", format_duration(total_secs));
    println!();

    if stats.is_empty() {
        println!("  No data yet. Keep the daemon running to collect data.");
        return Ok(());
    }

    render_duration(&stats, &desktop, &app_meta);
    render_activations(&stats, &desktop, &app_meta);
    render_heatmap(&hourly, &stats, &desktop, &app_meta);
    render_schedule(&first_seen, &desktop, &app_meta);

    Ok(())
}

fn name(app_id: &str, desktop: &DesktopDB, app_meta: &HashMap<String, String>) -> String {
    desktop::resolve(app_id, "", desktop, app_meta)
}

fn render_duration(
    stats: &[(String, i64, i64)],
    desktop: &DesktopDB,
    app_meta: &HashMap<String, String>,
) {
    println!("  ── Duration ──");
    println!("  {:<32} {:>12} {:>8}", "APP", "TIME", "TIMES");
    println!("  {:-<54}", "");
    for (app, secs, ct) in stats.iter().take(12) {
        let n = trun(&name(app, desktop, app_meta), 32);
        println!("  {:<32} {:>12} {:>8}", n, format_duration(*secs), ct);
    }
    println!();
}

fn render_activations(
    stats: &[(String, i64, i64)],
    desktop: &DesktopDB,
    app_meta: &HashMap<String, String>,
) {
    let mut sorted: Vec<_> = stats.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.2));

    println!("  ── Activations ──");
    println!("  {:<32} {:>8} {:>12}", "APP", "SWITCHES", "TIME");
    println!("  {:-<54}", "");
    for (app, secs, ct) in sorted.iter().take(12) {
        let n = trun(&name(app, desktop, app_meta), 32);
        println!("  {:<32} {:>8} {:>12}", n, ct, format_duration(*secs));
    }
    println!();
}

fn render_heatmap(
    hourly: &[(String, i64, i64)],
    stats: &[(String, i64, i64)],
    desktop: &DesktopDB,
    app_meta: &HashMap<String, String>,
) {
    let top: Vec<&str> = stats.iter().take(8).map(|(a, _, _)| a.as_str()).collect();

    let mut hm: HashMap<(&str, usize), i64> = HashMap::new();
    for (app, hour, secs) in hourly {
        let b = (*hour as usize) / 2;
        if b < 12 {
            *hm.entry((app.as_str(), b)).or_insert(0) += secs;
        }
    }

    println!("  ── Heatmap (2h buckets) ──");
    print!("  {:<32}", "");
    for h in 0..12 {
        print!("{:02}-{:02} ", h * 2, (h + 1) * 2);
    }
    println!();

    for app in &top {
        let n = trun(&name(app, desktop, app_meta), 32);
        print!("  {:<32}", n);
        for b in 0..12 {
            let v = *hm.get(&(app, b)).unwrap_or(&0);
            let ch = if v > 3600 {
                '█'
            } else if v > 1800 {
                '▓'
            } else if v > 600 {
                '▒'
            } else if v > 60 {
                '░'
            } else {
                ' '
            };
            print!(" {}  ", ch);
        }
        println!();
    }

    println!("  █ >1h  ▓ >30m  ▒ >10m  ░ >1m");
    println!();
}

fn render_schedule(
    first_seen: &[(String, i64)],
    desktop: &DesktopDB,
    app_meta: &HashMap<String, String>,
) {
    println!("  ── Average First Seen ──");
    println!("  {:<32} {:>12}", "APP", "FIRST ACTIVE");
    println!("  {:-<46}", "");
    for (app, ts) in first_seen.iter().take(12) {
        let t = if *ts > 0 {
            chrono::DateTime::from_timestamp(*ts, 0)
                .unwrap_or_default()
                .with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        } else {
            "-".to_string()
        };
        let n = trun(&name(app, desktop, app_meta), 32);
        println!("  {:<32} {:>12}", n, t);
    }
    println!();
}

fn trun(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}
