use crate::{actions, err, gmail, messages, now, oauth, text, St};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::params;
use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

fn header_urls(h: &str) -> (Vec<String>, Vec<String>) {
    let (mut https, mut mail) = (vec![], vec![]);
    for part in h.split(',') {
        let u = part.trim().trim_start_matches('<').trim_end_matches('>').trim();
        if u.starts_with("https://") || u.starts_with("http://") {
            https.push(u.to_string());
        } else if u.starts_with("mailto:") {
            mail.push(u.to_string());
        }
    }
    (https, mail)
}

async fn one_click(url: &str) -> Result<(), String> {
    let c = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Lumafly/0.1")
        .build()
        .map_err(err)?;
    let r = c
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("List-Unsubscribe=One-Click")
        .send()
        .await
        .map_err(err)?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", r.status().as_u16()))
    }
}

async fn mailto(app: &AppHandle, email: &str, m: &str) -> Result<(), String> {
    let u = reqwest::Url::parse(m).map_err(err)?;
    let to = text::pct_decode(u.path());
    if !to.contains('@') {
        return Err("mailto has no address".into());
    }
    let (mut subj, mut body) = ("unsubscribe".to_string(), String::new());
    for (k, v) in u.query_pairs() {
        match &*k {
            "subject" => subj = v.into_owned(),
            "body" => body = v.into_owned(),
            _ => {}
        }
    }
    let raw = format!(
        "From: {email}\r\nTo: {to}\r\nSubject: {subj}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    );
    let raw = URL_SAFE_NO_PAD.encode(raw);
    let raw = &raw;
    oauth::with_token(app, email, |tok| async move { gmail::send(&tok, raw).await }).await
}

fn pick_link(links: &[(String, String)]) -> Option<String> {
    let hard = ["unsubscribe", "opt-out", "opt out", "optout", "désabonner", "abmelden"];
    let soft = ["manage preferences", "email preferences", "subscription preferences", "manage your subscription", "manage subscription"];
    let hit = |s: &str, keys: &[&str]| {
        let l = s.to_lowercase();
        keys.iter().any(|k| l.contains(k))
    };
    links
        .iter()
        .find(|(_, t)| hit(t, &hard))
        .or_else(|| links.iter().find(|(h, _)| hit(h, &hard)))
        .or_else(|| links.iter().find(|(_, t)| hit(t, &soft)))
        .map(|(h, _)| h.clone())
        .filter(|h| h.starts_with("http"))
}

fn log(st: &St, m: &messages::Msg, method: &str, target: &str, result: &str) {
    let detail = json!({"method": method, "target": target, "category": m.category, "source": "unsubscribe"}).to_string();
    let _ = st.db.lock().unwrap().execute(
        "INSERT INTO actions(msg_id,account,action,detail,result,created) VALUES(?1,?2,'unsub',?3,?4,?5)",
        params![m.id, m.account, detail, result, now()],
    );
}

pub async fn run(app: &AppHandle, m: &messages::Msg) -> Result<&'static str, String> {
    let st = app.state::<St>();
    let (https, mail) = m.list_unsub.as_deref().map(header_urls).unwrap_or_default();
    let one = m
        .list_unsub_post
        .as_deref()
        .map(|p| p.to_lowercase().contains("one-click"))
        .unwrap_or(false);
    if one {
        for u in &https {
            match one_click(u).await {
                Ok(()) => {
                    log(&st, m, "one-click", u, "ok");
                    return Ok("one-click");
                }
                Err(e) => log(&st, m, "one-click", u, &format!("error: {e}")),
            }
        }
    }
    for u in &mail {
        match mailto(app, &m.account, u).await {
            Ok(()) => {
                log(&st, m, "mailto", u, "ok");
                return Ok("mailto");
            }
            Err(e) => log(&st, m, "mailto", u, &format!("error: {e}")),
        }
    }
    let mut link = https.first().cloned();
    if link.is_none() {
        let path = format!("messages/{}", m.id);
        let path = &path;
        let v = oauth::with_token(app, &m.account, |tok| async move {
            gmail::get(&tok, path, &[("format", "full")]).await
        })
        .await?;
        link = pick_link(&text::links(&gmail::html_of(&v)));
    }
    match link {
        Some(u) => match tauri_plugin_opener::open_url(&u, None::<&str>) {
            Ok(()) => {
                log(&st, m, "browser", &u, "ok");
                Ok("browser")
            }
            Err(e) => {
                log(&st, m, "browser", &u, &format!("error: {e}"));
                Err(format!("could not open browser: {e}"))
            }
        },
        None => {
            log(&st, m, "none", "", "error: no unsubscribe method found");
            Err("no unsubscribe link or header found".into())
        }
    }
}

#[tauri::command]
pub async fn unsubscribe(app: AppHandle, id: String) -> Result<String, String> {
    let m = messages::get(&app.state::<St>().db.lock().unwrap(), &id)
        .map_err(err)?
        .ok_or("message not found")?;
    if m.category.as_deref() == Some("spam") {
        return Err("Lumafly never contacts spam senders. Use Spam instead.".into());
    }
    let r = run(&app, &m).await;
    let cat = m.category.as_deref().filter(|c| matches!(*c, "newsletter" | "promotion"));
    let a = actions::execute(&app, &m.account, &id, "archive", cat, "unsubscribe").await;
    let _ = app.emit("actions_changed", ());
    let tail = match a {
        Ok(_) => " Archived.",
        Err(_) => " Archive failed; see Review.",
    };
    Ok(match r {
        Ok("one-click") => format!("Unsubscribed with one-click.{tail}"),
        Ok("mailto") => format!("Unsubscribe email sent.{tail}"),
        Ok(_) => format!("Opened the unsubscribe page in your browser; finish there.{tail}"),
        Err(e) => format!("Could not unsubscribe: {e}.{tail}"),
    })
}
