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
