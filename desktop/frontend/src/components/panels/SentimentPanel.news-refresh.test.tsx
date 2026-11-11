import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { WorkspaceProvider } from "../../hooks/useWorkspace";
import { workspaceTestHarness } from "../../test/workspace";
import { StockCodeInput } from "../StockCodeInput";
import { SentimentPanel } from "./SentimentPanel";

const transport = vi.hoisted(() => ({ getJson: vi.fn(), postJson: vi.fn() }));
vi.mock("../../lib/tauri", () => ({
  getJson: transport.getJson, postJson: transport.postJson,
  isMobileTauriRuntime: () => false, isTauriRuntime: () => false,
  openExternalUrl: vi.fn(),
}));
let workspace = workspaceTestHarness();
let renderer: ReactTestRenderer | undefined;
const cached = { id: "tencent", document_id: "tencent", stock_code: "00700.HK", title: "腾讯招聘",
  summary: "腾讯招聘", source_tier: "news", source_name: "新浪财经", sentiment: "uncertain", unread: false,
  published_at: "2026-11-09T11:37:00+08:00", url: "https://t.cj.sina.cn/articles/view/1651428902/626ece2602001lifc" };
const enriched = { ...cached, summary: "腾讯混元上线招聘，社招有60余个岗位。青云课题涵盖逾160个岗位。" };
const bank = { ...cached, id: "bank", document_id: "bank", stock_code: "01288.HK", title: "农业银行财务快照", source_tier: "financial_snapshot" };

function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(text).join("");
  if (value && typeof value === "object" && "children" in value) return text(value.children);
  return "";
}

beforeEach(() => {
  workspace = workspaceTestHarness(undefined, { "hk.news.stock": "" });
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("navigator", { onLine: true });
  vi.stubGlobal("document", { visibilityState: "visible", addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal("window", { setTimeout: vi.fn(() => 1), clearTimeout: vi.fn(), setInterval: vi.fn(() => 1), clearInterval: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() });
  let refreshed = false;
  transport.getJson.mockImplementation(async (path: string) => {
    const url = new URL(path, "https://fixture.test");
    const selected = url.searchParams.get("stock_code");
    const all = [bank, refreshed ? enriched : cached];
    const messages = selected ? all.filter(item => item.stock_code === selected) : all;
    if (url.pathname.endsWith("/messages")) return { items: messages };
    if (url.pathname.endsWith("/overview")) return { messages, document_count: messages.length, retrieval: { vector: { ready: false } } };
    return { items: [] };
  });
  transport.postJson.mockImplementation(async (path: string) => {
    if (path === "/api/research/refresh") {
      refreshed = true;
      return { news: { scope_codes: ["00700.HK"], notes: ["港股原文摘要补全 1 条。"] } };
    }
    if (path.endsWith("/snapshot")) return { evidence: [], timeline: [], metrics: {}, coverage: { gaps: [] } };
    if (path.endsWith("/latest")) return { analysis: null };
    return { items: [] };
  });
});

afterEach(async () => {
  await act(async () => renderer?.unmount());
  renderer = undefined;
  workspace.store.dispose();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("commits a typed HK stock and refreshes once through the real parent and child", async () => {
  await import("./NewsRagPanel");
  await act(async () => { renderer = create(<WorkspaceProvider store={workspace.store}><SentimentPanel market="hk" watchlist={[]} /></WorkspaceProvider>); });
  expect(renderer!.root.findByProps({ "aria-label": "立即更新消息" }).props.disabled).toBe(false);
  await act(async () => renderer!.root.findByType(StockCodeInput).props.onChange("00700"));
  await act(async () => renderer!.root.findByProps({ "aria-label": "立即更新消息" }).props.onClick());
  const updates = transport.postJson.mock.calls.filter(([path]) => path === "/api/research/refresh");
  expect(updates).toHaveLength(1);
  expect(updates[0]?.[1]).toMatchObject({ market: "hk", code: "00700.HK", seed_codes: ["00700.HK"] });
  expect(text(renderer!.toJSON())).toContain(enriched.summary);
  expect(text(renderer!.toJSON())).not.toContain(bank.title);
  expect(workspace.store.get("hk.news.stock", "")).toBe("00700.HK");
});
