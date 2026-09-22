mod db;
mod ollama;
mod settings;

use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;
use tauri::{Manager, RunEvent};

pub struct St {
    pub cfg: Mutex<settings::Settings>,
    pub db: Mutex<rusqlite::Connection>,
    pub child: Mutex<Option<Child>>,
    pub dir: PathBuf,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&dir)?;
            let cfg = settings::load(&dir);
            let db = db::open(&dir.join("lumafly.db"))?;
            app.manage(St {
                cfg: Mutex::new(cfg),
                db: Mutex::new(db),
                child: Mutex::new(None),
                dir,
            });
            let h = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = ollama::ensure(&h).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            settings::set_client_secret,
            settings::has_client_secret,
            ollama::test_ollama,
            ollama::ensure_ollama,
        ])
        .build(tauri::generate_context!())
        .expect("tauri build")
        .run(|app, ev| {
            if let RunEvent::Exit = ev {
                ollama::shutdown(app);
            }
        });
}
