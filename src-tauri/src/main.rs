#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use linux_scrollmaster::{bridge, native_host::NativeHost};
use serde_json::Value;
mod autostart;
mod instance;
mod lifecycle;
mod locale;
mod profile_files;
mod scroll_speed;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
static REVISION: AtomicU64 = AtomicU64::new(0);
/// Legacy Solaar/Python worker by default; the native worker only when the
/// launcher explicitly sets SCROLLMASTER_BACKEND=native. Never both, no fallback.
enum Selected {
    Legacy(Option<bridge::Worker>),
    Native(NativeHost),
}
struct Backend(Mutex<Selected>);
#[tauri::command]
async fn request(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || hardware_request(app, payload))
        .await
        .map_err(|e| e.to_string())?
}
fn hardware_request(app: tauri::AppHandle, payload: Value) -> Result<Value, String> {
    let state = app.state::<Backend>();
    let mut guard = state.0.lock().map_err(|_| "Servicio ocupado".to_string())?;
    if let Selected::Native(host) = &mut *guard {
        let mut reply = host
            .request(&payload, Duration::from_secs(45))
            .unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error}));
        reply["revision"] = Value::from(REVISION.fetch_add(1, Ordering::SeqCst) + 1);
        return Ok(reply);
    }
    let Selected::Legacy(guard) = &mut *guard else {
        unreachable!()
    };
    let result = (|| {
        if guard.is_none() {
            let script = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bridge/service.py")
            } else {
                app.path()
                    .resource_dir()
                    .map_err(|e| e.to_string())?
                    .join("bridge/service.py")
            };
            *guard = Some(bridge::Worker::spawn(&script)?);
        }
        guard
            .as_mut()
            .ok_or("Sin servicio")?
            .transact(&payload, Duration::from_secs(45))
    })();
    if result.is_err() {
        guard.take();
    }
    let mut reply = result.unwrap_or_else(|error| serde_json::json!({"ok": false, "error": error}));
    reply["revision"] = Value::from(REVISION.fetch_add(1, Ordering::SeqCst) + 1);
    Ok(reply)
}
fn main() {
    // The native worker and its guardian child re-execute this binary.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| linux_scrollmaster::worker::ENTRY_FLAGS.contains(&arg.as_str()))
    {
        if let Err(error) = linux_scrollmaster::worker::main_with_args(args) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let selected = if linux_scrollmaster::native_host::native_selected() {
        match NativeHost::system() {
            Ok(host) => Selected::Native(host),
            Err(error) => {
                eprintln!("No se pudo preparar el backend nativo: {error}");
                return;
            }
        }
    } else {
        Selected::Legacy(None)
    };
    let (instance_lease, listener) =
        match instance::runtime_directory().and_then(|path| instance::acquire(&path)) {
            Ok(instance::Instance::Primary(lease, listener)) => (lease, listener),
            Ok(instance::Instance::Existing) => return,
            Err(error) => {
                eprintln!("No se pudo iniciar Linux ScrollMaster: {error}");
                return;
            }
        };
    // Verified workaround for WebKitGTK Error 71 on this NVIDIA/Wayland host.
    // Scoped to Linux ScrollMaster; respects an explicit user override. No compositor edits.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::path::Path::new("/sys/module/nvidia").exists()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .manage(Backend(Mutex::new(selected)))
        .manage(lifecycle::DesktopState::new())
        .invoke_handler(tauri::generate_handler![
            request,
            profile_files::import_profile_file,
            profile_files::export_profile_file,
            lifecycle::desktop_preferences,
            lifecycle::set_tray,
            autostart::autostart_preferences,
            autostart::set_autostart,
            locale::set_locale,
            scroll_speed::scroll_speed_preferences,
            scroll_speed::set_scroll_speed
        ])
        .setup(move |app| {
            // Keep the lease until native exit cleanup has completed.
            app.manage(instance_lease);
            let window_app = app.handle().clone();
            instance::serve(listener, move || {
                let handle = window_app.clone();
                let _ = window_app.run_on_main_thread(move || lifecycle::show_window(&handle));
            });
            lifecycle::initialize(app.handle());
            scroll_speed::keep_applied();
            // Native polling continues when WebKit throttles a hidden/minimized window.
            // No autostart, daemon installation or compositor configuration is needed.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(5));
                let reply = hardware_request(handle.clone(), serde_json::json!({"op": "snapshot"}))
                    .unwrap_or_else(|error| {
                        serde_json::json!({
                            "ok": false, "error": error,
                            "revision": REVISION.fetch_add(1, Ordering::SeqCst) + 1
                        })
                    });
                let _ = handle.emit("device-snapshot", reply);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide only with a visible tray to bring the window back; a host that
                // came up after login gets its icon now.
                if lifecycle::ensure_tray(window.app_handle()) && window.hide().is_ok() {
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("No se pudo iniciar Linux ScrollMaster")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Ok(mut selected) = app.state::<Backend>().0.lock() {
                    match &mut *selected {
                        Selected::Legacy(worker) => drop(worker.take()),
                        // Confirmed shutdown: the guardian restores HID first.
                        Selected::Native(host) => {
                            if let Err(error) = host.shutdown() {
                                eprintln!("Cierre nativo no confirmado: {error}");
                            }
                        }
                    }
                }
            }
        });
}
