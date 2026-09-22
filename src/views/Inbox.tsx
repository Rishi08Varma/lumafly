import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Acct, CATS, classifyNow, classifyPending, fmtDate, fromName, listAccounts, listMessages, Msg, short, syncNow } from "../api";
import Detail from "./Detail";

type Prog = { email: string; done: number; total: number };
type Classified = Pick<Msg, "id" | "category" | "confidence" | "reason" | "summary">;

export default function Inbox() {
  const [accts, setAccts] = useState<Acct[]>([]);
  const [acct, setAcct] = useState<string | null>(null);
  const [cat, setCat] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [sel, setSel] = useState<string | null>(null);
  const [prog, setProg] = useState("");
  const [pending, setPending] = useState(0);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setAccts(await listAccounts());
    setMsgs(await listMessages(acct, cat, 500));
    setPending(await classifyPending());
  }, [acct, cat]);

  useEffect(() => { load().catch(() => {}); }, [load]);

  useEffect(() => {
    const u = [
      listen<string>("sync_start", (e) => setProg(`${short(e.payload)}: syncing…`)),
      listen<Prog>("sync_progress", (e) => setProg(`${short(e.payload.email)}: ${e.payload.done}/${e.payload.total}`)),
      listen<{ email: string; new: number }>("sync_done", (e) => { setProg(e.payload.new ? `${short(e.payload.email)}: ${e.payload.new} new` : ""); load(); }),
      listen<{ email: string; error: string }>("sync_error", (e) => setProg(`${short(e.payload.email)}: ${e.payload.error}`)),
      listen<number>("classify_progress", (e) => setPending(e.payload)),
      listen<string>("classify_error", (e) => setProg(`AI: ${e.payload}`)),
      listen<string>("message_changed", () => load()),
      listen<Classified>("classified", (e) => {
        const c = e.payload;
        setMsgs((ms) => ms.map((m) => (m.id === c.id ? { ...m, ...c } : m)));
      }),
    ];
    return () => { u.forEach((p) => p.then((f) => f())); };
  }, [load]);

  const sync = async () => {
    setBusy(true);
    try { await syncNow(acct ?? undefined); } catch (e) { setProg(String(e)); }
    setBusy(false);
  };

  const cur = msgs.find((m) => m.id === sel) ?? null;

  return (
    <div className="inbox">
      <div className="toolbar">
        <button className={acct === null ? "chip on" : "chip"} onClick={() => setAcct(null)}>All</button>
        {accts.map((a) => (
          <button key={a.email} className={acct === a.email ? "chip on" : "chip"} onClick={() => setAcct(a.email)}>
            {short(a.email)} <span className="n">{a.count}</span>
          </button>
        ))}
        <span className="grow" />
        <span className="hint">{prog}</span>
        {pending > 0 && <button className="chip" onClick={classifyNow} title="Click to resume if stalled">classifying {pending}…</button>}
        <button onClick={sync} disabled={busy || accts.length === 0}>Sync now</button>
      </div>
      <div className="toolbar sub">
        <button className={cat === null ? "chip on" : "chip"} onClick={() => setCat(null)}>Any category</button>
        {CATS.map((c) => (
          <button key={c} className={cat === c ? `chip on cat ${c}` : `chip cat ${c}`} onClick={() => setCat(c)}>{c}</button>
        ))}
      </div>
      <div className="split">
        <div className="pane">
          {accts.length === 0 && <div className="empty"><p>Add a Gmail account in Settings to get started.</p></div>}
          {accts.length > 0 && msgs.length === 0 && <div className="empty"><p>Nothing here.</p></div>}
          <ul className="list">
            {msgs.map((m) => (
              <li key={m.id} className={`row${m.unread ? " unread" : ""}${m.id === sel ? " sel" : ""}`} onClick={() => setSel(m.id)}>
                <div className="meta">
                  <span className={`badge a${accts.findIndex((a) => a.email === m.account) % 3}`}>{short(m.account)}</span>
                  {m.category ? <span className={`cat ${m.category}`}>{m.category}</span> : <span className="cat wait">…</span>}
                  <span className="from">{fromName(m.sender)}</span>
                  <span className="date">{fmtDate(m.date)}</span>
                </div>
                <div className="subj">{m.subject}</div>
                <div className="sum">{m.summary ?? m.snippet}</div>
              </li>
            ))}
          </ul>
        </div>
        {cur && <Detail msg={cur} onClose={() => setSel(null)} onNote={setProg} />}
      </div>
    </div>
  );
}
