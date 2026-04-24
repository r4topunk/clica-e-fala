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
        auto_enter: bool,
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
    pub auto_enter: bool,
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
        Ok(ReviewResult::Submit { transcript, refined, auto_enter }) => {
            Ok(Some(Edited { transcript, refined, auto_enter }))
        }
        Ok(ReviewResult::Cancel) => Ok(None),
        Err(_) => Err(anyhow!("review timeout (10 min)")),
    }
}

#[tauri::command]
pub fn review_submit(
    slot: State<'_, ReviewSlot>,
    transcript: String,
    refined: String,
    auto_enter: bool,
) {
    if let Some(tx) = slot.tx.lock().unwrap().take() {
        let _ = tx.send(ReviewResult::Submit { transcript, refined, auto_enter });
    }
}

#[tauri::command]
pub fn review_cancel(slot: State<'_, ReviewSlot>) {
    if let Some(tx) = slot.tx.lock().unwrap().take() {
        let _ = tx.send(ReviewResult::Cancel);
    }
}

#[derive(Clone, Debug)]
pub struct FrontApp {
    pub pid: i32,
    #[allow(dead_code)]
    pub name: String,
}

#[cfg(target_os = "macos")]
pub fn capture_frontmost_app() -> Option<FrontApp> {
    use objc2_app_kit::NSWorkspace;
    let ws = NSWorkspace::sharedWorkspace();
    let app = ws.frontmostApplication()?;
    let pid = app.processIdentifier();
    let name = app
        .localizedName()
        .map(|s| s.to_string())
        .unwrap_or_default();
    Some(FrontApp { pid, name })
}

#[cfg(not(target_os = "macos"))]
pub fn capture_frontmost_app() -> Option<FrontApp> {
    None
}

#[cfg(target_os = "macos")]
pub fn activate_app(front: &FrontApp) -> Result<()> {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(front.pid) {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        Ok(())
    } else {
        Err(anyhow!("no running app with pid {}", front.pid))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn activate_app(_front: &FrontApp) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn frontmost_pid() -> Option<i32> {
    use objc2_app_kit::NSWorkspace;
    let ws = NSWorkspace::sharedWorkspace();
    ws.frontmostApplication().map(|a| a.processIdentifier())
}

#[cfg(not(target_os = "macos"))]
pub fn frontmost_pid() -> Option<i32> {
    None
}
