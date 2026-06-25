use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct DesktopDB {
    mapping: HashMap<String, String>,
}

impl DesktopDB {
    pub fn new() -> Self {
        let mut db = Self {
            mapping: HashMap::new(),
        };
        db.scan();
        db
    }

    pub fn lookup(&self, app_id: &str) -> Option<String> {
        self.mapping.get(app_id).cloned()
    }

    fn scan(&mut self) {
        let local = dirs::data_local_dir().map(|d| d.join("applications"));

        let mut dirs: Vec<PathBuf> = vec![PathBuf::from("/usr/share/applications")];
        if let Some(local_dir) = local {
            dirs.push(local_dir);
        }

        for dir in &dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_none_or(|e| e != "desktop") {
                        continue;
                    }
                    let stem = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let (wmclass_opt, name) = parse_desktop(&path);
                    let name = match name {
                        Some(n) => n,
                        None => continue,
                    };
                    let keys: Vec<String> = if let Some(ref w) = wmclass_opt {
                        let mut k = vec![w.clone()];
                        if !stem.is_empty() && &stem != w {
                            k.push(stem.clone());
                        }
                        k
                    } else if !stem.is_empty() {
                        vec![stem]
                    } else {
                        continue;
                    };
                    for key in keys {
                        self.mapping.entry(key).or_insert_with(|| name.clone());
                    }
                }
            }
        }
    }
}

pub fn resolve(
    app_id: &str,
    path: &str,
    desktop: &DesktopDB,
    db_meta: &HashMap<String, String>,
) -> String {
    if let Some(name) = db_meta.get(app_id) {
        if !name.is_empty() {
            return name.clone();
        }
    }
    if let Some(name) = desktop.lookup(app_id) {
        return name;
    }
    if !path.is_empty() {
        let name = name_from_path(path);
        if !name.is_empty() && name.to_lowercase() != app_id.to_lowercase() {
            return name;
        }
    }
    friendly_fallback(app_id)
}

pub fn name_from_path(path: &str) -> String {
    let bin = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let bin = bin.strip_suffix(".exe").unwrap_or(&bin);
    let bin = bin.strip_suffix(".bin").unwrap_or(bin);
    if bin.is_empty() || bin == "." {
        return String::new();
    }
    let mut c = bin.chars();
    match c.next() {
        Some(ch) => ch.to_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

pub fn friendly_fallback(app_id: &str) -> String {
    if app_id.contains(' ') || app_id.contains('*') || app_id.contains('|') {
        let first = app_id
            .split([' ', '*', '|', '.'])
            .find(|s| !s.is_empty() && s.len() > 1)
            .unwrap_or(app_id);
        let mut c = first.chars();
        return c.next().unwrap_or('?').to_uppercase().to_string() + c.as_str();
    }
    if app_id.contains('.') {
        let cands: Vec<&str> = app_id.rsplit('.').collect();
        for part in &cands {
            if !matches!(
                *part,
                "Client" | "Studio" | "Desktop" | "Application" | "app" | "App" | ""
            ) {
                let mut c = part.chars();
                return c.next().unwrap_or('?').to_uppercase().to_string() + c.as_str();
            }
        }
        cands.last().unwrap_or(&app_id).to_string()
    } else {
        let mut c = app_id.chars();
        c.next().unwrap_or('?').to_uppercase().to_string() + c.as_str()
    }
}

fn parse_desktop(path: &PathBuf) -> (Option<String>, Option<String>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let mut wmclass = None;
    let mut name = None;
    let mut in_entry = false;

    for line in content.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_entry = true;
            continue;
        }
        if !in_entry {
            continue;
        }
        if line.starts_with('[') {
            break;
        }
        if let Some(v) = line.strip_prefix("StartupWMClass=") {
            wmclass = Some(v.to_string());
        }
        if let Some(v) = line.strip_prefix("Name=") {
            if name.is_none() && !v.is_empty() {
                name = Some(v.to_string());
            }
        }
    }

    if name.is_some() {
        (wmclass, name)
    } else {
        (None, None)
    }
}
