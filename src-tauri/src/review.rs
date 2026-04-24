use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize, Deserialize, Clone)]
pub struct ReviewPayload {
    pub transcript: String,
    pub refined: String,
}

pub enum ReviewResult {
    Submit {
        transcript: String,
        refined: String,
    },
    Cancel,
}

#[derive(Default)]
pub struct ReviewSlot {
    pub tx: Mutex<Option<Sender<ReviewResult>>>,
}

pub struct Edited {
    pub transcript: String,
    pub refined: String,
}

pub fn show_and_wait(app: &AppHandle, payload: ReviewPayload) -> Result<Option<Edited>> {
    let slot = app.state::<ReviewSlot>();
    let (tx, rx) = mpsc::channel::<ReviewResult>();
    *slot.tx.lock().unwrap() = Some(tx);

    let window = app
        .get_webview_window("review")
        .ok_or_else(|| anyhow!("review window not found"))?;

    window.emit("review:populate", payload)?;
    window.show()?;
    window.set_focus()?;

    let result = rx.recv_timeout(Duration::from_secs(600));

    let _ = window.hide();
    *slot.tx.lock().unwrap() = None;

    match result {
        Ok(ReviewResult::Submit { transcript, refined }) => {
            Ok(Some(Edited { transcript, refined }))
        }
        Ok(ReviewResult::Cancel) => Ok(None),
        Err(_) => Err(anyhow!("review timeout (10 min)")),
    }
}

#[tauri::command]
pub fn review_submit(slot: State<'_, ReviewSlot>, transcript: String, refined: String) {
    if let Some(tx) = slot.tx.lock().unwrap().take() {
        let _ = tx.send(ReviewResult::Submit { transcript, refined });
    }
}

#[tauri::command]
pub fn review_cancel(slot: State<'_, ReviewSlot>) {
    if let Some(tx) = slot.tx.lock().unwrap().take() {
        let _ = tx.send(ReviewResult::Cancel);
    }
}

pub fn capture_frontmost_app() -> Option<String> {
    let out = std::process::Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to get name of first application process whose frontmost is true",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn activate_app(name: &str) -> Result<()> {
    let sanitized = name.replace('"', "");
    std::process::Command::new("osascript")
        .args([
            "-e",
            &format!("tell application \"{}\" to activate", sanitized),
        ])
        .status()?;
    Ok(())
}
