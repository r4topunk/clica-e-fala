use anyhow::{anyhow, Result};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    AppHandle, Runtime,
};

pub const TRAY_ID: &str = "main-tray";

const ICON_IDLE: &[u8] = include_bytes!("../icons/tray_idle.png");
const ICON_RECORDING: &[u8] = include_bytes!("../icons/tray_recording.png");
const ICON_TRANSCRIBING: &[u8] = include_bytes!("../icons/tray_transcribing.png");
const ICON_REFINING: &[u8] = include_bytes!("../icons/tray_refining.png");
const ICON_REVIEW: &[u8] = include_bytes!("../icons/tray_review.png");

#[derive(Clone, Copy, Debug)]
pub enum TrayState {
    Idle,
    Recording,
    Transcribing,
    Refining,
    Review,
}

impl TrayState {
    fn icon_bytes(self) -> &'static [u8] {
        match self {
            TrayState::Idle => ICON_IDLE,
            TrayState::Recording => ICON_RECORDING,
            TrayState::Transcribing => ICON_TRANSCRIBING,
            TrayState::Refining => ICON_REFINING,
            TrayState::Review => ICON_REVIEW,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            TrayState::Idle => "Clica e Fala — idle",
            TrayState::Recording => "Recording…",
            TrayState::Transcribing => "Transcribing…",
            TrayState::Refining => "Refining…",
            TrayState::Review => "Review — edit & ⏎ to paste",
        }
    }
}

pub fn set_state<R: Runtime>(app: &AppHandle<R>, state: TrayState) -> Result<()> {
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| anyhow!("tray not found"))?;
    let img = Image::from_bytes(state.icon_bytes())?;
    tray.set_icon(Some(img))?;
    tray.set_icon_as_template(false)?;
    tray.set_tooltip(Some(state.tooltip()))?;
    Ok(())
}

pub fn update_last_output<R: Runtime>(app: &AppHandle<R>, text: &str) -> Result<()> {
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| anyhow!("tray not found"))?;
    let truncated = truncate(text, 60);
    let label = format!("Last: {}", truncated);

    let toggle = MenuItem::with_id(app, "toggle", "Record / Stop  (⌘⇧Space)", true, None::<&str>)?;
    let last = MenuItem::with_id(app, "last", label, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &last, &quit])?;
    tray.set_menu(Some(menu))?;
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    let cleaned: String = s.chars().filter(|c| !c.is_control()).collect();
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let end: String = cleaned.chars().take(max).collect();
        format!("{}…", end)
    }
}

pub fn initial_icon() -> Result<Image<'static>> {
    Ok(Image::from_bytes(ICON_IDLE)?)
}
