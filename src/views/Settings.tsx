import { useEffect, useState } from "react";
import { Acct, Approved, approvalCounts, CATS, classifyNow, ensureOllama, getSettings, hasClientSecret, listAccounts, Probe, saveSettings, setClientSecret, Settings as S, testOllama, UNLOCK } from "../api";
import Accounts from "./Accounts";

export default function Settings() {
  const [s, setS] = useState<S | null>(null);
  const [secret, setSecret] = useState("");
  const [hasSecret, setHasSecret] = useState(false);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [accts, setAccts] = useState<Acct[]>([]);
  const [appr, setAppr] = useState<Approved[]>([]);

  useEffect(() => {
    getSettings().then(setS);
    hasClientSecret().then(setHasSecret);
    listAccounts().then(setAccts).catch(() => {});
    approvalCounts().then(setAppr).catch(() => {});
  }, []);

  if (!s) return null;

  const set = (p: Partial<S>) => setS({ ...s, ...p });

  const save = async () => {
    setBusy(true);
    setMsg("");
    try {
      await saveSettings(s);
      if (secret) {
        await setClientSecret(secret);
        setSecret("");
        setHasSecret(true);
      }
      ensureOllama().then(() => classifyNow()).catch(() => {});
      setMsg("Saved.");
    } catch (e) {
      setMsg(String(e));
    }
    setBusy(false);
  };

  const test = async () => {
    setBusy(true);
    setProbe(null);
    try {
      await ensureOllama().catch(() => {});
      setProbe(await testOllama(s.ollama_url, s.model));
    } catch (e) {
      setProbe({ ok: false, version: "", models: [], has_model: false, error: String(e) });
    }
    setBusy(false);
  };

  const local = /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/.test(s.ollama_url);
  const approved = (a: string, c: string) => appr.find((x) => x.account === a && x.category === c)?.n ?? 0;
  const toggleAuto = (a: string, c: string, on: boolean) => {
    const cur = s.auto[a] ?? [];
    const next = on ? [...new Set([...cur, c])] : cur.filter((x) => x !== c);
    set({ auto: { ...s.auto, [a]: next } });
  };

  return (
    <div className="page">
      <h1>Settings</h1>

      <section>
        <h2>Google OAuth client</h2>
        <p className="hint">From Google Cloud Console. See SETUP.md.</p>
        <label>
          Client ID
          <input value={s.client_id} onChange={(e) => set({ client_id: e.target.value })} placeholder="xxxx.apps.googleusercontent.com" />
        </label>
        <label>
          Client secret {hasSecret && <span className="ok">stored in keychain</span>}
          <input type="password" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={hasSecret ? "leave blank to keep" : "GOCSPX-..."} />
        </label>
      </section>

      <Accounts ready={!!s.client_id && hasSecret} />

      <section>
        <h2>Ollama</h2>
        <label>
          Base URL
          <input value={s.ollama_url} onChange={(e) => set({ ollama_url: e.target.value })} />
        </label>
        <label>
          Model
          <input value={s.model} onChange={(e) => set({ model: e.target.value })} />
        </label>
        <p className="hint">{local ? "Local URL: Lumafly starts and stops ollama serve itself." : "Remote URL: Lumafly only sends requests."}</p>
        <button onClick={test} disabled={busy}>Test connection</button>
        {probe && (
          <div className={probe.ok ? "card ok" : "card err"}>
            {probe.ok ? (
              <>
                <div>Connected to Ollama {probe.version}</div>
                <div>{probe.has_model ? `Model ${s.model} is available.` : `Model ${s.model} not found. Run: ollama pull ${s.model}`}</div>
                {probe.models.length > 0 && <div className="hint">Installed: {probe.models.join(", ")}</div>}
              </>
            ) : (
              <>
                <div>Cannot reach {s.ollama_url}</div>
                <div className="hint">{probe.error}</div>
                {!local && <div className="hint">On the host PC make sure OLLAMA_HOST=0.0.0.0 is set and port 11434 is open in the firewall.</div>}
              </>
            )}
          </div>
        )}
      </section>

      <section>
        <h2>Sync</h2>
        <label>
          Poll interval (minutes)
          <input type="number" min={1} value={s.poll_min} onChange={(e) => set({ poll_min: +e.target.value || 1 })} />
        </label>
        <label>
          Initial sync window (days)
          <input type="number" min={1} value={s.sync_days} onChange={(e) => set({ sync_days: +e.target.value || 1 })} />
        </label>
        <label className="row">
          <input type="checkbox" checked={s.notify} onChange={(e) => set({ notify: e.target.checked })} />
          Native notifications
        </label>
      </section>

      <section>
        <h2>Auto mode</h2>
        <p className="hint">
          When on, Lumafly applies that category's default action without asking and notifies you. Each toggle unlocks after {UNLOCK} approved
          proposals for that account and category. Spam still goes to Spam, never to Trash, and nothing is ever deleted.
        </p>
        {accts.length === 0 && <p className="hint">No accounts yet.</p>}
        {accts.map((a) => (
          <div key={a.email} className="card">
            <div>{a.email}</div>
            <div className="chips">
              {CATS.map((c) => {
                const n = approved(a.email, c);
                const ok = n >= UNLOCK;
                return (
                  <label key={c} className="row" title={ok ? "Unlocked" : `${n}/${UNLOCK} approved`}>
                    <input
                      type="checkbox"
                      checked={s.auto[a.email]?.includes(c) ?? false}
                      disabled={!ok}
                      onChange={(e) => toggleAuto(a.email, c, e.target.checked)}
                    />
                    {c} <span className="hint">{ok ? "" : `${n}/${UNLOCK}`}</span>
                  </label>
                );
              })}
            </div>
          </div>
        ))}
      </section>

      <div className="bar">
        <button className="primary" onClick={save} disabled={busy}>Save</button>
        <span className="hint">{msg}</span>
      </div>
    </div>
  );
}
