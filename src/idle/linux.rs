use std::process::Command;

pub fn idle_seconds() -> u64 {
    if let Ok(output) = Command::new("xprintidle").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(ms) = stdout.trim().parse::<u64>() {
                return ms / 1000;
            }
        }
    }

    if let Ok(output) = Command::new("dbus-send")
        .args([
            "--print-reply",
            "--dest=org.gnome.ScreenSaver",
            "/org/gnome/ScreenSaver",
            "org.gnome.ScreenSaver.GetSessionIdleTime",
        ])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for part in stdout.split_whitespace().rev() {
                if let Ok(ms) = part.parse::<u64>() {
                    return ms / 1000;
                }
            }
        }
    }

    0
}
