use crate::{err, St};
use rusqlite::{params, Connection, Row};
use serde::Serialize;
use tauri::State;

#[derive(Serialize, Clone, Default)]
pub struct Msg {
    pub id: String,
    pub account: String,
    pub thread_id: String,
    pub subject: String,
    pub sender: String,
    pub snippet: String,
    pub date: i64,
    pub labels: Vec<String>,
    pub list_unsub: Option<String>,
    pub list_unsub_post: Option<String>,
    pub body: String,
    pub unread: bool,
    pub state: String,
    pub category: Option<String>,
    pub confidence: Option<f64>,
    pub reason: Option<String>,
    pub summary: Option<String>,
}

pub fn state_of(labels: &[String]) -> &'static str {
    let has = |l: &str| labels.iter().any(|x| x == l);
    if has("TRASH") {
        "trash"
    } else if has("SPAM") {
        "spam"
    } else if has("INBOX") {
        "inbox"
    } else {
        "archived"
    }
}

pub fn upsert(db: &Connection, m: &Msg) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO messages(id,account,thread_id,subject,sender,snippet,date,labels,list_unsub,list_unsub_post,body,unread,state)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(id) DO UPDATE SET labels=excluded.labels, unread=excluded.unread, state=excluded.state",
        params![
            m.id,
            m.account,
            m.thread_id,
            m.subject,
            m.sender,
            m.snippet,
            m.date,
            m.labels.join(","),
            m.list_unsub,
            m.list_unsub_post,
            m.body,
            m.unread as i32,
            m.state
        ],
    )?;
    Ok(())
}

pub fn set_labels(db: &Connection, id: &str, labels: &[String]) -> rusqlite::Result<()> {
    db.execute(
        "UPDATE messages SET labels=?1, unread=?2, state=?3 WHERE id=?4",
        params![labels.join(","), labels.iter().any(|l| l == "UNREAD") as i32, state_of(labels), id],
    )?;
    Ok(())
}

pub fn known(db: &Connection, account: &str) -> std::collections::HashSet<String> {
    let mut s = match db.prepare("SELECT id FROM messages WHERE account=?1") {
        Ok(s) => s,
        Err(_) => return Default::default(),
    };
    s.query_map([account], |r| r.get::<_, String>(0))
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

const COLS: &str = "id,account,thread_id,subject,sender,snippet,date,labels,list_unsub,list_unsub_post,unread,state,category,confidence,reason,summary";

fn row(r: &Row) -> rusqlite::Result<Msg> {
    let labels: String = r.get::<_, Option<String>>(7)?.unwrap_or_default();
    Ok(Msg {
        id: r.get(0)?,
        account: r.get(1)?,
        thread_id: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        subject: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        sender: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        snippet: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        date: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
        labels: labels.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
        list_unsub: r.get(8)?,
        list_unsub_post: r.get(9)?,
        body: String::new(),
        unread: r.get::<_, Option<i32>>(10)?.unwrap_or(0) != 0,
        state: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        category: r.get(12)?,
        confidence: r.get(13)?,
        reason: r.get(14)?,
        summary: r.get(15)?,
    })
}

pub fn get(db: &Connection, id: &str) -> rusqlite::Result<Option<Msg>> {
    let mut q = db.prepare(&format!("SELECT {COLS},body FROM messages WHERE id=?1"))?;
    let mut rows = q.query([id])?;
    match rows.next()? {
        Some(r) => {
            let mut m = row(r)?;
            m.body = r.get::<_, Option<String>>(16)?.unwrap_or_default();
            Ok(Some(m))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn list_messages(
    st: State<'_, St>,
    account: Option<String>,
    category: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<Msg>, String> {
    let db = st.db.lock().unwrap();
    let mut q = db
        .prepare(&format!(
            "SELECT {COLS} FROM messages WHERE state='inbox' AND (?1 IS NULL OR account=?1) AND (?2 IS NULL OR category=?2)
             ORDER BY date DESC LIMIT ?3 OFFSET ?4"
        ))
        .map_err(err)?;
    let rows = q.query_map(params![account, category, limit, offset], row).map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}
