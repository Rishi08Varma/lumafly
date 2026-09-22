import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";
import { pendingCount } from "./api";
import Settings from "./views/Settings";
import Inbox from "./views/Inbox";
import Review from "./views/Review";
import Digest from "./views/Digest";

const tabs = ["Inbox", "Review", "Digest", "Settings"] as const;
type Tab = (typeof tabs)[number];

export default function App() {
  const [tab, setTab] = useState<Tab>("Inbox");
  const [n, setN] = useState(0);
  useEffect(() => {
    const load = () => pendingCount().then(setN).catch(() => {});
    load();
    isPermissionGranted().then((ok) => { if (!ok) requestPermission().catch(() => {}); }).catch(() => {});
    const u = [listen("proposals_changed", load), listen("message_changed", load), listen("sync_done", load)];
    return () => { u.forEach((p) => p.then((f) => f())); };
  }, []);
  return (
    <div className="app">
      <nav className="side">
        <div className="brand">Lumafly</div>
        {tabs.map((t) => (
          <button key={t} className={t === tab ? "nav on" : "nav"} onClick={() => setTab(t)}>
            {t}
            {t === "Review" && n > 0 && <span className="pill">{n}</span>}
          </button>
        ))}
      </nav>
      <main className="main">
        {tab === "Settings" ? <Settings /> : tab === "Inbox" ? <Inbox /> : tab === "Review" ? <Review /> : <Digest />}
      </main>
    </div>
  );
}
