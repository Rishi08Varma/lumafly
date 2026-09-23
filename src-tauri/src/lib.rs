mod accounts;
mod actions;
mod classify;
mod db;
mod digest;
mod gmail;
mod messages;
mod notify;
mod oauth;
mod ollama;
mod review;
mod rules;
mod settings;
mod sync;
mod text;
mod unsub;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Manager, RunEvent};

pub struct St {
    pub cfg: Mutex<settings::Settings>,
    pub db: Mutex<rusqlite::Connection>,
    pub child: Mutex<Option<Child>>,
    pub tokens: Mutex<HashMap<String, (String, i64)>>,
    pub syncing: Mutex<HashSet<String>>,
    pub classifying: Mutex<bool>,
    pub dir: PathBuf,
}

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
                tokens: Mutex::new(HashMap::new()),
                syncing: Mutex::new(HashSet::new()),
                classifying: Mutex::new(false),
                dir,
            });
            let h = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = ollama::ensure(&h).await;
            });
            let h = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                loop {
                    sync::sync_all(&h).await;
                    let mins = h.state::<St>().cfg.lock().unwrap().poll_min.max(1) as u64;
                    tokio::time::sleep(Duration::from_secs(mins * 60)).await;
                }
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
            accounts::list_accounts,
            accounts::add_account,
            accounts::reauth_account,
            accounts::remove_account,
            sync::sync_now,
            messages::list_messages,
            classify::classify_now,
            classify::classify_pending,
            classify::reclassify,
            classify::get_message,
            actions::list_actions,
            actions::undo_action,
            actions::act,
            review::list_proposals,
            review::pending_count,
            review::pending_ids,
            review::approve,
            review::reject,
            unsub::unsubscribe,
            digest::make_digest,
            digest::last_digest,
            review::approval_counts,
            rules::list_rules,
            rules::save_rule,
            rules::delete_rule,
            rules::apply_rules,
        ])
        .build(tauri::generate_context!())
        .expect("tauri build")
        .run(|app, ev| {
            if let RunEvent::Exit = ev {
                ollama::shutdown(app);
            }
        });
}
