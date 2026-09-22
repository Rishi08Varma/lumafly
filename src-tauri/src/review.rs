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

pub const UNLOCK: i64 = 20;

pub fn approved(st: &St, account: &str, cat: &str) -> i64 {
    st.db
        .lock()
        .unwrap()
        .query_row("SELECT n FROM approvals WHERE account=?1 AND category=?2", [account, cat], |r| r.get(0))
        .unwrap_or(0)
}

fn auto_on(st: &St, account: &str, cat: &str) -> bool {
    st.cfg
        .lock()
        .unwrap()
        .auto
        .get(account)
        .map(|v| v.iter().any(|c| c == cat))
        .unwrap_or(false)
}

pub async fn propose(app: &AppHandle, msg_id: &str, account: &str, cat: &str) -> Option<String> {
    let Some(action) = default_action(cat) else { return None };
    let st = app.state::<St>();
    let auto = auto_on(&st, account, cat) && approved(&st, account, cat) >= UNLOCK;
    let pid = {
        let db = st.db.lock().unwrap();
        let dup: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM proposals WHERE msg_id=?1 AND status IN ('pending','approved','auto')",
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
            return None;
        }
        let _ = db.execute(
            "INSERT INTO proposals(msg_id,account,action,category,status,created) VALUES(?1,?2,?3,?4,?5,?6)",
            params![msg_id, account, action, cat, if auto { "auto" } else { "pending" }, now()],
        );
        db.last_insert_rowid()
    };
    if !auto {
        let _ = app.emit("proposals_changed", ());
        return None;
    }
    match actions::execute(app, account, msg_id, action, Some(cat), "auto").await {
        Ok(_) => {
            let _ = app.emit("actions_changed", ());
            Some(actions::describe(action, Some(cat)))
        }
        Err(_) => {
            set_status(&st, pid, "pending");
            let _ = app.emit("proposals_changed", ());
            None
        }
    }
}

#[derive(Serialize)]
pub struct Approved {
    pub account: String,
    pub category: String,
    pub n: i64,
}

#[tauri::command]
pub fn approval_counts(st: State<'_, St>) -> Result<Vec<Approved>, String> {
    let db = st.db.lock().unwrap();
    let mut q = db.prepare("SELECT account,category,n FROM approvals").map_err(err)?;
    let rows = q
        .query_map([], |r| Ok(Approved { account: r.get(0)?, category: r.get(1)?, n: r.get(2)? }))
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub async fn backfill(app: &AppHandle) -> Vec<String> {
    let rows: Vec<(String, String, String)> = {
        let st = app.state::<St>();
        let db = st.db.lock().unwrap();
        let Ok(mut q) = db.prepare(
            "SELECT m.id,m.account,m.category FROM messages m
             WHERE m.state='inbox' AND m.category IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM proposals p WHERE p.msg_id=m.id)
               AND NOT EXISTS(SELECT 1 FROM actions a WHERE a.msg_id=m.id AND a.result='ok' AND a.undone=0)",
        ) else { return vec![] };
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default()
    };
    let mut auto = vec![];
    for (id, acct, cat) in rows {
        if let Some(t) = propose(app, &id, &acct, &cat).await {
            auto.push(t);
        }
    }
    auto
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
