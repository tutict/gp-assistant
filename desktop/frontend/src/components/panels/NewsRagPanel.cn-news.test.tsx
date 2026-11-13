import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceProvider } from "../../hooks/useWorkspace";
import { workspaceTestHarness } from "../../test/workspace";
import { NewsRagPanel, formatResearchUpdatedAt } from "./NewsRagPanel";
import type { ResearchMessage } from "../../types";

const transport = vi.hoisted(() => ({ getJson: vi.fn(), postJson: vi.fn() }));
vi.mock("../../lib/tauri", () => ({
  getJson: transport.getJson, postJson: transport.postJson, openExternalUrl: vi.fn(),
  isMobileTauriRuntime: () => false, isTauriRuntime: () => false,
}));

const message = {
  id: "sina-cn-news", document_id: "cn-news", stock_code: "600909.SH",
  title: "华安证券发布业务进展", summary: "证券经纪和资产管理业务持续发展。",
  source_tier: "news", source_name: "新浪财经", sentiment: "uncertain", unread: false,
  published_at: "2026-11-12T01:00:00Z", fetched_at_epoch_ms: Date.parse("2026-11-12T02:00:00Z"),
  article_summary_fetched_at_epoch_ms: Date.parse("2026-11-12T02:02:00Z"), summary_kind: "article_summary",
  url: "https://finance.sina.com.cn/stock/s/2026-11-12/doc-cn-fixture.shtml",
} satisfies ResearchMessage;
const overview = { schema_version: 2, document_count: 1, chunk_count: 1, unread_count: 0,
  messages: [message], updated_at_epoch_ms: message.fetched_at_epoch_ms, retrieval: { vector: { ready: false } } };
const report = (status: string) => ({ news: { scope_codes: [message.stock_code], notes: [], refresh: {
  status, fetched_count: status === "failed" ? 0 : 1, summary_count: 1,
  sources: [
    { source_id: "sina_finance", source_name: "新浪财经", successful_requests: status === "failed" ? 0 : 1, failed_requests: status === "failed" ? 1 : 0, fetched_count: 1, errors: [] },
    { source_id: "eastmoney", source_name: "东方财富", successful_requests: status === "success" ? 1 : 0, failed_requests: status === "success" ? 0 : 1, fetched_count: 0, errors: status === "success" ? [] : ["新闻连接超时"] },
  ],
} } });
let renderer: ReactTestRenderer | undefined;
let workspace = workspaceTestHarness();

function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(text).join("");
  if (value && typeof value === "object" && "children" in value) return text(value.children);
  return "";
}
async function renderNews(props: Parameters<typeof NewsRagPanel>[0] = {}) {
  await act(async () => { renderer = create(<WorkspaceProvider store={workspace.store}>
    <NewsRagPanel market="cn" initialCode={message.stock_code} watchlist={[{ code: message.stock_code, name: "华安证券" }]} {...props} />
  </WorkspaceProvider>); });
}
async function refresh() {
  await act(async () => renderer!.root.findByProps({ className: "research-actions" }).findAllByType("button")
    .find(button => /^更新/.test(text(button.children)))!.props.onClick());
}
async function openMessage() {
  await act(async () => renderer!.root.find(node => node.type === "button" && node.props.className?.startsWith("research-event ")).props.onClick());
}

beforeEach(() => {
  workspace = workspaceTestHarness();
  transport.getJson.mockReset().mockImplementation(async (path: string) => path.includes("/overview") ? overview : path.includes("/messages") ? { items: [message] } : { items: [] });
  transport.postJson.mockReset().mockResolvedValue(report("success"));
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("navigator", { onLine: true });
  vi.stubGlobal("document", { visibilityState: "visible", addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.stubGlobal("window", { setTimeout: vi.fn(() => 1), clearTimeout: vi.fn(), setInterval: vi.fn(() => 1), clearInterval: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() });
});
afterEach(async () => {
  await act(async () => renderer?.unmount());
  renderer = undefined;
  workspace.store.dispose();
  vi.unstubAllGlobals();
});

describe("A-share news provenance and refresh", () => {
  it("keeps the summary once when expanded and restores its preview when collapsed", async () => {
    await renderNews();
    expect(text(renderer!.toJSON()).split(message.summary).length - 1).toBe(1);
    await openMessage();
    expect(text(renderer!.toJSON()).split(message.summary).length - 1).toBe(1);
    expect(renderer!.root.findAllByProps({ className: "research-event-summary" })).toHaveLength(0);
    await openMessage();
    expect(renderer!.root.findByProps({ className: "research-event-summary" }).children).toEqual([message.summary]);
  });

  it("shows the last update beside the stock heading without opening a disclosure", async () => {
    await renderNews();
    const heading = renderer!.root.findByProps({ className: "research-reading-heading" });
    expect(text(heading)).toContain("华安证券");
    expect(text(heading)).toContain("1 条消息");
    expect(text(heading)).toContain(`上次更新 ${formatResearchUpdatedAt(overview)}`);
  });

  it("shows publication, message fetch and original-summary fetch times separately", async () => {
    await renderNews();
    await openMessage();
    const visible = text(renderer!.toJSON());
    expect(visible).toContain("发布 2026/11/12 09:00");
    expect(visible).toContain("消息抓取 2026/11/12 10:00");
    expect(visible).toContain("摘要抓取 2026/11/12 10:02");
    expect(visible).toContain("原文摘要");
    const detail = renderer!.root.findByProps({ className: "research-event-detail" });
    expect(detail.findByType("a").props.href).toBe(message.url);
  });

  it("marks missing fetch time and headline-only text without inventing provenance", async () => {
    transport.getJson.mockImplementation(async (path: string) => path.includes("/overview") ? overview : path.includes("/messages")
      ? { items: [{ ...message, summary: message.title, summary_kind: "unavailable", fetched_at_epoch_ms: null, article_summary_fetched_at_epoch_ms: null }] } : { items: [] });
    await renderNews();
    await openMessage();
    const detail = text(renderer!.root.findByProps({ className: "research-event-detail" }));
    expect(detail).toContain("消息抓取时间未记录");
    expect(detail).toContain("摘要未获取");
    expect(detail).not.toContain("消息抓取 2026/11/12 10:00");
  });

  it("refreshes unique A-share stocks in the all-messages view", async () => {
    await renderNews({ watchlist: [{ code: "600909.SH" }, { code: "000001.SZ" }, { code: "00700.HK" }] });
    await act(async () => renderer!.root.find(node => node.type === "button" && text(node) === "全部消息").props.onClick());
    await refresh();
    const calls = transport.postJson.mock.calls.filter(([path]) => path === "/api/research/refresh");
    expect(calls).toHaveLength(1);
    expect(calls[0]?.[1]).toMatchObject({ seed_codes: ["600909.SH", "000001.SZ"], include_us_market_brief: false, summary_priority_urls: [message.url] });
  });

  it.each(["partial", "failed"])("reports %s source refreshes while keeping cached messages readable", async status => {
    transport.postJson.mockResolvedValue(report(status));
    await renderNews();
    await refresh();
    const visible = text(renderer!.toJSON());
    expect(visible).toContain(status === "failed" ? "消息更新失败" : "部分消息来源未更新");
    expect(visible).toContain("东方财富");
    expect(visible).toContain(message.title);
    expect(visible).not.toContain("消息已更新。");
  });

  it("clears a failed refresh after a successful retry", async () => {
    transport.postJson.mockResolvedValueOnce(report("failed")).mockResolvedValueOnce(report("success"));
    await renderNews();
    await refresh();
    expect(text(renderer!.toJSON())).toContain("消息更新失败");
    await refresh();
    expect(text(renderer!.toJSON())).not.toContain("消息更新失败");
    expect(text(renderer!.toJSON())).toContain("消息已更新。");
  });

  it("reports an unresolved A-share scope instead of confirming an update", async () => {
    transport.postJson.mockResolvedValue({ news: { scope_codes: [], notes: ["目标 A 股未匹配到证券目录"] } });
    await renderNews();
    await refresh();
    expect(text(renderer!.toJSON())).toContain("目标 A 股未匹配到证券目录");
    expect(text(renderer!.toJSON())).not.toContain("消息已更新。");
  });

  it("keeps the calendar date in the overview update time", () => {
    expect(formatResearchUpdatedAt(overview)).toBe("2026/11/12 10:00");
  });
});
