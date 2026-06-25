use std::io::{self, Write};

pub fn init_screen(stdout: &mut impl Write) -> io::Result<()> {
    write!(stdout, "\x1b[?1049h\x1b[?25l")?;
    stdout.flush()
}

pub fn restore_screen(stdout: &mut impl Write) -> io::Result<()> {
    write!(stdout, "\x1b[?25h\x1b[?1049l")?;
    stdout.flush()
}

pub fn format_duration(total_secs: i64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{:>3}h {:>2}m {:>2}s", h, m, s)
}

pub fn render_dashboard(
    stdout: &mut impl Write,
    current: &str,
    app_id: &str,
    is_idle: bool,
    usage: &[(String, i64)],
) -> io::Result<()> {
    write!(stdout, "\x1b[H")?;

    writeln!(stdout, "ZXTracker — real-time dashboard\x1b[K")?;

    let status = if is_idle { "[IDLE] " } else { "" };
    let app_display = if app_id.is_empty() { "-" } else { app_id };
    writeln!(
        stdout,
        "{}{:<30}  {}\x1b[K",
        status,
        trun(app_display, 28),
        trun(current, 70)
    )?;
    writeln!(stdout, "{:-<80}\x1b[K", "")?;

    for (app, secs) in usage {
        let dur = format_duration(*secs);
        writeln!(stdout, "  {:<30} {}\x1b[K", trun(app, 30), dur)?;
    }

    write!(stdout, "\x1b[J")?;
    stdout.flush()
}

fn trun(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}
