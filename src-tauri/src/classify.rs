use crate::{err, messages, ollama, text, St};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter, Manager, State};

pub const CATS: [&str; 6] = ["important", "personal", "newsletter", "promotion", "notification", "spam"];

const SYS: &str = "You triage a person's Gmail inbox. Classify each email into exactly one category:
- important: needs the person's attention or action soon. Bills due, security alerts, appointments, interviews, replies from real people about ongoing matters, deliveries needing action, deadlines.
- personal: written by a real person the user knows (friend, family, colleague) and not urgent.
- newsletter: recurring editorial content the user subscribed to: digests, blogs, substacks, community updates.
- promotion: marketing, sales, offers, coupons, product launches, upsells, surveys.
- notification: automated transactional mail: receipts, order and shipping updates, account activity, sign-in codes, social media alerts, calendar, app or service notices.
- spam: unsolicited junk, scams, phishing, fake invoices, bulk mail with no real relationship to the user.
confidence is 0 to 1. reason is under 15 words. summary is one or two plain sentences saying what the email says and any action needed, with no preamble.";

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "category": {"type": "string", "enum": CATS},
            "confidence": {"type": "number"},
            "reason": {"type": "string"},
            "summary": {"type": "string"}
        },
        "required": ["category", "confidence", "reason", "summary"]
    })
}

#[derive(Deserialize)]
struct Out {
    category: String,
    confidence: f64,
    reason: String,
    summary: String,
}

fn date_str(t: i64) -> String {
    let d = t / 86400;
    let (mut y, mut m, mut dd) = (1970, 1, 0);
    let mut days = d;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let n = if leap { 366 } else { 365 };
        if days < n {
            break;
        }
        days -= n;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let ml = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for l in ml {
        if days < l {
            dd = days + 1;
            break;
        }
        days -= l;
        m += 1;
    }
    format!("{y}-{m:02}-{dd:02}")
}

pub async fn classify_one(app: &AppHandle, id: &str) -> Result<Option<String>, String> {
    let st = app.state::<St>();
    let (url, model) = {
        let c = st.cfg.lock().unwrap();
        (c.ollama_url.clone(), c.model.clone())
    };
    let m = messages::get(&st.db.lock().unwrap(), id).map_err(err)?.ok_or("message not found")?;
    let rule = crate::rules::matching(&st.db.lock().unwrap(), &m);
    if let Some(r) = rule {
        let (cat, reason) = ("notification", format!("Rule: sender matches {}", r.sender));
        st.db
            .lock()
            .unwrap()
            .execute(
                "UPDATE messages SET category=?1, confidence=1.0, reason=?2, summary=?3 WHERE id=?4",
                params![cat, reason, m.subject, id],
            )
            .map_err(err)?;
        let _ = app.emit(
            "classified",
            json!({"id": id, "account": m.account, "category": cat, "confidence": 1.0, "reason": reason, "summary": m.subject}),
        );
        let m = messages::Msg { category: Some(cat.into()), ..m };
        return Ok(crate::rules::propose(app, &m, &r).await);
    }
    let user = format!(
        "Account: {}\nFrom: {}\nSubject: {}\nDate: {}\nHas List-Unsubscribe header: {}\n\n{}",
        m.account,
        m.sender,
        m.subject,
        date_str(m.date),
        if m.list_unsub.is_some() { "yes" } else { "no" },
        text::clip(&m.body, 14000)
    );
    let v = ollama::chat(&url, &model, SYS, &user, schema()).await?;
    let mut o: Out = serde_json::from_value(v).map_err(err)?;
    if !CATS.contains(&o.category.as_str()) {
        o.category = "notification".into();
    }
    o.confidence = o.confidence.clamp(0.0, 1.0);
    st.db
        .lock()
        .unwrap()
        .execute(
            "UPDATE messages SET category=?1, confidence=?2, reason=?3, summary=?4 WHERE id=?5",
            params![o.category, o.confidence, o.reason, o.summary, id],
        )
        .map_err(err)?;
    let _ = app.emit(
        "classified",
        json!({"id": id, "account": m.account, "category": o.category, "confidence": o.confidence, "reason": o.reason, "summary": o.summary}),
    );
    if o.category == "important" && m.date > crate::now() - 86400 && m.state == "inbox" {
        crate::notify::notify(app, &format!("Important: {}", m.subject), &o.summary);
    }
    Ok(crate::review::propose(app, id, &m.account, &o.category).await)
}

pub fn auto_summary(auto: &[String]) -> String {
    let mut counts: Vec<(String, usize)> = vec![];
    for a in auto {
        match counts.iter_mut().find(|(k, _)| k == a) {
            Some(c) => c.1 += 1,
            None => counts.push((a.clone(), 1)),
        }
    }
    counts.iter().map(|(k, n)| format!("{n}× {k}")).collect::<Vec<_>>().join(", ")
}

pub fn pending(st: &St) -> i64 {
    st.db
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM messages WHERE category IS NULL AND state='inbox'", [], |r| r.get(0))
        .unwrap_or(0)
}

pub fn kick(app: &AppHandle) {
    let st = app.state::<St>();
    {
        let mut g = st.classifying.lock().unwrap();
        if *g {
            return;
        }
        *g = true;
    }
    let h = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = ollama::ensure(&h).await;
        let mut skip: HashSet<String> = HashSet::new();
        let mut fails = 0;
        let mut auto: Vec<String> = vec![];
        loop {
            let st = h.state::<St>();
            let ids: Vec<String> = {
                let db = st.db.lock().unwrap();
                let mut q = match db.prepare("SELECT id FROM messages WHERE category IS NULL AND state='inbox' ORDER BY date DESC LIMIT 50") {
                    Ok(q) => q,
                    Err(_) => break,
                };
                q.query_map([], |r| r.get(0)).map(|it| it.filter_map(Result::ok).collect()).unwrap_or_default()
            };
            let Some(id) = ids.into_iter().find(|i| !skip.contains(i)) else { break };
            match classify_one(&h, &id).await {
                Ok(a) => {
                    fails = 0;
                    auto.extend(a);
                }
                Err(e) => {
                    skip.insert(id);
                    fails += 1;
                    let _ = h.emit("classify_error", &e);
                    if fails >= 3 {
                        break;
                    }
                }
            }
            let _ = h.emit("classify_progress", pending(&st));
        }
        auto.extend(crate::rules::apply_all(&h).await);
        auto.extend(crate::review::backfill(&h).await);
        if !auto.is_empty() {
            crate::notify::notify(&h, &format!("Lumafly handled {} emails", auto.len()), &auto_summary(&auto));
        }
        *h.state::<St>().classifying.lock().unwrap() = false;
    });
}

#[tauri::command]
pub fn classify_now(app: AppHandle) {
    kick(&app);
}

#[tauri::command]
pub fn classify_pending(st: State<'_, St>) -> i64 {
    pending(&st)
}

#[tauri::command]
pub async fn reclassify(app: AppHandle, id: String) -> Result<(), String> {
    classify_one(&app, &id).await.map(|_| ())
}

#[tauri::command]
pub fn get_message(st: State<'_, St>, id: String) -> Result<Option<messages::Msg>, String> {
    messages::get(&st.db.lock().unwrap(), &id).map_err(err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::date_str(0), "1970-01-01");
        assert_eq!(super::date_str(951782400), "2000-02-29");
        assert_eq!(super::date_str(1758499200), "2025-09-22");
    }
}
