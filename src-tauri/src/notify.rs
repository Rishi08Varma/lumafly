use crate::St;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if !app.state::<St>().cfg.lock().unwrap().notify {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}
