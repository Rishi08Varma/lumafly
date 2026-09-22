import { useState } from "react";
import Settings from "./views/Settings";
import Stub from "./views/Stub";

const tabs = ["Inbox", "Review", "Digest", "Settings"] as const;
type Tab = (typeof tabs)[number];

export default function App() {
  const [tab, setTab] = useState<Tab>("Settings");
  return (
    <div className="app">
      <nav className="side">
        <div className="brand">Lumafly</div>
        {tabs.map((t) => (
          <button key={t} className={t === tab ? "nav on" : "nav"} onClick={() => setTab(t)}>
            {t}
          </button>
        ))}
      </nav>
      <main className="main">
        {tab === "Settings" ? <Settings /> : <Stub name={tab} />}
      </main>
    </div>
  );
}
