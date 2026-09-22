use crate::{err, gmail, messages, now, oauth, St};
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

pub fn label_name(cat: &str) -> String {
    let mut c = cat.chars();
    match c.next() {
        Some(f) => format!("Lumafly/{}{}", f.to_uppercase(), c.as_str()),
        None => "Lumafly".into(),
    }
}

pub fn ops(action: &str, cat: Option<&str>) -> Result<(Vec<String>, Vec<String>), String> {
    let lbl = cat.map(label_name);
    Ok(match action {
        "spam" => (vec!["SPAM".into()], vec!["INBOX".into()]),
        "trash" => (vec!["TRASH".into()], vec!["INBOX".into()]),
        "archive" => (lbl.into_iter().collect(), vec!["INBOX".into()]),
        "label" => (lbl.into_iter().collect(), vec![]),
        _ => return Err(format!("unknown action {action}")),
    })
}

pub fn describe(action: &str, cat: Option<&str>) -> String {
    let l = cat.map(label_name).unwrap_or_default();
    match action {
        "spam" => "Move to Spam".into(),
        "trash" => "Move to Trash".into(),
        "archive" if l.is_empty() => "Archive".into(),
        "archive" => format!("Label {l} and archive"),
        "label" => format!("Label {l}"),
        "unsub" => "Unsubscribe".into(),
        _ => action.into(),
    }
}

fn is_system(n: &str) -> bool {
    n.chars().all(|c| c.is_ascii_uppercase() || c == '_')
}

fn cached(st: &St, email: &str, name: &str) -> Option<String> {
    st.db
        .lock()
        .unwrap()
        .query_row("SELECT id FROM labels WHERE account=?1 AND name=?2", [email, name], |r| r.get(0))
        .ok()
}

fn cache(st: &St, email: &str, name: &str, id: &str) {
    let _ = st.db.lock().unwrap().execute(
        "INSERT OR REPLACE INTO labels(account,name,id) VALUES(?1,?2,?3)",
        [email, name, id],
    );
}

async fn resolve(app: &AppHandle, email: &str, tok: &str, names: &[String]) -> Result<Vec<String>, String> {
    let st = app.state::<St>();
    let mut out = vec![];
    let mut fresh = false;
    for n in names {
        if is_system(n) {
            out.push(n.clone());
            continue;
        }
        if let Some(id) = cached(&st, email, n) {
            out.push(id);
            continue;
        }
        if !fresh {
            for (name, id) in gmail::labels_list(tok).await? {
                cache(&st, email, &name, &id);
            }
            fresh = true;
            if let Some(id) = cached(&st, email, n) {
                out.push(id);
                continue;
            }
        }
        if let Some((parent, _)) = n.rsplit_once('/') {
            if cached(&st, email, parent).is_none() {
                let pid = gmail::label_create(tok, parent).await?;
                cache(&st, email, parent, &pid);
            }
        }
        let id = gmail::label_create(tok, n).await?;
        cache(&st, email, n, &id);
        out.push(id);
    }
    Ok(out)
}

pub async fn execute(
    app: &AppHandle,
    email: &str,
    msg_id: &str,
    action: &str,
    cat: Option<&str>,
    source: &str,
) -> Result<i64, String> {
    let st = app.state::<St>();
    let (add, rm) = ops(action, cat)?;
    let (add_r, rm_r) = (&add, &rm);
    let r = oauth::with_token(app, email, |tok| async move {
        let a = resolve(app, email, &tok, add_r).await?;
        let d = resolve(app, email, &tok, rm_r).await?;
        gmail::modify(&tok, msg_id, &a, &d).await
    })
    .await;
    let detail = json!({"add": add, "remove": rm, "category": cat, "source": source}).to_string();
    let result = match &r {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error: {e}"),
    };
    let id = {
        let db = st.db.lock().unwrap();
        db.execute(
            "INSERT INTO actions(msg_id,account,action,detail,result,created) VALUES(?1,?2,?3,?4,?5,?6)",
            params![msg_id, email, action, detail, result, now()],
        )
        .map_err(err)?;
        let id = db.last_insert_rowid();
        if let Ok(l) = &r {
            let _ = messages::set_labels(&db, msg_id, l);
        }
        id
    };
    let _ = app.emit("message_changed", msg_id);
    r.map(|_| id)
}

#[derive(Serialize)]
pub struct Act {
    pub id: i64,
    pub msg_id: String,
    pub account: String,
    pub action: String,
    pub detail: Value,
    pub result: String,
    pub undone: bool,
    pub created: i64,
    pub subject: String,
    pub sender: String,
    pub text: String,
}

#[tauri::command]
pub fn list_actions(st: State<'_, St>, limit: u32) -> Result<Vec<Act>, String> {
    let db = st.db.lock().unwrap();
    let mut q = db
        .prepare(
            "SELECT a.id,a.msg_id,a.account,a.action,a.detail,a.result,a.undone,a.created,
                    COALESCE(m.subject,''),COALESCE(m.sender,'')
             FROM actions a LEFT JOIN messages m ON m.id=a.msg_id ORDER BY a.id DESC LIMIT ?1",
        )
        .map_err(err)?;
    let rows = q
        .query_map([limit], |r| {
            let detail: Value = serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null);
            let action: String = r.get(3)?;
            let cat = detail["category"].as_str().map(String::from);
            let text = match detail["method"].as_str() {
                Some(m) => format!("Unsubscribe via {m}"),
                None => describe(&action, cat.as_deref()),
            };
            Ok(Act {
                id: r.get(0)?,
                msg_id: r.get(1)?,
                account: r.get(2)?,
                text,
                action,
                detail,
                result: r.get(5)?,
                undone: r.get::<_, i32>(6)? != 0,
                created: r.get(7)?,
                subject: r.get(8)?,
                sender: r.get(9)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[tauri::command]
pub async fn undo_action(app: AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<St>();
    let (msg_id, email, detail, result, undone, action): (String, String, String, String, i32, String) = st
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT msg_id,account,detail,result,undone,action FROM actions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .map_err(err)?;
    if action == "unsub" {
        return Err("an unsubscribe cannot be undone; re-subscribe on the sender's site".into());
    }
    if undone != 0 {
        return Err("already undone".into());
    }
    if result != "ok" {
        return Err("that action did not succeed, nothing to undo".into());
    }
    let d: Value = serde_json::from_str(&detail).map_err(err)?;
    let names = |k: &str| -> Vec<String> {
        d[k].as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    };
    let (add, rm) = (names("remove"), names("add"));
    let (app_r, email_r, msg_r, add_r, rm_r) = (&app, &email, &msg_id, &add, &rm);
    let labels = oauth::with_token(&app, &email, |tok| async move {
        let a = resolve(app_r, email_r, &tok, add_r).await?;
        let r = resolve(app_r, email_r, &tok, rm_r).await?;
        gmail::modify(&tok, msg_r, &a, &r).await
    })
    .await?;
    {
        let db = st.db.lock().unwrap();
        db.execute("UPDATE actions SET undone=1 WHERE id=?1", [id]).map_err(err)?;
        let _ = messages::set_labels(&db, &msg_id, &labels);
        let _ = db.execute(
            "UPDATE proposals SET status='pending' WHERE msg_id=?1 AND status IN ('approved','auto')",
            [&msg_id],
        );
    }
    let _ = app.emit("message_changed", &msg_id);
    let _ = app.emit("actions_changed", ());
    let _ = app.emit("proposals_changed", ());
    Ok(())
}

#[tauri::command]
pub async fn act(app: AppHandle, id: String, action: String) -> Result<i64, String> {
    let (email, cat): (String, Option<String>) = app
        .state::<St>()
        .db
        .lock()
        .unwrap()
        .query_row("SELECT account,category FROM messages WHERE id=?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?;
    let cat = if action == "label" || action == "archive" { cat } else { None };
    let r = execute(&app, &email, &id, &action, cat.as_deref(), "manual").await;
    let _ = app.emit("actions_changed", ());
    r
}

pub async fn execute_many(
    app: &AppHandle,
    email: &str,
    ids: &[String],
    action: &str,
    cat: Option<&str>,
    source: &str,
) -> Result<Vec<String>, String> {
    let st = app.state::<St>();
    let (add, rm) = ops(action, cat)?;
    let (add_r, rm_r) = (&add, &rm);
    let (a, d) = oauth::with_token(app, email, |tok| async move {
        Ok((resolve(app, email, &tok, add_r).await?, resolve(app, email, &tok, rm_r).await?))
    })
    .await?;
    let (a_r, d_r) = (&a, &d);
    let detail = json!({"add": add, "remove": rm, "category": cat, "source": source}).to_string();
    let mut done = vec![];
    for chunk in ids.chunks(500) {
        let r = oauth::with_token(app, email, |tok| async move { gmail::batch_modify(&tok, chunk, a_r, d_r).await }).await;
        let result = match &r {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("error: {e}"),
        };
        {
            let db = st.db.lock().unwrap();
            for id in chunk {
                let _ = db.execute(
                    "INSERT INTO actions(msg_id,account,action,detail,result,created) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![id, email, action, detail, result, now()],
                );
                if r.is_ok() {
                    let old: String = db
                        .query_row("SELECT COALESCE(labels,'') FROM messages WHERE id=?1", [id], |r| r.get(0))
                        .unwrap_or_default();
                    let mut l: Vec<String> = old.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                    l.retain(|x| !d.contains(x));
                    for x in &a {
                        if !l.contains(x) {
                            l.push(x.clone());
                        }
                    }
                    let _ = messages::set_labels(&db, id, &l);
                    done.push(id.clone());
                }
            }
        }
        let _ = app.emit("message_changed", "*");
        let _ = app.emit("approve_progress", json!({"email": email, "done": done.len(), "total": ids.len()}));
        r?;
    }
    Ok(done)
}
