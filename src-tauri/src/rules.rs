use crate::{actions, err, messages, now, St};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Serialize, Deserialize, Clone)]
pub struct Rule {
    pub id: i64,
    pub sender: String,
    pub action: String,
    pub except: String,
    pub auto: bool,
    pub note: String,
}

pub fn list(db: &Connection) -> Vec<Rule> {
    let Ok(mut q) = db.prepare("SELECT id,sender,action,unless,auto,note FROM rules ORDER BY id") else { return vec![] };
    q.query_map([], |r| {
        Ok(Rule {
            id: r.get(0)?,
            sender: r.get(1)?,
            action: r.get(2)?,
            except: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            auto: r.get::<_, i32>(4)? != 0,
            note: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        })
    })
    .map(|it| it.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

pub fn matching(db: &Connection, m: &messages::Msg) -> Option<Rule> {
    let from = m.sender.to_lowercase();
    let text = format!("{}\n{}", m.subject, m.body).to_lowercase();
    list(db).into_iter().find(|r| {
        let s = r.sender.trim().to_lowercase();
        if s.is_empty() || !from.contains(&s) {
            return false;
        }
        !r.except
            .split(',')
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty())
            .any(|k| text.contains(&k))
    })
}

pub async fn propose(app: &AppHandle, m: &messages::Msg, r: &Rule) -> Option<String> {
    let st = app.state::<St>();
    let cat = m.category.clone().unwrap_or_else(|| "notification".into());
    let pid = {
        let db = st.db.lock().unwrap();
        let done: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM actions WHERE msg_id=?1 AND action=?2 AND result='ok' AND undone=0",
                params![m.id, r.action],
                |x| x.get(0),
            )
            .unwrap_or(0);
        let same: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM proposals WHERE msg_id=?1 AND action=?2 AND status IN ('pending','auto')",
                params![m.id, r.action],
                |x| x.get(0),
            )
            .unwrap_or(0);
        if done > 0 || same > 0 {
            return None;
        }
        let _ = db.execute("UPDATE proposals SET status='rejected' WHERE msg_id=?1 AND status='pending'", [&m.id]);
        let _ = db.execute(
            "INSERT INTO proposals(msg_id,account,action,category,status,created) VALUES(?1,?2,?3,?4,?5,?6)",
            params![m.id, m.account, r.action, cat, if r.auto { "auto" } else { "pending" }, now()],
        );
        db.last_insert_rowid()
    };
    if !r.auto {
        let _ = app.emit("proposals_changed", ());
        return None;
    }
    match actions::execute(app, &m.account, &m.id, &r.action, None, &format!("rule: {}", r.sender)).await {
        Ok(_) => {
            let _ = app.emit("actions_changed", ());
            Some(format!("{} ({})", actions::describe(&r.action, None), r.sender))
        }
        Err(_) => {
            let _ = st
                .db
                .lock()
                .unwrap()
                .execute("UPDATE proposals SET status='pending' WHERE id=?1", [pid]);
            let _ = app.emit("proposals_changed", ());
            None
        }
    }
}

pub async fn apply_all(app: &AppHandle) -> Vec<String> {
    let st = app.state::<St>();
    let rules = list(&st.db.lock().unwrap());
    if rules.is_empty() {
        return vec![];
    }
    let ids: Vec<String> = {
        let db = st.db.lock().unwrap();
        let Ok(mut q) = db.prepare("SELECT id FROM messages WHERE state='inbox'") else { return vec![] };
        q.query_map([], |r| r.get(0)).map(|it| it.filter_map(Result::ok).collect()).unwrap_or_default()
    };
    let mut out = vec![];
    for id in ids {
        let hit = {
            let db = st.db.lock().unwrap();
            messages::get(&db, &id).ok().flatten().and_then(|m| matching(&db, &m).map(|r| (m, r)))
        };
        if let Some((m, r)) = hit {
            if let Some(t) = propose(app, &m, &r).await {
                out.push(t);
            }
        }
    }
    out
}

#[tauri::command]
pub fn list_rules(st: State<'_, St>) -> Vec<Rule> {
    list(&st.db.lock().unwrap())
}

#[tauri::command]
pub fn save_rule(st: State<'_, St>, r: Rule) -> Result<i64, String> {
    if !matches!(r.action.as_str(), "trash" | "archive" | "spam") {
        return Err("action must be trash, archive or spam".into());
    }
    if r.sender.trim().is_empty() {
        return Err("sender is required".into());
    }
    let db = st.db.lock().unwrap();
    if r.id > 0 {
        db.execute(
            "UPDATE rules SET sender=?1,action=?2,unless=?3,auto=?4,note=?5 WHERE id=?6",
            params![r.sender.trim(), r.action, r.except, r.auto as i32, r.note, r.id],
        )
        .map_err(err)?;
        Ok(r.id)
    } else {
        db.execute(
            "INSERT INTO rules(sender,action,unless,auto,note) VALUES(?1,?2,?3,?4,?5)",
            params![r.sender.trim(), r.action, r.except, r.auto as i32, r.note],
        )
        .map_err(err)?;
        Ok(db.last_insert_rowid())
    }
}

#[tauri::command]
pub fn delete_rule(st: State<'_, St>, id: i64) -> Result<(), String> {
    st.db.lock().unwrap().execute("DELETE FROM rules WHERE id=?1", [id]).map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn apply_rules(app: AppHandle) -> Result<usize, String> {
    let n = apply_all(&app).await.len();
    let _ = app.emit("proposals_changed", ());
    Ok(n)
}
