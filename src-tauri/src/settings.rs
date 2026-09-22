use crate::St;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tauri::State;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub ollama_url: String,
    pub model: String,
    pub poll_min: u32,
    pub sync_days: u32,
    pub client_id: String,
    pub notify: bool,
    pub auto: HashMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ollama_url: "http://localhost:11434".into(),
            model: "qwen3:30b-a3b".into(),
            poll_min: 5,
            sync_days: 30,
            client_id: String::new(),
            notify: true,
            auto: HashMap::new(),
        }
    }
}

pub fn load(dir: &Path) -> Settings {
    fs::read(dir.join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, s: &Settings) -> Result<(), String> {
    let b = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), b).map_err(|e| e.to_string())
}

fn entry(k: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("lumafly", k).map_err(|e| e.to_string())
}

pub fn secret_get(k: &str) -> Option<String> {
    entry(k).ok()?.get_password().ok()
}

pub fn secret_set(k: &str, v: &str) -> Result<(), String> {
    entry(k)?.set_password(v).map_err(|e| e.to_string())
}

pub fn secret_del(k: &str) {
    if let Ok(e) = entry(k) {
        let _ = e.delete_credential();
    }
}

#[tauri::command]
pub fn get_settings(st: State<'_, St>) -> Settings {
    st.cfg.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(st: State<'_, St>, s: Settings) -> Result<(), String> {
    save(&st.dir, &s)?;
    *st.cfg.lock().unwrap() = s;
    Ok(())
}

#[tauri::command]
pub fn set_client_secret(v: String) -> Result<(), String> {
    if v.is_empty() {
        secret_del("client_secret");
        return Ok(());
    }
    secret_set("client_secret", &v)
}

#[tauri::command]
pub fn has_client_secret() -> bool {
    secret_get("client_secret").is_some()
}
