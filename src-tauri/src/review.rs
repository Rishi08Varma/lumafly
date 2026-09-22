use crate::{actions, err, now, St};
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub fn default_action(cat: &str) -> Option<&'static str> {
    match cat {
        "spam" => Some("spam"),
        "notification" => Some("archive"),
        "important" | "personal" | "newsletter" | "promotion" => Some("label"),
        _ => None,
    }
}

pub fn propose(app: &AppHandle, msg_id: &str, account: &str, cat: &str) {
    let Some(action) = default_action(cat) else { return };
    let st = app.state::<St>();
    let db = st.db.lock().unwrap();
    let dup: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM proposals WHERE msg_id=?1 AND status IN ('pending','approved')",
            [msg_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let done: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM actions WHERE msg_id=?1 AND action=?2 AND result='ok' AND undone=0",
            [msg_id, action],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if dup > 0 || done > 0 {
        return;
    }
    let _ = db.execute(
        "INSERT INTO proposals(msg_id,account,action,category,status,created) VALUES(?1,?2,?3,?4,'pending',?5)",
        params![msg_id, account, action, cat, now()],
    );
    drop(db);
    let _ = app.emit("proposals_changed", ());
}

pub fn backfill(app: &AppHandle) {
    let rows: Vec<(String, String, String)> = {
        let st = app.state::<St>();
        let db = st.db.lock().unwrap();
        let Ok(mut q) = db.prepare(
            "SELECT m.id,m.account,m.category FROM messages m
             WHERE m.state='inbox' AND m.category IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM proposals p WHERE p.msg_id=m.id)
               AND NOT EXISTS(SELECT 1 FROM actions a WHERE a.msg_id=m.id AND a.result='ok' AND a.undone=0)",
        ) else { return };
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default()
    };
    for (id, acct, cat) in rows {
        propose(app, &id, &acct, &cat);
    }
}

#[derive(Serialize)]
pub struct Prop {
    pub id: i64,
    pub msg_id: String,
    pub account: String,
    pub action: String,
    pub category: String,
    pub created: i64,
    pub subject: String,
    pub sender: String,
    pub summary: Option<String>,
    pub confidence: Option<f64>,
    pub text: String,
}

#[tauri::command]
pub fn list_proposals(st: State<'_, St>) -> Result<Vec<Prop>, String> {
    let db = st.db.lock().unwrap();
    let mut q = db
        .prepare(
            "SELECT p.id,p.msg_id,p.account,p.action,p.category,p.created,
                    COALESCE(m.subject,''),COALESCE(m.sender,''),m.summary,m.confidence
             FROM proposals p JOIN messages m ON m.id=p.msg_id
             WHERE p.status='pending' AND m.state='inbox' ORDER BY m.date DESC",
        )
        .map_err(err)?;
    let rows = q
        .query_map([], |r| {
            let action: String = r.get(3)?;
            let category: String = r.get(4)?;
            Ok(Prop {
                id: r.get(0)?,
                msg_id: r.get(1)?,
                account: r.get(2)?,
                text: actions::describe(&action, Some(&category)),
                action,
                category,
                created: r.get(5)?,
                subject: r.get(6)?,
                sender: r.get(7)?,
                summary: r.get(8)?,
                confidence: r.get(9)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[tauri::command]
pub fn pending_count(st: State<'_, St>) -> i64 {
    st.db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM proposals p JOIN messages m ON m.id=p.msg_id WHERE p.status='pending' AND m.state='inbox'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0)
}

fn set_status(st: &St, id: i64, status: &str) {
    let _ = st
        .db
        .lock()
        .unwrap()
        .execute("UPDATE proposals SET status=?1 WHERE id=?2", params![status, id]);
}

async fn approve_one(app: &AppHandle, id: i64) -> Result<(), String> {
    let st = app.state::<St>();
    let (msg_id, email, action, cat): (String, String, String, String) = st
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT msg_id,account,action,category FROM proposals WHERE id=?1 AND status='pending'",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "proposal not pending".to_string())?;
    let r = actions::execute(app, &email, &msg_id, &action, Some(&cat), "review").await;
    match &r {
        Ok(_) => {
            set_status(&st, id, "approved");
            let _ = st.db.lock().unwrap().execute(
                "INSERT INTO approvals(account,category,n) VALUES(?1,?2,1)
                 ON CONFLICT(account,category) DO UPDATE SET n=n+1",
                [&email, &cat],
            );
        }
        Err(_) => set_status(&st, id, "failed"),
    }
    r.map(|_| ())
}

#[tauri::command]
pub async fn approve(app: AppHandle, ids: Vec<i64>) -> Result<Vec<String>, String> {
    let mut errs = vec![];
    for id in ids {
        if let Err(e) = approve_one(&app, id).await {
            errs.push(e);
        }
    }
    let _ = app.emit("proposals_changed", ());
    let _ = app.emit("actions_changed", ());
    Ok(errs)
}

#[tauri::command]
pub fn reject(app: AppHandle, ids: Vec<i64>) -> Result<(), String> {
    let st = app.state::<St>();
    for id in ids {
        set_status(&st, id, "rejected");
    }
    let _ = app.emit("proposals_changed", ());
    Ok(())
}

#[tauri::command]
pub fn pending_ids(st: State<'_, St>, category: String, account: Option<String>) -> Result<Vec<i64>, String> {
    let db = st.db.lock().unwrap();
    let mut q = db
        .prepare(
            "SELECT p.id FROM proposals p JOIN messages m ON m.id=p.msg_id
             WHERE p.status='pending' AND m.state='inbox' AND p.category=?1 AND (?2 IS NULL OR p.account=?2)",
        )
        .map_err(err)?;
    let rows = q.query_map(params![category, account], |r| r.get(0)).map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}
