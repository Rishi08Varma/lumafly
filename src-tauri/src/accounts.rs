use crate::{err, oauth, settings, sync, St};
use rusqlite::Connection;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

#[derive(Serialize, Clone)]
pub struct Acct {
    pub email: String,
    pub last_sync: Option<i64>,
    pub needs_auth: bool,
    pub count: i64,
}

pub fn list(db: &Connection) -> Result<Vec<Acct>, String> {
    let mut q = db
        .prepare(
            "SELECT a.email, a.last_sync, a.needs_auth,
              (SELECT COUNT(*) FROM messages m WHERE m.account=a.email AND m.state='inbox')
             FROM accounts a ORDER BY a.email",
        )
        .map_err(err)?;
    let rows = q
        .query_map([], |r| {
            Ok(Acct {
                email: r.get(0)?,
                last_sync: r.get(1)?,
                needs_auth: r.get::<_, Option<i32>>(2)?.unwrap_or(0) != 0,
                count: r.get(3)?,
            })
        })
        .map_err(err)?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn emails(db: &Connection) -> Vec<String> {
    list(db).map(|a| a.into_iter().filter(|a| !a.needs_auth).map(|a| a.email).collect()).unwrap_or_default()
}

#[tauri::command]
pub fn list_accounts(st: State<'_, St>) -> Result<Vec<Acct>, String> {
    list(&st.db.lock().unwrap())
}

async fn link(app: &AppHandle, hint: Option<&str>) -> Result<Acct, String> {
    let (email, access, refresh) = oauth::flow(app, hint).await?;
    if let Some(h) = hint {
        if h != email {
            return Err(format!("signed in as {email} but expected {h}"));
        }
    }
    settings::secret_set(&oauth::rt_key(&email), &refresh)?;
    let st = app.state::<St>();
    st.tokens
        .lock()
        .unwrap()
        .insert(email.clone(), (access, Instant::now() + Duration::from_secs(3500)));
    let a = {
        let db = st.db.lock().unwrap();
        db.execute(
            "INSERT INTO accounts(email,needs_auth) VALUES(?1,0) ON CONFLICT(email) DO UPDATE SET needs_auth=0",
            [&email],
        )
        .map_err(err)?;
        list(&db)?.into_iter().find(|a| a.email == email)
    };
    let (h, e) = (app.clone(), email.clone());
    tauri::async_runtime::spawn(async move {
        let _ = sync::sync(&h, &e, false).await;
    });
    a.ok_or_else(|| "account missing after link".to_string())
}

#[tauri::command]
pub async fn add_account(app: AppHandle) -> Result<Acct, String> {
    link(&app, None).await
}

#[tauri::command]
pub async fn reauth_account(app: AppHandle, email: String) -> Result<Acct, String> {
    link(&app, Some(&email)).await
}

#[tauri::command]
pub async fn remove_account(app: AppHandle, email: String) -> Result<(), String> {
    if let Some(rt) = settings::secret_get(&oauth::rt_key(&email)) {
        oauth::revoke(&rt).await;
    }
    settings::secret_del(&oauth::rt_key(&email));
    let st = app.state::<St>();
    st.tokens.lock().unwrap().remove(&email);
    let db = st.db.lock().unwrap();
    for t in ["messages", "proposals", "approvals"] {
        db.execute(&format!("DELETE FROM {t} WHERE account=?1"), [&email]).map_err(err)?;
    }
    db.execute("DELETE FROM accounts WHERE email=?1", [&email]).map_err(err)?;
    Ok(())
}
