import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Acct, addAccount, fmtDate, listAccounts, reauthAccount, removeAccount, syncNow } from "../api";

export default function Accounts({ ready }: { ready: boolean }) {
  const [accts, setAccts] = useState<Acct[]>([]);
  const [busy, setBusy] = useState("");
  const [msg, setMsg] = useState("");

  const load = () => listAccounts().then(setAccts).catch(() => {});

  useEffect(() => {
    load();
    const u = [listen("sync_done", load), listen("account_needs_auth", load)];
    return () => { u.forEach((p) => p.then((f) => f())); };
  }, []);

  const run = async (label: string, f: () => Promise<unknown>) => {
    setBusy(label);
    setMsg("");
    try {
      await f();
    } catch (e) {
      setMsg(String(e));
    }
    setBusy("");
    load();
  };

  return (
    <section>
      <h2>Accounts</h2>
      {accts.length === 0 && <p className="hint">No Gmail accounts linked yet.</p>}
      {accts.length > 0 && <p className="hint">Sync fetches inbox mail from the sync window below. Widen the window, Save, then Resync to pull older mail.</p>}
      {accts.map((a) => (
        <div key={a.email} className={a.needs_auth ? "card err acct" : "card acct"}>
          <div className="grow">
            <div>{a.email}</div>
            <div className="hint">
              {a.needs_auth
                ? "Sign-in expired. Re-authenticate to resume syncing."
                : `${a.count} synced${a.inbox_total != null ? ` of ${a.inbox_total} in Gmail inbox` : ""}${a.last_sync ? ` · synced ${fmtDate(a.last_sync)}` : " · not synced yet"}`}
            </div>
          </div>
          {a.needs_auth ? (
            <button className="primary" disabled={!!busy} onClick={() => run(a.email, () => reauthAccount(a.email))}>
              {busy === a.email ? "Waiting for browser…" : "Re-authenticate"}
            </button>
          ) : (
            <>
              <button disabled={!!busy} onClick={() => run("sync" + a.email, () => syncNow(a.email))}>Sync</button>
              <button disabled={!!busy} title="Fetch everything in the sync window again" onClick={() => run("full" + a.email, () => syncNow(a.email, true))}>
                {busy === "full" + a.email ? "Fetching…" : "Resync"}
              </button>
            </>
          )}
          <button
            disabled={!!busy}
            onClick={() => confirm(`Remove ${a.email} from Lumafly? Nothing in Gmail is changed.`) && run("rm", () => removeAccount(a.email))}
          >
            Remove
          </button>
        </div>
      ))}
      <div className="bar">
        <button className="primary" disabled={!ready || !!busy} onClick={() => run("add", addAccount)}>
          {busy === "add" ? "Complete sign-in in your browser…" : "Add Gmail account"}
        </button>
        {!ready && <span className="hint">Save a client ID and secret first.</span>}
        {msg && <span className="hint err">{msg}</span>}
      </div>
    </section>
  );
}
