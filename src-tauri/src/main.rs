#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod aura;
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
    model_path: PathBuf,
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
        let model = state.model_path.clone();
        let player = state.player.clone();
        let paste_lock = state.paste_lock.clone();
        let app_handle = app.clone();

        std::thread::spawn(move || {
            let result =
                run_pipeline(recorder, &model, &player, &paste_lock, mode, &app_handle);
            let _ = tray::set_state(&app_handle, tray::TrayState::Idle);
            aura::set_state(&app_handle, tray::TrayState::Idle);
            if let Err(e) = result {
                logln!("[pipeline] error: {:?}", e);
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
                logln!("[rec] failed to start: {:?}", e);
            }
        }
    }
}

fn run_pipeline(
    recorder: audio::Recorder,
    model: &std::path::Path,
    player: &sound::AudioPlayer,
    paste_lock: &std::sync::Arc<Mutex<()>>,
    mode: Mode,
    app: &tauri::AppHandle,
) -> anyhow::Result<()> {
    let wav = recorder.stop()?;
    logln!("[rec] stopped (mode={:?})", mode);

    let normalized = pipeline::preprocess(&wav)?;
    logln!("[ffmpeg] normalized");

    let _ = tray::set_state(app, tray::TrayState::Transcribing);
    aura::set_state(app, tray::TrayState::Transcribing);
    let transcript = {
        let _guard = player.start_transcribe_loop();
        let provider = if std::env::var("GROQ_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
        {
            "groq"
        } else {
            "local"
        };
        logln!("[whisper] starting ({})", provider);
        pipeline::transcribe(&normalized, model)
    }?;
    logln!("[whisper] transcript: {}", transcript);
    if transcript.is_empty() {
        return Err(anyhow::anyhow!("empty transcript"));
    }

    let output = match mode {
        Mode::Raw => {
            logln!("[mode] raw — skipping refine");
            transcript.clone()
        }
        Mode::Refined => {
            let _ = tray::set_state(app, tray::TrayState::Refining);
            aura::set_state(app, tray::TrayState::Refining);
            let (refined, provider) = {
                let _guard = player.start_claude_loop();
                logln!("[refine] starting");
                pipeline::refine(&transcript)
            }?;
            logln!("[refine] ({}) refined: {}", provider, refined);
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
                    logln!("[review] error: {:?}", e);
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
            let prov = provider.to_string();
            std::thread::spawn(move || {
                match pipeline::log_and_maybe_consolidate(&t, &r, &prov) {
                    Ok(true) => {
                        logln!("[consolidate] threshold hit, running (bg)");
                        match pipeline::consolidate_profile() {
                            Ok(n) => logln!("[consolidate] appended {} bullets", n),
                            Err(e) => logln!("[consolidate] error: {:?}", e),
                        }
                    }
                    Ok(false) => {}
                    Err(e) => logln!("[history] log error: {:?}", e),
                }
            });

            edited.refined
        }
    };

    {
        let _paste_guard = paste_lock.lock().unwrap();
        logln!("[paste] acquired lock, pasting");
        pipeline::copy_and_paste(&output)?;
    }
    player.play_finish();
    Ok(())
}

fn main() {
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("[init] loaded .env from {:?}", path),
        Err(e) => eprintln!("[init] no .env loaded: {}", e),
    }

    let model_path = dirs_home()
        .join("models")
        .join("whisper")
        .join("ggml-large-v3.bin");
    let work_dir = std::env::temp_dir().join("clica-e-fala");
    std::fs::create_dir_all(&work_dir).ok();

    logln!("[init] model={:?}", model_path);
    logln!("[init] work_dir={:?}", work_dir);

    let player = sound::AudioPlayer::new().expect("failed to init audio player");

    let state = AppState {
        recorder: Mutex::new(None),
        model_path,
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
                    let mode = if shortcut
                        .matches(Modifiers::SUPER | Modifiers::SHIFT | Modifiers::ALT, Code::Space)
                    {
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
                "Record / Stop  (⌘⇧Space)",
                true,
                None::<&str>,
            )?;
            let menu = Menu::with_items(app, &[&toggle, &quit])?;

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
                    _ => {}
                })
                .build(app)?;

            if let Err(e) = aura::install(app.handle()) {
                logln!("[aura] install failed: {:?}", e);
            }

            let refined_sc =
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space);
            let raw_sc = Shortcut::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT | Modifiers::ALT),
                Code::Space,
            );
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            app.global_shortcut().register(refined_sc)?;
            app.global_shortcut().register(raw_sc)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}
