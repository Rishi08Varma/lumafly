import { invoke } from "@tauri-apps/api/core";

export type Settings = {
  ollama_url: string;
  model: string;
  poll_min: number;
  sync_days: number;
  client_id: string;
  notify: boolean;
  auto: Record<string, string[]>;
};

export type Probe = {
  ok: boolean;
  version: string;
  models: string[];
  has_model: boolean;
  error: string;
};

export const CATS = ["important", "personal", "newsletter", "promotion", "notification", "spam"] as const;
export type Cat = (typeof CATS)[number];

export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (s: Settings) => invoke<void>("save_settings", { s });
export const setClientSecret = (v: string) => invoke<void>("set_client_secret", { v });
export const hasClientSecret = () => invoke<boolean>("has_client_secret");
export const testOllama = (url: string, model: string) => invoke<Probe>("test_ollama", { url, model });
export const ensureOllama = () => invoke<boolean>("ensure_ollama");

export type Acct = { email: string; last_sync: number | null; needs_auth: boolean; count: number; inbox_total: number | null };

export type Msg = {
  id: string;
  account: string;
  thread_id: string;
  subject: string;
  sender: string;
  snippet: string;
  date: number;
  labels: string[];
  list_unsub: string | null;
  list_unsub_post: string | null;
  unread: boolean;
  state: string;
  category: Cat | null;
  confidence: number | null;
  reason: string | null;
  summary: string | null;
};

export const listAccounts = () => invoke<Acct[]>("list_accounts");
export const addAccount = () => invoke<Acct>("add_account");
export const reauthAccount = (email: string) => invoke<Acct>("reauth_account", { email });
export const removeAccount = (email: string) => invoke<void>("remove_account", { email });
export const syncNow = (email?: string, full?: boolean) => invoke<number>("sync_now", { email, full });
export const listMessages = (account: string | null, category: string | null, limit = 200, offset = 0) =>
  invoke<Msg[]>("list_messages", { account, category, limit, offset });
export const getMessage = (id: string) => invoke<(Msg & { body: string }) | null>("get_message", { id });
export const reclassify = (id: string) => invoke<void>("reclassify", { id });
export const classifyNow = () => invoke<void>("classify_now");
export const classifyPending = () => invoke<number>("classify_pending");
export const gmailUrl = (m: Msg) => `https://mail.google.com/mail/u/?authuser=${encodeURIComponent(m.account)}#all/${m.thread_id}`;

export const fmtDate = (s: number) => {
  const d = new Date(s * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (d.getFullYear() === now.getFullYear()) return d.toLocaleDateString([], { month: "short", day: "numeric" });
  return d.toLocaleDateString();
};

export const fromName = (s: string) => {
  const m = s.match(/^\s*"?([^"<]*?)"?\s*<[^>]+>\s*$/);
  return (m ? m[1] : s.replace(/<[^>]+>/g, "")).trim() || s;
};

export const short = (email: string) => email.split("@")[0];

export type Prop = {
  id: number;
  msg_id: string;
  account: string;
  action: string;
  category: Cat;
  created: number;
  subject: string;
  sender: string;
  summary: string | null;
  confidence: number | null;
  text: string;
};

export type Act = {
  id: number;
  msg_id: string;
  account: string;
  action: string;
  detail: { add: string[]; remove: string[]; category: string | null; source: string };
  result: string;
  undone: boolean;
  created: number;
  subject: string;
  sender: string;
  text: string;
};

export const listProposals = () => invoke<Prop[]>("list_proposals");
export const pendingCount = () => invoke<number>("pending_count");
export const pendingIds = (category: string, account?: string) => invoke<number[]>("pending_ids", { category, account });
export const approve = (ids: number[]) => invoke<string[]>("approve", { ids });
export const reject = (ids: number[]) => invoke<void>("reject", { ids });
export const listActions = (limit = 50) => invoke<Act[]>("list_actions", { limit });
export const undoAction = (id: number) => invoke<void>("undo_action", { id });
export const act = (id: string, action: "archive" | "trash" | "spam" | "label") => invoke<number>("act", { id, action });
export const unsubscribe = (id: string) => invoke<string>("unsubscribe", { id });
export const canUnsub = (m: Pick<Msg, "category" | "list_unsub">) =>
  m.category !== "spam" && (!!m.list_unsub || m.category === "newsletter" || m.category === "promotion");

export type Digest = {
  created: number;
  hours: number;
  total: number;
  counts: { category: string; account: string; n: number }[];
  overview: string;
  action_items: { title: string; detail: string }[];
  notable: string[];
};
export type Approved = { account: string; category: string; n: number };

export const makeDigest = (hours: number) => invoke<Digest>("make_digest", { hours });
export const lastDigest = () => invoke<Digest | null>("last_digest");
export const approvalCounts = () => invoke<Approved[]>("approval_counts");
export const UNLOCK = 20;
