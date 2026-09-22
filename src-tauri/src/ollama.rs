use crate::St;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

fn client(secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(secs))
        .build()
        .unwrap()
}

fn base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub fn is_local(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| matches!(h, "localhost" | "127.0.0.1" | "::1" | "[::1]")))
        .unwrap_or(false)
}

async fn getj(c: &reqwest::Client, url: String) -> Result<Value, String> {
    c.get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct Probe {
    pub ok: bool,
    pub version: String,
    pub models: Vec<String>,
    pub has_model: bool,
    pub error: String,
}

pub async fn probe(url: &str, model: &str) -> Probe {
    let c = client(5);
    let u = base(url);
    let r = async {
        let v = getj(&c, format!("{u}/api/version")).await?;
        let t = getj(&c, format!("{u}/api/tags")).await?;
        Ok::<_, String>((v, t))
    }
    .await;
    match r {
        Ok((v, t)) => {
            let models: Vec<String> = t["models"]
                .as_array()
                .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
                .unwrap_or_default();
            let has_model = models
                .iter()
                .any(|m| m == model || m.trim_end_matches(":latest") == model);
            Probe {
                ok: true,
                version: v["version"].as_str().unwrap_or("").into(),
                models,
                has_model,
                error: String::new(),
            }
        }
        Err(e) => Probe {
            ok: false,
            version: String::new(),
            models: vec![],
            has_model: false,
            error: e,
        },
    }
}

pub async fn chat(url: &str, model: &str, sys: &str, user: &str, schema: Value) -> Result<Value, String> {
    let body = json!({
        "model": model,
        "stream": false,
        "think": false,
        "keep_alive": "5m",
        "format": schema,
        "options": {"temperature": 0.1},
        "messages": [
            {"role": "system", "content": sys},
            {"role": "user", "content": user}
        ]
    });
    let r: Value = client(300)
        .post(format!("{}/api/chat", base(url)))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let s = r["message"]["content"].as_str().ok_or("no content")?;
    serde_json::from_str(s).map_err(|e| format!("bad json: {e}"))
}

async fn loaded(url: &str, model: &str) -> bool {
    match getj(&client(3), format!("{}/api/ps", base(url))).await {
        Ok(v) => v["models"]
            .as_array()
            .map(|a| a.iter().any(|m| m["name"].as_str() == Some(model) || m["model"].as_str() == Some(model)))
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn unload(url: &str, model: &str) {
    if !loaded(url, model).await {
        return;
    }
    let _ = client(5)
        .post(format!("{}/api/generate", base(url)))
        .json(&json!({"model": model, "keep_alive": 0}))
        .send()
        .await;
}

fn find_bin() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ollama.exe" } else { "ollama" };
    let mut c: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(exe)).collect())
        .unwrap_or_default();
    #[cfg(target_os = "macos")]
    c.extend(
        [
            "/usr/local/bin/ollama",
            "/opt/homebrew/bin/ollama",
            "/Applications/Ollama.app/Contents/Resources/ollama",
        ]
        .map(PathBuf::from),
    );
    #[cfg(target_os = "linux")]
    c.extend(["/usr/local/bin/ollama", "/usr/bin/ollama"].map(PathBuf::from));
    #[cfg(target_os = "windows")]
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        c.push(PathBuf::from(l).join("Programs").join("Ollama").join("ollama.exe"));
    }
    c.into_iter().find(|p| p.is_file())
}

fn cfg(st: &St) -> (String, String) {
    let c = st.cfg.lock().unwrap();
    (c.ollama_url.clone(), c.model.clone())
}

pub async fn ensure(app: &AppHandle) -> Result<bool, String> {
    let st = app.state::<St>();
    let (url, model) = cfg(&st);
    if !is_local(&url) {
        return Ok(false);
    }
    if probe(&url, &model).await.ok {
        return Ok(false);
    }
    {
        let mut g = st.child.lock().unwrap();
        if let Some(ch) = g.as_mut() {
            match ch.try_wait() {
                Ok(None) => return Ok(true),
                _ => *g = None,
            }
        }
    }
    let bin = find_bin().ok_or("ollama binary not found; install from https://ollama.com")?;
    let mut cmd = Command::new(bin);
    cmd.arg("serve").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let ch = cmd.spawn().map_err(|e| e.to_string())?;
    *st.child.lock().unwrap() = Some(ch);
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if probe(&url, &model).await.ok {
            return Ok(true);
        }
    }
    Err("ollama serve started but is not responding".into())
}

pub fn shutdown(app: &AppHandle) {
    let st = app.state::<St>();
    let (url, model) = cfg(&st);
    tauri::async_runtime::block_on(async {
        let _ = tokio::time::timeout(Duration::from_secs(4), unload(&url, &model)).await;
    });
    let ch = st.child.lock().unwrap().take();
    if let Some(mut ch) = ch {
        let _ = ch.kill();
        let _ = ch.wait();
    }
}

#[tauri::command]
pub async fn test_ollama(url: String, model: String) -> Probe {
    probe(&url, &model).await
}

#[tauri::command]
pub async fn ensure_ollama(app: AppHandle) -> Result<bool, String> {
    ensure(&app).await
}

#[allow(dead_code)]
pub async fn ask(st: &State<'_, St>, sys: &str, user: &str, schema: Value) -> Result<Value, String> {
    let (url, model) = cfg(st);
    chat(&url, &model, sys, user, schema).await
}
