use crate::{accounts, err, gmail, messages, now, oauth, St};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn sync_all(app: &AppHandle) {
    let emails = accounts::emails(&app.state::<St>().db.lock().unwrap());
    for e in emails {
        let _ = sync(app, &e, false).await;
    }
}

pub async fn sync(app: &AppHandle, email: &str, full: bool) -> Result<usize, String> {
    let st = app.state::<St>();
    if !st.syncing.lock().unwrap().insert(email.to_string()) {
        return Ok(0);
    }
    let _ = app.emit("sync_start", email);
    let mut r = run(app, email, full).await;
    if matches!(&r, Err(e) if e.starts_with("gmail 401")) {
        oauth::invalidate(app, email);
        r = run(app, email, full).await;
    }
    st.syncing.lock().unwrap().remove(email);
    match &r {
        Ok(n) => {
            let _ = st.db.lock().unwrap().execute(
                "UPDATE accounts SET last_sync=?1 WHERE email=?2",
                rusqlite::params![now(), email],
            );
            let _ = app.emit("sync_done", json!({"email": email, "new": n}));
            crate::classify::kick(app);
        }
        Err(e) => {
            let _ = app.emit("sync_error", json!({"email": email, "error": e}));
        }
    }
    r
}

async fn run(app: &AppHandle, email: &str, full: bool) -> Result<usize, String> {
    let st = app.state::<St>();
    let tok = oauth::token(app, email).await?;
    let hid: Option<String> = st
        .db
        .lock()
        .unwrap()
        .query_row("SELECT history_id FROM accounts WHERE email=?1", [email], |r| r.get(0))
        .ok()
        .flatten();
    if let (Some(h), false) = (&hid, full) {
        match incremental(app, email, &tok, h).await {
            Err(e) if e.starts_with("gmail 404") => {}
            r => return r,
        }
    }
    let days = st.cfg.lock().unwrap().sync_days.max(1) as i64;
    initial(app, email, &tok, days).await
}

fn set_hid(app: &AppHandle, email: &str, hid: &str) {
    if hid.is_empty() {
        return;
    }
    let _ = app.state::<St>().db.lock().unwrap().execute(
        "UPDATE accounts SET history_id=?1 WHERE email=?2",
        rusqlite::params![hid, email],
    );
}

async fn initial(app: &AppHandle, email: &str, tok: &str, days: i64) -> Result<usize, String> {
    let (_, hid) = gmail::profile(tok).await?;
    let ids = gmail::list_ids(tok, &format!("in:inbox after:{}", now() - days * 86400)).await?;
    let known = messages::known(&app.state::<St>().db.lock().unwrap(), email);
    let todo: Vec<String> = ids.into_iter().filter(|i| !known.contains(i)).collect();
    let n = fetch_all(app, email, tok, &todo, true).await;
    set_hid(app, email, &hid);
    Ok(n)
}

async fn incremental(app: &AppHandle, email: &str, tok: &str, hid: &str) -> Result<usize, String> {
    let h = gmail::history(tok, hid).await?;
    let st = app.state::<St>();
    let known = {
        let db = st.db.lock().unwrap();
        for d in &h.deleted {
            let _ = db.execute("DELETE FROM messages WHERE id=?1", [d]);
        }
        messages::known(&db, email)
    };
    let gone: HashSet<&String> = h.deleted.iter().collect();
    let mut seen = HashSet::new();
    let new: Vec<String> = h
        .added
        .iter()
        .filter(|i| !known.contains(*i) && !gone.contains(i) && seen.insert((*i).clone()))
        .cloned()
        .collect();
    let n = fetch_all(app, email, tok, &new, false).await;
    let mut seen = HashSet::new();
    let changed: Vec<&String> = h
        .changed
        .iter()
        .filter(|i| known.contains(*i) && !gone.contains(i) && seen.insert((*i).clone()))
        .collect();
    for id in changed {
        if let Ok(l) = gmail::labels(tok, id).await {
            let _ = messages::set_labels(&st.db.lock().unwrap(), id, &l);
        }
    }
    set_hid(app, email, &h.hid);
    Ok(n)
}

async fn fetch_all(app: &AppHandle, email: &str, tok: &str, ids: &[String], any_state: bool) -> usize {
    let st = app.state::<St>();
    let sem = Arc::new(Semaphore::new(8));
    let mut js = JoinSet::new();
    for id in ids {
        let (sem, tok, id, acct) = (sem.clone(), tok.to_string(), id.clone(), email.to_string());
        js.spawn(async move {
            let _p = sem.acquire().await;
            gmail::fetch(&tok, &acct, &id).await
        });
    }
    let (mut done, total, mut n) = (0usize, ids.len(), 0usize);
    while let Some(r) = js.join_next().await {
        done += 1;
        if let Ok(Ok(m)) = r {
            if any_state || m.state == "inbox" {
                if messages::upsert(&st.db.lock().unwrap(), &m).is_ok() {
                    n += 1;
                }
            }
        }
        if done % 10 == 0 || done == total {
            let _ = app.emit("sync_progress", json!({"email": email, "done": done, "total": total}));
        }
    }
    n
}

#[tauri::command]
pub async fn sync_now(app: AppHandle, email: Option<String>, full: Option<bool>) -> Result<usize, String> {
    match email {
        Some(e) => sync(&app, &e, full.unwrap_or(false)).await,
        None => {
            let emails = accounts::emails(&app.state::<St>().db.lock().unwrap());
            let mut n = 0;
            for e in emails {
                n += sync(&app, &e, full.unwrap_or(false)).await.map_err(err)?;
            }
            Ok(n)
        }
    }
}
