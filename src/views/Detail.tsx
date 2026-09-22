import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { act, canUnsub, fmtDate, getMessage, gmailUrl, Msg, reclassify, unsubscribe } from "../api";

export default function Detail({ msg, onClose, onNote }: { msg: Msg; onClose: () => void; onNote: (s: string) => void }) {
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => {
    setBody("");
    getMessage(msg.id).then((m) => setBody(m?.body ?? "")).catch(() => {});
  }, [msg.id]);

  const redo = async () => {
    setBusy(true);
    setErr("");
    try { await reclassify(msg.id); } catch (e) { setErr(String(e)); }
    setBusy(false);
  };

  const doAct = async (a: "archive" | "trash" | "spam") => {
    if (a !== "archive" && !confirm(`Move this message to ${a === "spam" ? "Spam" : "Trash"}? You can undo it from Review.`)) return;
    setBusy(true);
    setErr("");
    try { await act(msg.id, a); onClose(); } catch (e) { setErr(String(e)); }
    setBusy(false);
  };

  const unsub = async () => {
    if (!confirm("Unsubscribe from this sender and archive the message? Lumafly tries one-click first, then the unsubscribe email, then opens the link in your browser.")) return;
    setBusy(true);
    setErr("");
    try { onNote(await unsubscribe(msg.id)); onClose(); } catch (e) { setErr(String(e)); }
    setBusy(false);
  };

  return (
    <div className="detail">
      <div className="dhead">
        <button className="chip" onClick={onClose}>Close</button>
        <span className="grow" />
        <button className="chip" onClick={() => openUrl(gmailUrl(msg))}>Open in Gmail</button>
        <button className="chip" onClick={redo} disabled={busy}>{busy ? "Thinking…" : "Reclassify"}</button>
      </div>
      <div className="dhead">
        <button className="chip" disabled={busy} onClick={() => doAct("archive")}>Archive</button>
        <button className="chip" disabled={busy} onClick={() => doAct("trash")}>Trash</button>
        <button className="chip" disabled={busy} onClick={() => doAct("spam")}>Spam</button>
        {canUnsub(msg) && <button className="chip" disabled={busy} onClick={unsub}>{busy ? "Working…" : "Unsubscribe"}</button>}
      </div>
      <h2 className="dsubj">{msg.subject}</h2>
      <div className="hint">{msg.sender} · {fmtDate(msg.date)} · {msg.account}</div>
      <div className="card ai">
        <div className="meta">
          {msg.category ? <span className={`cat ${msg.category}`}>{msg.category}</span> : <span className="cat wait">not classified yet</span>}
          {msg.confidence != null && <span className="hint">{Math.round(msg.confidence * 100)}% confident</span>}
        </div>
        {msg.summary && <p>{msg.summary}</p>}
        {msg.reason && <div className="hint">Why: {msg.reason}</div>}
        {err && <div className="hint err">{err}</div>}
      </div>
      <pre className="body">{body || msg.snippet}</pre>
    </div>
  );
}
