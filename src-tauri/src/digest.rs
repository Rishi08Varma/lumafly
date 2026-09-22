use crate::{err, now, ollama, St};
use rusqlite::params;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

const SYS: &str = "You write a short daily email digest for one person across their Gmail accounts. Be concrete and brief.
overview: two or three sentences on what came in and the overall picture.
action_items: things the person must do or reply to, most urgent first. Each has a short title and one sentence of detail that names the account and any deadline.
notable: up to six one-line items worth knowing that need no action.
Skip spam and routine notifications unless they matter. Never invent details that are not in the input.";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "overview": {"type": "string"},
            "action_items": {"type": "array", "items": {"type": "object", "properties": {"title": {"type": "string"}, "detail": {"type": "string"}}, "required": ["title", "detail"]}},
            "notable": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["overview", "action_items", "notable"]
    })
}

#[tauri::command]
pub async fn make_digest(app: AppHandle, hours: u32) -> Result<Value, String> {
    let st = app.state::<St>();
    let since = now() - hours as i64 * 3600;
    let (rows, counts): (Vec<(String, String, String, String, String)>, Vec<Value>) = {
        let db = st.db.lock().unwrap();
        let mut q = db
            .prepare(
                "SELECT account,sender,subject,COALESCE(category,'unclassified'),COALESCE(summary,snippet,'')
                 FROM messages WHERE date>=?1 AND state!='trash'
                 ORDER BY CASE category WHEN 'important' THEN 0 WHEN 'personal' THEN 1 WHEN 'newsletter' THEN 2
                          WHEN 'promotion' THEN 3 WHEN 'notification' THEN 4 ELSE 5 END, date DESC LIMIT 200",
            )
            .map_err(err)?;
        let rows = q
            .query_map([since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(err)?
            .filter_map(Result::ok)
            .collect();
        let mut c = db
            .prepare("SELECT COALESCE(category,'unclassified'),account,COUNT(*) FROM messages WHERE date>=?1 AND state!='trash' GROUP BY 1,2")
            .map_err(err)?;
        let counts = c
            .query_map([since], |r| {
                Ok(json!({"category": r.get::<_, String>(0)?, "account": r.get::<_, String>(1)?, "n": r.get::<_, i64>(2)?}))
            })
            .map_err(err)?
            .filter_map(Result::ok)
            .collect();
        (rows, counts)
    };
    let total = rows.len();
    if rows.is_empty() {
        return Ok(json!({"created": now(), "hours": hours, "total": 0, "counts": counts, "overview": "", "action_items": [], "notable": []}));
    }
    let mut input = String::new();
    for (acct, from, subj, cat, sum) in &rows {
        let line = format!("[{cat}] {acct} | {from} | {subj} | {sum}\n");
        if input.len() + line.len() > 14000 {
            break;
        }
        input.push_str(&line);
    }
    let (url, model) = {
        let c = st.cfg.lock().unwrap();
        (c.ollama_url.clone(), c.model.clone())
    };
    let _ = ollama::ensure(&app).await;
    let v = ollama::chat(&url, &model, SYS, &format!("Emails from the last {hours} hours:\n\n{input}"), schema()).await?;
    let d = json!({
        "created": now(), "hours": hours, "total": total, "counts": counts,
        "overview": v["overview"], "action_items": v["action_items"], "notable": v["notable"]
    });
    st.db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO digests(created,hours,json) VALUES(?1,?2,?3)",
            params![now(), hours, d.to_string()],
        )
        .map_err(err)?;
    Ok(d)
}

#[tauri::command]
pub fn last_digest(st: State<'_, St>) -> Option<Value> {
    st.db
        .lock()
        .unwrap()
        .query_row("SELECT json FROM digests ORDER BY id DESC LIMIT 1", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}
