import { useEffect, useState } from "react";
import { applyRules, deleteRule, listRules, Rule, saveRule } from "../api";

const blank: Rule = { id: 0, sender: "", action: "trash", except: "", auto: false, note: "" };
const verbs = { trash: "Move to Trash", archive: "Archive", spam: "Move to Spam" };

export default function Rules() {
  const [rules, setRules] = useState<Rule[]>([]);
  const [r, setR] = useState<Rule>(blank);
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => listRules().then(setRules).catch(() => {});
  useEffect(() => { load(); }, []);

  const save = async () => {
    setBusy(true);
    setMsg("");
    try {
      await saveRule(r);
      setR(blank);
      await load();
      const n = await applyRules();
      setMsg(n ? `Applied to ${n} messages already in the inbox.` : "Saved. Applies to matching mail from now on.");
    } catch (e) { setMsg(String(e)); }
    setBusy(false);
  };

  const del = async (id: number) => {
    if (!confirm("Delete this rule?")) return;
    await deleteRule(id);
    load();
  };

  return (
    <section>
      <h2>Sender rules</h2>
      <p className="hint">
        Mail whose From line contains the sender text gets the action instead of AI classification, unless the subject or body contains one of the
        exception words. With "apply automatically" off, matches wait in Review.
      </p>
      {rules.map((x) => (
        <div key={x.id} className="card acct">
          <div className="grow">
            <div>{x.sender} <span className="hint">→ {verbs[x.action]}{x.auto ? " automatically" : " after review"}</span></div>
            {x.except && <div className="hint">unless it mentions: {x.except}</div>}
            {x.note && <div className="hint">{x.note}</div>}
          </div>
          <button onClick={() => setR(x)}>Edit</button>
          <button onClick={() => del(x.id)}>Delete</button>
        </div>
      ))}
      <div className="card">
        <label>
          Sender contains
          <input value={r.sender} onChange={(e) => setR({ ...r, sender: e.target.value })} placeholder="facebookmail.com" />
        </label>
        <label>
          Action
          <select value={r.action} onChange={(e) => setR({ ...r, action: e.target.value as Rule["action"] })}>
            <option value="trash">Move to Trash</option>
            <option value="archive">Archive</option>
            <option value="spam">Move to Spam</option>
          </select>
        </label>
        <label>
          Unless subject or body contains (comma separated)
          <input value={r.except} onChange={(e) => setR({ ...r, except: e.target.value })} placeholder="password, login, security" />
        </label>
        <label>
          Note
          <input value={r.note} onChange={(e) => setR({ ...r, note: e.target.value })} placeholder="why this rule exists" />
        </label>
        <label className="row">
          <input type="checkbox" checked={r.auto} onChange={(e) => setR({ ...r, auto: e.target.checked })} />
          Apply automatically
        </label>
        <div className="bar">
          <button className="primary" disabled={busy || !r.sender.trim()} onClick={save}>{r.id ? "Update rule" : "Add rule"}</button>
          {r.id > 0 && <button onClick={() => setR(blank)}>Cancel</button>}
          <span className="hint">{msg}</span>
        </div>
      </div>
    </section>
  );
}
