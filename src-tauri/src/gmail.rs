use crate::messages::{state_of, Msg};
use crate::{err, text};
use base64::alphabet::URL_SAFE;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const B64: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap()
}

pub fn sv(v: &Value) -> Option<String> {
    v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string()))
}

pub async fn get(tok: &str, path: &str, q: &[(&str, &str)]) -> Result<Value, String> {
    let url = reqwest::Url::parse_with_params(&format!("{API}/{path}"), q).map_err(err)?;
    let mut wait = 1;
    loop {
        let r = http().get(url.clone()).bearer_auth(tok).send().await.map_err(err)?;
        let s = r.status();
        if (s.as_u16() == 429 || s.is_server_error()) && wait <= 16 {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait *= 2;
            continue;
        }
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if !s.is_success() {
            return Err(format!("gmail {}: {}", s.as_u16(), v["error"]["message"].as_str().unwrap_or("request failed")));
        }
        return Ok(v);
    }
}

pub async fn profile(tok: &str) -> Result<(String, String), String> {
    let v = get(tok, "profile", &[]).await?;
    let email = v["emailAddress"].as_str().ok_or("profile has no email")?.to_string();
    Ok((email, sv(&v["historyId"]).unwrap_or_default()))
}

pub async fn list_ids(tok: &str, q: &str) -> Result<Vec<String>, String> {
    let mut ids = vec![];
    let mut pt = String::new();
    loop {
        let v = {
            let mut p = vec![("q", q), ("maxResults", "500")];
            if !pt.is_empty() {
                p.push(("pageToken", pt.as_str()));
            }
            get(tok, "messages", &p).await?
        };
        if let Some(a) = v["messages"].as_array() {
            ids.extend(a.iter().filter_map(|m| m["id"].as_str().map(String::from)));
        }
        match v["nextPageToken"].as_str() {
            Some(t) => pt = t.to_string(),
            None => return Ok(ids),
        }
    }
}

pub async fn fetch(tok: &str, account: &str, id: &str) -> Result<Msg, String> {
    let v = get(tok, &format!("messages/{id}"), &[("format", "full")]).await?;
    Ok(parse(&v, account))
}

pub async fn labels(tok: &str, id: &str) -> Result<Vec<String>, String> {
    let v = get(tok, &format!("messages/{id}"), &[("format", "minimal")]).await?;
    Ok(strs(&v["labelIds"]))
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn hdr<'a>(hs: &'a [Value], name: &str) -> Option<&'a str> {
    hs.iter()
        .find(|h| h["name"].as_str().map_or(false, |n| n.eq_ignore_ascii_case(name)))
        .and_then(|h| h["value"].as_str())
}

fn walk(p: &Value, txt: &mut String, html: &mut String) {
    let mime = p["mimeType"].as_str().unwrap_or("");
    if let Some(d) = p["body"]["data"].as_str() {
        if let Ok(b) = B64.decode(d) {
            let s = String::from_utf8_lossy(&b);
            if mime.starts_with("text/plain") {
                txt.push_str(&s);
            } else if mime.starts_with("text/html") {
                html.push_str(&s);
            }
        }
    }
    if let Some(parts) = p["parts"].as_array() {
        for q in parts {
            walk(q, txt, html);
        }
    }
}

pub fn parse(v: &Value, account: &str) -> Msg {
    let p = &v["payload"];
    let hs = p["headers"].as_array().cloned().unwrap_or_default();
    let labels = strs(&v["labelIds"]);
    let (mut txt, mut html) = (String::new(), String::new());
    walk(p, &mut txt, &mut html);
    let body = if txt.trim().is_empty() { text::html_to_text(&html) } else { text::squeeze(&txt) };
    Msg {
        id: v["id"].as_str().unwrap_or("").into(),
        account: account.into(),
        thread_id: v["threadId"].as_str().unwrap_or("").into(),
        subject: hdr(&hs, "Subject").unwrap_or("(no subject)").into(),
        sender: hdr(&hs, "From").unwrap_or("").into(),
        snippet: text::squeeze(&text::html_to_text(v["snippet"].as_str().unwrap_or(""))),
        date: sv(&v["internalDate"]).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) / 1000,
        list_unsub: hdr(&hs, "List-Unsubscribe").map(String::from),
        list_unsub_post: hdr(&hs, "List-Unsubscribe-Post").map(String::from),
        body: text::clip(&body, 20000),
        unread: labels.iter().any(|l| l == "UNREAD"),
        state: state_of(&labels).into(),
        labels,
        ..Default::default()
    }
}

#[derive(Default)]
pub struct Hist {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub deleted: Vec<String>,
    pub hid: String,
}

fn ids_in(v: &Value) -> impl Iterator<Item = String> + '_ {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["message"]["id"].as_str().map(String::from))
}

pub async fn history(tok: &str, start: &str) -> Result<Hist, String> {
    let mut h = Hist::default();
    let mut pt = String::new();
    loop {
        let v = {
            let mut p = vec![("startHistoryId", start), ("maxResults", "500")];
            if !pt.is_empty() {
                p.push(("pageToken", pt.as_str()));
            }
            get(tok, "history", &p).await?
        };
        for r in v["history"].as_array().into_iter().flatten() {
            h.added.extend(ids_in(&r["messagesAdded"]));
            h.deleted.extend(ids_in(&r["messagesDeleted"]));
            h.changed.extend(ids_in(&r["labelsAdded"]));
            h.changed.extend(ids_in(&r["labelsRemoved"]));
        }
        if let Some(x) = sv(&v["historyId"]) {
            h.hid = x;
        }
        match v["nextPageToken"].as_str() {
            Some(t) => pt = t.to_string(),
            None => return Ok(h),
        }
    }
}

pub async fn post(tok: &str, path: &str, body: &Value) -> Result<Value, String> {
    let mut wait = 1;
    loop {
        let r = http()
            .post(format!("{API}/{path}"))
            .bearer_auth(tok)
            .json(body)
            .send()
            .await
            .map_err(err)?;
        let s = r.status();
        if (s.as_u16() == 429 || s.is_server_error()) && wait <= 16 {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait *= 2;
            continue;
        }
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if !s.is_success() {
            return Err(format!("gmail {}: {}", s.as_u16(), v["error"]["message"].as_str().unwrap_or("request failed")));
        }
        return Ok(v);
    }
}

pub async fn modify(tok: &str, id: &str, add: &[String], remove: &[String]) -> Result<Vec<String>, String> {
    let v = post(
        tok,
        &format!("messages/{id}/modify"),
        &serde_json::json!({"addLabelIds": add, "removeLabelIds": remove}),
    )
    .await?;
    Ok(strs(&v["labelIds"]))
}

pub async fn labels_list(tok: &str) -> Result<Vec<(String, String)>, String> {
    let v = get(tok, "labels", &[]).await?;
    Ok(v["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| Some((l["name"].as_str()?.to_string(), l["id"].as_str()?.to_string())))
        .collect())
}

pub async fn label_create(tok: &str, name: &str) -> Result<String, String> {
    let v = post(
        tok,
        "labels",
        &serde_json::json!({"name": name, "labelListVisibility": "labelShow", "messageListVisibility": "show"}),
    )
    .await?;
    v["id"].as_str().map(String::from).ok_or_else(|| "label create returned no id".into())
}

pub async fn send(tok: &str, raw: &str) -> Result<(), String> {
    post(tok, "messages/send", &serde_json::json!({"raw": raw})).await.map(|_| ())
}

pub fn html_of(v: &Value) -> String {
    let (mut txt, mut html) = (String::new(), String::new());
    walk(&v["payload"], &mut txt, &mut html);
    html
}
