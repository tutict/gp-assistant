import { DEFAULT_FILTER_CRITERIA, sanitizeFilterCriteria } from "./screenCriteria";
import type { FilterCriteria } from "../components/FilterBar";

export const WORKSPACE_SCHEMA = 1;
export const MAX_VALUE_BYTES = 256 * 1024;
export const MAX_WORKSPACE_BYTES = 2 * 1024 * 1024;
export const MAX_WORKSPACE_KEYS = 128;
export const WORKSPACE_DEBOUNCE_MS = 350;
export type LegacyStorage = Pick<Storage, "getItem">;
const bytes = (value: string) => new TextEncoder().encode(value).length;
const record = (value: unknown): value is Record<string, unknown> => Boolean(value && typeof value === "object" && !Array.isArray(value));
function text(value: unknown, max: number): string {
  if (typeof value !== "string" || bytes(value) > max) throw new Error("Invalid or oversized workspace text");
  return value;
}
function clipped(value: unknown, max: number): string {
  let result = String(value ?? "").slice(0, max);
  while (bytes(result) > max) result = result.slice(0, Math.floor(result.length * 0.9));
  return result;
}
export function projectConversations(value: unknown) {
  if (!Array.isArray(value) || value.length > 40) throw new Error("Invalid workspace conversations");
  return value.map(item => {
    if (!record(item) || !["quick", "expert", "research"].includes(String(item.mode)) || !Array.isArray(item.messages) || item.messages.length > 200) throw new Error("Conversation storage limit reached; history was not truncated");
    const id = text(item.id, 256); if (!id) throw new Error("Missing conversation identity");
    const messages = item.messages.map(message => {
      if (!record(message) || !["user", "assistant"].includes(String(message.role))) throw new Error("Invalid workspace message");
      return { role: message.role, content: text(message.content ?? "", 128 * 1024), timestamp: Number(message.timestamp) || 0,
        ...(typeof message.runId === "string" ? { runId: text(message.runId, 256) } : {}), error: Boolean(message.error) };
    });
    return { id, title: clipped(item.title, 180), mode: item.mode, messages, createdAt: Number(item.createdAt) || 0, updatedAt: Number(item.updatedAt) || 0 };
  });
}

const CORE_CRITERIA_KEYS = ["min_roe","max_pe","max_pb","min_market_cap_billion","min_deducted_net_profit_billion","min_deducted_net_profit_margin","min_deducted_net_profit_growth_rate","industry","market_scope","require_institution_buy_ratio_gt_sell_ratio","include_st","limit","sort_by","sort_dir","score_profile"];
function validCoreCriteria(value: unknown): boolean {
  return record(value) && Object.entries(value).every(([key,item]) => CORE_CRITERIA_KEYS.includes(key) && (item == null || typeof item === "boolean" || (typeof item === "number" && Number.isFinite(item)) || (typeof item === "string" && bytes(item) <= 256)));
}
export function workspaceValue(key: string, value: unknown): unknown {
  if (bytes(key) > 280 || /[\u0000-\u001f]/.test(key)) throw new Error("Invalid workspace key");
  let result: unknown;
  if (key.startsWith("agent.draft:") && key.length > 12) result = value === null ? null : text(value, 32000);
  else switch (key) {
    case "app.view": if (!["screen", "observe", "backtest", "news", "agent"].includes(String(value))) throw new Error("Invalid workspace view"); result = value; break;
    case "app.stock": case "news.stock": result = text(value, 32); break;
    case "agent.active": case "news.thread": result = text(value, 256); break;
    case "news.question": result = text(value, 32000); break;
    case "app.backtest.source": if (value !== "criteria" && value !== "watchlist") throw new Error("Invalid backtest source"); result = value; break;
    case "migration.localStorage.v1": if (value !== true) throw new Error("Invalid migration marker"); result = true; break;
    case "agent.conversations": result = projectConversations(value); break;
    case "backtest.start": case "backtest.end": case "backtest.rebalance": case "backtest.benchmark": case "backtest.strategyMode": result = text(value, 64); break;
    case "backtest.topN": case "backtest.costBps": if (typeof value !== "number" || !Number.isFinite(value)) throw new Error("Invalid backtest number"); result = value; break;
    case "backtest.adaptiveSpec": {
      if (value === null) { result = null; break; }
      if (!record(value) || Object.keys(value).some(key => !["criteria","mode","horizon","primary_limit","exploration_limit","run_id"].includes(key)) || !validCoreCriteria(value.criteria) || !["auto","range","trend","defensive"].includes(String(value.mode)) || value.horizon !== "swing_10_30d" || typeof value.primary_limit !== "number" || typeof value.exploration_limit !== "number") throw new Error("Invalid adaptive backtest specification");
      text(value.run_id, 256); result = value; break;
    }
    case "backtest.criteria": case "app.criteria": {
      if (!record(value)) throw new Error("Invalid filter criteria");
      const fields = Object.fromEntries(Object.entries(DEFAULT_FILTER_CRITERIA).map(([name, fallback]) => {
        const candidate = value[name] ?? fallback;
        if (typeof candidate !== typeof fallback || (typeof candidate === "string" && bytes(candidate) > 256)
          || (typeof candidate === "number" && !Number.isFinite(candidate))) throw new Error("Invalid filter field");
        return [name, candidate];
      }));
      result = sanitizeFilterCriteria(fields as unknown as FilterCriteria); break;
    }
    default: throw new Error("Unknown workspace key");
  }
  if (bytes(JSON.stringify(result)) > (key === "agent.conversations" ? MAX_WORKSPACE_BYTES : MAX_VALUE_BYTES)) throw new Error("Workspace value exceeds size limit");
  return result;
}
export function validateWorkspaceValues(values: unknown): Record<string, unknown> {
  if (!record(values) || Object.keys(values).length > MAX_WORKSPACE_KEYS || bytes(JSON.stringify(values)) > MAX_WORKSPACE_BYTES) throw new Error("Invalid workspace size");
  return Object.fromEntries(Object.entries(values).map(([key, value]) => [key, workspaceValue(key, value)]));
}
/** Read only known non-secret keys. Never delete or rewrite legacy ledger tombstones. */
export function migrateWorkspaceLegacy(source: LegacyStorage): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  const criteria = source.getItem("stock-optimizer-criteria");
  if (criteria) values["app.criteria"] = workspaceValue("app.criteria", JSON.parse(criteria));
  const history = source.getItem("stock-optimizer-agent-conversations");
  if (history) {
    const parsed: unknown = JSON.parse(history);
    if (!Array.isArray(parsed)) throw new Error("Invalid legacy Agent history; original retained");
    const kept = parsed.filter(item => record(item) && typeof item.id === "string"
      && source.getItem(`stock-optimizer-agent-failed-ledger-deletion:${encodeURIComponent(item.id)}`) === null);
    values["agent.conversations"] = projectConversations(kept);
    const rawActive = source.getItem("stock-optimizer-agent-active-conversation");
    if (rawActive) {
      let active = rawActive;
      try { const decoded: unknown = JSON.parse(rawActive); if (typeof decoded === "string") active = decoded; } catch { /* Older versions stored a plain string. */ }
      if (kept.some(item => item.id === active)) values["agent.active"] = workspaceValue("agent.active", active);
    }
  }
  return { ...values, "migration.localStorage.v1": true };
}
