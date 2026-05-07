#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod aura;
mod config;
mod history;
mod logging;
mod pipeline;
mod review;
mod sound;
mod tray;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

#[derive(Clone, Copy, Debug)]
enum Mode {
    Refined,
    Raw,
}

struct AppState {
    recorder: Mutex<Option<(audio::Recorder, Mode)>>,
    work_dir: PathBuf,
    player: sound::AudioPlayer,
    paste_lock: std::sync::Arc<Mutex<()>>,
}

fn toggle_recording(app: &tauri::AppHandle, new_mode: Mode) {
    let state = app.state::<AppState>();
    let is_recording = state.recorder.lock().unwrap().is_some();

    if is_recording {
        let taken = state.recorder.lock().unwrap().take();
        let Some((recorder, mode)) = taken else { return };
        let player = state.player.clone();
        let paste_lock = state.paste_lock.clone();
        let app_handle = app.clone();

        std::thread::spawn(move || {
            let result =
                run_pipeline(recorder, &player, &paste_lock, mode, &app_handle);
            let _ = tray::set_state(&app_handle, tray::TrayState::Idle);
            aura::set_state(&app_handle, tray::TrayState::Idle);
            if let Err(e) = result {
                logerr!("[pipeline] error: {:?}", e);
            }
        });
    } else {
        let path = state.work_dir.join(format!(
            "rec-{}.wav",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        state.player.play_rec_start();
        match audio::start(path) {
            Ok(r) => {
                *state.recorder.lock().unwrap() = Some((r, new_mode));
                let _ = tray::set_state(app, tray::TrayState::Recording);
                aura::set_state(app, tray::TrayState::Recording);
                logln!("[rec] started (mode={:?})", new_mode);
            }
            Err(e) => {
                logerr!("[rec] failed to start: {:?}", e);
            }
        }
    }
}

fn run_pipeline(
    recorder: audio::Recorder,
    player: &sound::AudioPlayer,
    paste_lock: &std::sync::Arc<Mutex<()>>,
    mode: Mode,
    app: &tauri::AppHandle,
) -> anyhow::Result<()> {
    let wav = recorder.stop()?;
    logln!("[rec] stopped (mode={:?})", mode);

    let _ = tray::set_state(app, tray::TrayState::Processing);
    aura::set_state(app, tray::TrayState::Processing);
    let normalized = pipeline::preprocess(&wav)?;
    logln!("[ffmpeg] normalized");

    let _ = tray::set_state(app, tray::TrayState::Transcribing);
    aura::set_state(app, tray::TrayState::Transcribing);
    let transcript = {
        let _guard = player.start_transcribe_loop();
        logln!("[whisper] starting");
        pipeline::transcribe(&normalized)
    }?;
    logln!("[whisper] transcript: {}", transcript);
    if transcript.is_empty() {
        return Err(anyhow::anyhow!("empty transcript"));
    }

    let (output, auto_enter) = match mode {
        Mode::Raw => {
            logln!("[mode] raw — skipping refine");
            (transcript.clone(), false)
        }
        Mode::Refined => {
            let _ = tray::set_state(app, tray::TrayState::Refining);
            aura::set_state(app, tray::TrayState::Refining);
            let refined = {
                let _guard = player.start_claude_loop();
                logln!("[refine] starting");
                pipeline::refine(&transcript)
            }?;
            logln!("[refine] refined: {}", refined);
            if refined.is_empty() {
                return Err(anyhow::anyhow!("empty refinement"));
            }

            let front = review::capture_frontmost_app();
            logln!("[review] frontmost app: {:?}", front);
            let _ = tray::set_state(app, tray::TrayState::Review);
            aura::set_state(app, tray::TrayState::Review);

            let edited = match review::show_and_wait(
                app,
                review::ReviewPayload {
                    transcript: transcript.clone(),
                    refined: refined.clone(),
                },
            ) {
                Ok(Some(e)) => e,
                Ok(None) => {
                    logln!("[review] cancelled");
                    if let Some(f) = front {
                        let _ = review::activate_app(&f);
                    }
                    return Ok(());
                }
                Err(e) => {
                    logerr!("[review] error: {:?}", e);
                    return Err(e);
                }
            };

            if let Some(f) = &front {
                let _ = review::activate_app(f);
                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_millis(150);
                while std::time::Instant::now() < deadline {
                    if review::frontmost_pid() == Some(f.pid) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }

            let _ = tray::update_last_output(app, &edited.refined);

            let t = edited.transcript.clone();
            let r = edited.refined.clone();
            std::thread::spawn(move || {
                match pipeline::log_and_maybe_consolidate(&t, &r, "groq") {
                    Ok(true) => {
                        logln!("[consolidate] threshold hit, running (bg)");
                        match pipeline::consolidate_profile() {
                            Ok(n) => logln!("[consolidate] appended {} bullets", n),
                            Err(e) => logerr!("[consolidate] error: {:?}", e),
                        }
                    }
                    Ok(false) => {}
                    Err(e) => logerr!("[history] log error: {:?}", e),
                }
            });

            (edited.refined, edited.auto_enter)
        }
    };

    {
        let _paste_guard = paste_lock.lock().unwrap();
        logln!("[paste] acquired lock, pasting (auto_enter={})", auto_enter);
        pipeline::copy_and_paste(&output)?;
        if auto_enter {
            std::thread::sleep(std::time::Duration::from_millis(40));
            pipeline::post_return()?;
        }
    }
    player.play_finish();
    Ok(())
}

fn main() {
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("[init] loaded .env from {:?}", path),
        Err(e) => eprintln!("[init] no .env loaded: {}", e),
    }

    config::ensure_groq_key();

    let work_dir = std::env::temp_dir().join("clica-e-fala");
    std::fs::create_dir_all(&work_dir).ok();

    logln!("[init] work_dir={:?}", work_dir);

    let player = sound::AudioPlayer::new().expect("failed to init audio player");

    let state = AppState {
        recorder: Mutex::new(None),
        work_dir,
        player,
        paste_lock: std::sync::Arc::new(Mutex::new(())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let mode = if shortcut.matches(Modifiers::SHIFT, Code::F5) {
                        Mode::Raw
                    } else {
                        Mode::Refined
                    };
                    let handle = app.clone();
                    std::thread::spawn(move || toggle_recording(&handle, mode));
                })
                .build(),
        )
        .manage(state)
        .manage(review::ReviewSlot::default())
        .invoke_handler(tauri::generate_handler![
            review::review_submit,
            review::review_cancel
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let toggle = MenuItem::with_id(
                app,
                "toggle",
                "Record / Stop  (F5)",
                true,
                None::<&str>,
            )?;
            let reset_key = MenuItem::with_id(
                app,
                "reset_key",
                "Reset API Key…",
                true,
                None::<&str>,
            )?;
            let menu = Menu::with_items(app, &[&toggle, &reset_key, &quit])?;

            let _tray = TrayIconBuilder::with_id(tray::TRAY_ID)
                .menu(&menu)
                .icon(tray::initial_icon()?)
                .icon_as_template(false)
                .tooltip("Clica e Fala — idle")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "toggle" => {
                        let handle = app.clone();
                        std::thread::spawn(move || toggle_recording(&handle, Mode::Refined));
                    }
                    "reset_key" => {
                        std::thread::spawn(|| config::reset_key());
                    }
                    _ => {}
                })
                .build(app)?;

            if let Err(e) = aura::install(app.handle()) {
                logerr!("[aura] install failed: {:?}", e);
            }

            let refined_f5 = Shortcut::new(None, Code::F5);
            let raw_f5 = Shortcut::new(Some(Modifiers::SHIFT), Code::F5);
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            app.global_shortcut().register(refined_f5)?;
            app.global_shortcut().register(raw_f5)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}

