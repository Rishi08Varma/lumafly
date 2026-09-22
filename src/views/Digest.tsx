import { useEffect, useState } from "react";
import { CATS, Digest as D, lastDigest, makeDigest, short } from "../api";

const spans = [
  [24, "24 hours"],
  [48, "2 days"],
  [168, "7 days"],
] as const;

export default function Digest() {
  const [d, setD] = useState<D | null>(null);
  const [hours, setHours] = useState(24);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => { lastDigest().then(setD).catch(() => {}); }, []);

  const gen = async () => {
    setBusy(true);
    setErr("");
    try { setD(await makeDigest(hours)); } catch (e) { setErr(String(e)); }
    setBusy(false);
  };

  const accts = d ? [...new Set(d.counts.map((c) => c.account))].sort() : [];
  const n = (cat: string, acct: string) => d?.counts.find((c) => c.category === cat && c.account === acct)?.n ?? 0;
  const rowTotal = (cat: string) => accts.reduce((s, a) => s + n(cat, a), 0);

  return (
    <div className="page wide">
      <div className="bar">
        <h1>Digest</h1>
        <span className="grow" />
        <select value={hours} onChange={(e) => setHours(+e.target.value)}>
          {spans.map(([h, l]) => <option key={h} value={h}>Last {l}</option>)}
        </select>
        <button className="primary" onClick={gen} disabled={busy}>{busy ? "Writing…" : "Generate"}</button>
      </div>
      {err && <p className="hint err">{err}</p>}
      {!d && !busy && <p className="hint">No digest yet. Generate one to get a summary across all accounts.</p>}
      {d && (
        <>
          <p className="hint">
            Generated {new Date(d.created * 1000).toLocaleString()} · last {d.hours} hours · {d.total} emails
          </p>
          {d.total === 0 && <section><p>Nothing arrived in that window.</p></section>}
          {d.overview && <section><h2>Overview</h2><p>{d.overview}</p></section>}
          {d.action_items?.length > 0 && (
            <section>
              <h2>Needs you</h2>
              <ol className="items">
                {d.action_items.map((a, i) => (
                  <li key={i}><strong>{a.title}</strong><div className="hint">{a.detail}</div></li>
                ))}
              </ol>
            </section>
          )}
          {d.notable?.length > 0 && (
            <section>
              <h2>Worth knowing</h2>
              <ul className="items">{d.notable.map((s, i) => <li key={i}>{s}</li>)}</ul>
            </section>
          )}
          {d.total > 0 && (
            <section>
              <h2>By category</h2>
              <table className="counts">
                <thead>
                  <tr><th /> {accts.map((a) => <th key={a}>{short(a)}</th>)}<th>Total</th></tr>
                </thead>
                <tbody>
                  {[...CATS, "unclassified"].filter((c) => rowTotal(c) > 0).map((c) => (
                    <tr key={c}>
                      <td><span className={`cat ${c}`}>{c}</span></td>
                      {accts.map((a) => <td key={a}>{n(c, a) || ""}</td>)}
                      <td>{rowTotal(c)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </section>
          )}
        </>
      )}
    </div>
  );
}
