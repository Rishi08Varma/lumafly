use crate::{err, settings, St};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str = "https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/gmail.send";

fn rnd() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    B64.encode(b)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap()
}

fn creds(st: &St) -> Result<(String, String), String> {
    let id = st.cfg.lock().unwrap().client_id.clone();
    if id.is_empty() {
        return Err("set the Google client ID in Settings".into());
    }
    let sec = settings::secret_get("client_secret").ok_or("set the Google client secret in Settings")?;
    Ok((id, sec))
}

pub fn rt_key(email: &str) -> String {
    format!("refresh:{email}")
}

pub async fn flow(app: &AppHandle, hint: Option<&str>) -> Result<(String, String, String), String> {
    let (id, sec) = creds(&app.state::<St>())?;
    let l = TcpListener::bind("127.0.0.1:0").await.map_err(err)?;
    let redir = format!("http://127.0.0.1:{}", l.local_addr().map_err(err)?.port());
    let ver = rnd();
    let state = rnd();
    let chal = B64.encode(Sha256::digest(ver.as_bytes()));
    let mut p = vec![
        ("client_id", id.as_str()),
        ("redirect_uri", redir.as_str()),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", chal.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state.as_str()),
    ];
    if let Some(h) = hint {
        p.push(("login_hint", h));
    }
    let url = reqwest::Url::parse_with_params(AUTH, &p).map_err(err)?;
    tauri_plugin_opener::open_url(url.as_str(), None::<&str>).map_err(err)?;
    let code = tokio::time::timeout(Duration::from_secs(300), listen(l, &state))
        .await
        .map_err(|_| "timed out waiting for Google sign-in")??;
    let tok: Value = http()
        .post(TOKEN)
        .form(&[
            ("client_id", id.as_str()),
            ("client_secret", sec.as_str()),
            ("code", code.as_str()),
            ("code_verifier", ver.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redir.as_str()),
        ])
        .send()
        .await
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    let access = tok["access_token"]
        .as_str()
        .ok_or_else(|| format!("token exchange failed: {}", tok["error_description"].as_str().unwrap_or(&tok.to_string())))?
        .to_string();
    let refresh = tok["refresh_token"].as_str().ok_or("Google returned no refresh token")?.to_string();
    let (email, _) = crate::gmail::profile(&access).await?;
    Ok((email, access, refresh))
}

async fn listen(l: TcpListener, state: &str) -> Result<String, String> {
    loop {
        let (mut s, _) = l.accept().await.map_err(err)?;
        let mut buf = vec![0u8; 8192];
        let n = s.read(&mut buf).await.map_err(err)?;
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
        if !path.starts_with("/?") {
            let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let u = reqwest::Url::parse(&format!("http://x{path}")).map_err(err)?;
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        let (body, res) = if q.get("state").map(String::as_str) != Some(state) {
            ("Sign-in failed: state mismatch.", Err("state mismatch".to_string()))
        } else if let Some(c) = q.get("code") {
            ("Lumafly is connected. You can close this tab.", Ok(c.clone()))
        } else {
            ("Sign-in was cancelled.", Err(q.get("error").cloned().unwrap_or("access denied".into())))
        };
        let html = format!("<!doctype html><meta charset=utf-8><body style=\"font-family:sans-serif;padding:40px\"><h2>{body}</h2>");
        let _ = s
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
                    html.len()
                )
                .as_bytes(),
            )
            .await;
        let _ = s.shutdown().await;
        return res;
    }
}

pub fn set_needs_auth(app: &AppHandle, email: &str, v: bool) {
    let st = app.state::<St>();
    let _ = st.db.lock().unwrap().execute(
        "UPDATE accounts SET needs_auth=?1 WHERE email=?2",
        rusqlite::params![v as i32, email],
    );
    if v {
        let _ = app.emit("account_needs_auth", email);
    }
}

pub async fn token(app: &AppHandle, email: &str) -> Result<String, String> {
    let st = app.state::<St>();
    let cached = st.tokens.lock().unwrap().get(email).cloned();
    if let Some((t, exp)) = cached {
        if exp > Instant::now() + Duration::from_secs(60) {
            return Ok(t);
        }
    }
    let (id, sec) = creds(&st)?;
    let rt = match settings::secret_get(&rt_key(email)) {
        Some(r) => r,
        None => {
            set_needs_auth(app, email, true);
            return Err("no refresh token; re-authenticate".into());
        }
    };
    let r: Value = http()
        .post(TOKEN)
        .form(&[
            ("client_id", id.as_str()),
            ("client_secret", sec.as_str()),
            ("refresh_token", rt.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    if let Some(a) = r["access_token"].as_str() {
        let exp = Instant::now() + Duration::from_secs(r["expires_in"].as_u64().unwrap_or(3600));
        st.tokens.lock().unwrap().insert(email.into(), (a.into(), exp));
        return Ok(a.into());
    }
    let e = r["error"].as_str().unwrap_or("unknown").to_string();
    if e == "invalid_grant" {
        set_needs_auth(app, email, true);
        return Err("Google session expired (Testing apps expire after 7 days); re-authenticate".into());
    }
    Err(format!("token refresh failed: {e}"))
}

pub async fn revoke(rt: &str) {
    let _ = http()
        .post("https://oauth2.googleapis.com/revoke")
        .form(&[("token", rt)])
        .send()
        .await;
}
