use crate::tray::TrayState;
use anyhow::{anyhow, Result};
use tauri::{AppHandle, Manager, Runtime};

const AURA_LABEL: &str = "aura";

pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let app = app.clone();
    app.clone()
        .run_on_main_thread(move || {
            let Some(w) = app.get_webview_window(AURA_LABEL) else {
                return;
            };
            let _ = w.set_ignore_cursor_events(true);
            #[cfg(target_os = "macos")]
            apply_macos_tweaks(&w);
        })
        .map_err(|e| anyhow!("aura install run_on_main_thread failed: {e}"))?;
    Ok(())
}

pub fn set_state<R: Runtime>(app: &AppHandle<R>, state: TrayState) {
    let app_cloned = app.clone();
    let show = matches!(state, TrayState::Recording);
    let _ = app.run_on_main_thread(move || {
        let Some(w) = app_cloned.get_webview_window(AURA_LABEL) else {
            return;
        };
        if show {
            #[cfg(target_os = "macos")]
            let _ = position_on_active_screen(&w);
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.show();
        } else {
            let _ = w.hide();
        }
    });
}

#[cfg(target_os = "macos")]
fn apply_macos_tweaks<R: Runtime>(w: &tauri::WebviewWindow<R>) {
    use objc2_app_kit::{NSColor, NSWindow, NSWindowCollectionBehavior};

    let Ok(raw) = w.ns_window() else { return };
    if raw.is_null() {
        return;
    }
    let ns: *mut NSWindow = raw as *mut NSWindow;
    unsafe {
        let win = &*ns;
        // NSStatusWindowLevel = 25 (above floating, below pop-up menus)
        win.setLevel(25);
        let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        win.setCollectionBehavior(behavior);
        win.setHasShadow(false);
        win.setOpaque(false);
        let clear = NSColor::clearColor();
        win.setBackgroundColor(Some(&clear));
    }
}

#[cfg(target_os = "macos")]
fn position_on_active_screen<R: Runtime>(w: &tauri::WebviewWindow<R>) -> Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};
    use tauri::{PhysicalPosition, PhysicalSize};

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| anyhow!("aura: must run on main thread"))?;

    let cursor = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);

    let mut target = None;
    for s in screens.iter() {
        let f = s.frame();
        let min_x = f.origin.x;
        let min_y = f.origin.y;
        let max_x = min_x + f.size.width;
        let max_y = min_y + f.size.height;
        if cursor.x >= min_x && cursor.x < max_x && cursor.y >= min_y && cursor.y < max_y {
            target = Some(s.to_owned());
            break;
        }
    }
    let screen = target
        .or_else(|| NSScreen::mainScreen(mtm))
        .ok_or_else(|| anyhow!("aura: no NSScreen available"))?;

    let frame = screen.frame();
    let primary_height = screens
        .iter()
        .next()
        .map(|s| s.frame().size.height)
        .unwrap_or(frame.size.height);

    // Cocoa has bottom-left origin in global coords. Tauri uses top-left,
    // so top = primary_height - (origin.y + height).
    let top_pt = primary_height - (frame.origin.y + frame.size.height);
    let left_pt = frame.origin.x;
    let width_pt = frame.size.width;
    let height_pt = frame.size.height;

    let scale = w.scale_factor().unwrap_or(1.0);
    w.set_position(PhysicalPosition::new(
        (left_pt * scale).round() as i32,
        (top_pt * scale).round() as i32,
    ))?;
    w.set_size(PhysicalSize::new(
        (width_pt * scale).round() as u32,
        (height_pt * scale).round() as u32,
    ))?;
    Ok(())
}
