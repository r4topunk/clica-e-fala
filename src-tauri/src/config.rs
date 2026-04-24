use std::fs;
use std::path::PathBuf;
use std::process::Command;

const APP_DIR: &str = "ClicaEFala";
const CONFIG_FILE: &str = "config.env";

pub fn config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    home.join("Library")
        .join("Application Support")
        .join(APP_DIR)
        .join(CONFIG_FILE)
}

fn read_key_from_file(path: &std::path::Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("GROQ_API_KEY=") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn write_key(key: &str) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = format!("GROQ_API_KEY={}\n", key);
    fs::write(&path, body)?;
    Ok(())
}

fn prompt_key_dialog() -> Option<String> {
    let script = r#"display dialog "Cole sua Groq API key:

Pegue em https://console.groq.com/keys" default answer "" with title "Clica e Fala — Setup" buttons {"OK"} default button "OK""#;
    let out = Command::new("osascript").args(["-e", script]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for part in stdout.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("text returned:") {
            let key = rest.trim().to_string();
            if !key.is_empty() {
                return Some(key);
            }
        }
    }
    None
}

fn show_error_dialog(msg: &str) {
    let script = format!(
        r#"display dialog "{}" with title "Clica e Fala" buttons {{"OK"}} default button "OK" with icon stop"#,
        msg.replace('"', "'")
    );
    let _ = Command::new("osascript").args(["-e", &script]).output();
}

pub fn ensure_groq_key() {
    if std::env::var("GROQ_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        return;
    }

    let path = config_path();
    if let Some(key) = read_key_from_file(&path) {
        std::env::set_var("GROQ_API_KEY", &key);
        eprintln!("[init] loaded GROQ_API_KEY from {:?}", path);
        return;
    }

    match prompt_key_dialog() {
        Some(key) => {
            if let Err(e) = write_key(&key) {
                eprintln!("[init] failed to persist config: {:?}", e);
                show_error_dialog(&format!("Falha ao salvar config: {}", e));
                std::process::exit(1);
            }
            std::env::set_var("GROQ_API_KEY", &key);
            eprintln!("[init] saved GROQ_API_KEY to {:?}", path);
        }
        None => {
            show_error_dialog(
                "Sem Groq API key o app não inicia. Feche e reabra pra tentar de novo.",
            );
            std::process::exit(1);
        }
    }
}

pub fn reset_key() {
    let path = config_path();
    let _ = fs::remove_file(&path);
    std::env::remove_var("GROQ_API_KEY");
    ensure_groq_key();
}
