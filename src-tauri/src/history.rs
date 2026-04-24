use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub ts: String,
    pub transcript: String,
    pub refined: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub consolidate_counter: u32,
}

fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".config/clica-e-fala");
    std::fs::create_dir_all(&p).ok()?;
    Some(p)
}

pub fn history_path() -> Option<PathBuf> {
    Some(config_dir()?.join("history.jsonl"))
}

pub fn profile_path() -> Option<PathBuf> {
    Some(config_dir()?.join("profile.md"))
}

fn state_path() -> Option<PathBuf> {
    Some(config_dir()?.join("state.json"))
}

pub fn append_entry(transcript: &str, refined: &str, model: &str) -> Result<()> {
    let Some(path) = history_path() else {
        return Ok(());
    };
    let entry = Entry {
        ts: chrono::Local::now().to_rfc3339(),
        transcript: transcript.to_string(),
        refined: refined.to_string(),
        model: model.to_string(),
    };
    let line = serde_json::to_string(&entry)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

pub fn read_last_n(n: usize) -> Vec<Entry> {
    let Some(path) = history_path() else {
        return Vec::new();
    };
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut entries: Vec<Entry> = reader
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    let len = entries.len();
    if len > n {
        entries.drain(0..len - n);
    }
    entries
}

fn read_state() -> State {
    let Some(path) = state_path() else {
        return State::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => State::default(),
    }
}

fn write_state(s: &State) -> Result<()> {
    let Some(path) = state_path() else {
        return Ok(());
    };
    std::fs::write(path, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

pub fn increment_counter() -> u32 {
    let mut s = read_state();
    s.consolidate_counter += 1;
    let _ = write_state(&s);
    s.consolidate_counter
}

pub fn reset_counter() {
    let mut s = read_state();
    s.consolidate_counter = 0;
    let _ = write_state(&s);
}

pub fn read_profile() -> Option<String> {
    let path = profile_path()?;
    std::fs::read_to_string(path).ok().and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    })
}

pub fn append_to_profile(section: &str) -> Result<()> {
    let Some(path) = profile_path() else {
        return Ok(());
    };
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(section.as_bytes())?;
    Ok(())
}
