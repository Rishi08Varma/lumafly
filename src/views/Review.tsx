import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Act, approve, CATS, fmtDate, fromName, listActions, listProposals, Prop, reject, short, undoAction, unsubscribe } from "../api";

export default function Review() {
  const [props, setProps] = useState<Prop[]>([]);
  const [acts, setActs] = useState<Act[]>([]);
  const [busy, setBusy] = useState<Set<number>>(new Set());
  const [msg, setMsg] = useState("");

  const load = useCallback(async () => {
    setProps(await listProposals());
    setActs(await listActions(50));
  }, []);

  useEffect(() => { load().catch(() => {}); }, [load]);
  useEffect(() => {
    const u = [
      listen("proposals_changed", load),
      listen("actions_changed", load),
      listen("sync_done", load),
      listen<{ email: string; done: number; total: number }>("approve_progress", (e) => setMsg(`${short(e.payload.email)}: ${e.payload.done}/${e.payload.total} done`)),
    ];
    return () => { u.forEach((p) => p.then((f) => f())); };
  }, [load]);

  const mark = (ids: number[], on: boolean) =>
    setBusy((b) => { const n = new Set(b); ids.forEach((i) => (on ? n.add(i) : n.delete(i))); return n; });

  const ok = async (ids: number[]) => {
    mark(ids, true);
    setMsg("");
    try {
      const errs = await approve(ids);
      setMsg(errs.length ? errs[0] : "");
    } catch (e) { setMsg(String(e)); }
    mark(ids, false);
  };

  const no = async (ids: number[]) => {
    try { await reject(ids); } catch (e) { setMsg(String(e)); }
  };

  const unwanted = async (p: Prop) => {
    if (!confirm(`Unsubscribe from ${fromName(p.sender)} and archive this message?`)) return;
    mark([p.id], true);
    setMsg("");
    try {
      await reject([p.id]);
      setMsg(await unsubscribe(p.msg_id));
    } catch (e) { setMsg(String(e)); }
    mark([p.id], false);
  };

  const undo = async (id: number) => {
    mark([id], true);
    try { await undoAction(id); } catch (e) { setMsg(String(e)); }
    mark([id], false);
  };

  const groups = CATS.map((c) => ({ cat: c, items: props.filter((p) => p.category === c) })).filter((g) => g.items.length);

  return (
    <div className="page wide">
      <div className="bar">
        <h1>Review</h1>
        <span className="grow" />
        <span className={msg.startsWith("Could not") || msg.includes("error") ? "hint err" : "hint"}>{msg}</span>
        {props.length > 0 && (
          <button className="primary" disabled={busy.size > 0} onClick={() => ok(props.map((p) => p.id))}>
            Approve all ({props.length})
          </button>
        )}
      </div>
      {groups.length === 0 && <p className="hint">Nothing to review. New proposals appear here as mail is classified.</p>}
      {groups.map((g) => (
        <section key={g.cat}>
          <div className="bar">
            <h2><span className={`cat ${g.cat}`}>{g.cat}</span> {g.items.length}</h2>
            <span className="hint">{g.items[0].text}</span>
            <span className="grow" />
            <button disabled={busy.size > 0} onClick={() => ok(g.items.map((p) => p.id))}>Approve all</button>
            <button disabled={busy.size > 0} onClick={() => no(g.items.map((p) => p.id))}>Reject all</button>
          </div>
          <ul className="list">
            {g.items.map((p) => (
              <li key={p.id} className="row prow">
                <div className="grow min">
                  <div className="meta">
                    <span className="badge">{short(p.account)}</span>
                    <span className="from">{fromName(p.sender)}</span>
                    {p.confidence != null && <span className="hint">{Math.round(p.confidence * 100)}%</span>}
                    <span className="date">{fmtDate(p.created)}</span>
                  </div>
                  <div className="subj">{p.subject}</div>
                  <div className="sum">{p.summary}</div>
                </div>
                <button className="primary" disabled={busy.has(p.id)} onClick={() => ok([p.id])}>Approve</button>
                <button disabled={busy.has(p.id)} onClick={() => no([p.id])}>Reject</button>
                {(p.category === "newsletter" || p.category === "promotion") && (
                  <button disabled={busy.has(p.id)} onClick={() => unwanted(p)}>Unwanted</button>
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
      <section>
        <h2>Recent actions</h2>
        {acts.length === 0 && <p className="hint">No actions yet.</p>}
        <ul className="list">
          {acts.map((a) => (
            <li key={a.id} className={`row prow${a.undone ? " dim" : ""}`}>
              <div className="grow min">
                <div className="meta">
                  <span className="badge">{short(a.account)}</span>
                  <span className={a.result === "ok" ? "ok" : "hint err"}>{a.text}</span>
                  <span className="hint">{a.detail.source}</span>
                  <span className="date">{fmtDate(a.created)}</span>
                </div>
                <div className="subj">{a.subject}</div>
                {a.result !== "ok" && <div className="hint err">{a.result}</div>}
              </div>
              {a.undone ? <span className="hint">undone</span> : a.result === "ok" && (
                <button disabled={busy.has(a.id)} onClick={() => undo(a.id)}>Undo</button>
              )}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
