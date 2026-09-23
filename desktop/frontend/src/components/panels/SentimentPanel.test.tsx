import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { SentimentAnalysis, SentimentRun, SentimentSnapshot } from "../../types/sentiment";
import { SentimentPanel } from "./SentimentPanel";

const { postJson } = vi.hoisted(() => ({ postJson: vi.fn() }));
vi.mock("../../lib/tauri", () => ({ postJson }));
vi.mock("./LlmSettingsPanel", () => ({ LlmSettingsPanel: () => null }));
vi.mock("./NewsRagPanel", () => ({
  NewsRagPanel: (props: { code?: string; onCodeChange?: (code: string) => void }) => (
    <div>
      <span>消息资料子视图</span>
      <span>{props.code}</span>
      <button type="button" onClick={() => props.onCodeChange?.("000001.SZ")}>消息内换股票</button>
    </div>
  ),
}));

const snapshot: SentimentSnapshot = {
  snapshot_id: "frozen", stock_code: "600000.SH", stock_name: "浦发银行", industry: "银行",
  window_days: 30, cutoff: 1789732800000, generation: "fixture", rule_version: "fixture",
  evidence: [], timeline: [], metrics: {},
  coverage: { facts: 0, discussions: 0, price_days: 0, history_days: 0, industry_members: 0, industry_covered: 0, gaps: [] },
};
const previous: SentimentAnalysis = {
  analysis_id: "previous", run_id: "previous-run", snapshot_id: snapshot.snapshot_id,
  stock_code: snapshot.stock_code, created_at: snapshot.cutoff, stage: "证据不足",
  top_risk: "待确认", bottom_candidate: "待确认", turning_signal: "待确认", sufficiency: "不足",
  summary: "先前分析结论", dimensions: [], support: [], against: [], invalidation: [],
  evidence_ids: [], model: "fixture", rule_version: "fixture", stale: false, snapshot,
};
let renderer: ReactTestRenderer | undefined;
let status: SentimentRun;
function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(textOf).join("");
  if (value && typeof value === "object" && "children" in value) return textOf(value.children);
  return "";
}
async function click(label: string) {
  const button = renderer!.root.findAllByType("button").find((item) => textOf(item.children) === label);
  expect(button, `button ${label} must be visible`).toBeDefined();
  await act(async () => button!.props.onClick());
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  status = { run_id: "active-run", stock_code: snapshot.stock_code, status: "running", stage: "冻结证据", progress: 25 };
  postJson.mockImplementation(async (path: string) => {
    if (path.endsWith("/snapshot")) return snapshot;
    if (path.endsWith("/latest")) return { analysis: previous };
    if (path.endsWith("/history")) return { items: [previous] };
    if (path.endsWith("/start")) return { run_id: status.run_id };
    if (path.endsWith("/status")) return status;
    if (path.endsWith("/cancel")) return { cancelled: true };
    throw new Error(`Unexpected request: ${path}`);
  });
});
afterEach(async () => {
  await act(async () => renderer?.unmount());
  renderer = undefined;
  vi.useRealTimers();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("keeps the original run polling through the sources subview and displays its completion", async () => {
  await act(async () => {
    renderer = create(<SentimentPanel initialCode={snapshot.stock_code} llmSettings={{
      active_provider_id: "fixture", providers: [{ id: "fixture", model: "fixture", base_url: "http://localhost" }],
    }} />);
  });
  await click("情绪");
  await click("重新分析");
  expect(renderer!.root.findByType("progress").props.value).toBe(25);
  const readsBeforeSwitch = postJson.mock.calls.filter(([path]) => /\/(snapshot|latest|history)$/.test(path)).length;
  await click("消息");
  expect(textOf(renderer!.toJSON())).toContain("消息资料子视图");
  expect(renderer!.root.findByProps({ className: "sentiment-view sentiment-view-analysis" }).props.hidden).toBe(true);

  status = { ...status, stage: "核对冻结证据", progress: 60 };
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  await click("情绪");
  expect(renderer!.root.findByType("progress").props.value).toBe(60);
  expect(textOf(renderer!.toJSON())).toContain("取消分析");
  expect(textOf(renderer!.toJSON())).toContain("先前分析结论");
  expect(postJson.mock.calls.filter(([path]) => /\/(snapshot|latest|history)$/.test(path))).toHaveLength(readsBeforeSwitch);

  status = { ...status, status: "completed", progress: 100, result: { ...previous, analysis_id: "completed", run_id: "active-run", summary: "本次分析已完成" } };
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(textOf(renderer!.toJSON())).toContain("本次分析已完成");
  expect(renderer!.root.findAllByType("progress")).toHaveLength(0);
  expect(postJson.mock.calls.filter(([path]) => path.endsWith("/start"))).toHaveLength(1);
  expect(postJson.mock.calls.filter(([path]) => path.endsWith("/status")).map(([, body]) => body)).toEqual([
    { run_id: "active-run" }, { run_id: "active-run" }, { run_id: "active-run" },
  ]);
});

it("filters evidence by the Shanghai day and shows the current snapshot when analysis is stale", async () => {
  const utcEvening = "2026-09-14T20:00:00Z";
  const shanghaiMorning = "2026-09-14T10:00:00+08:00";
  const evidence = [
    { ...snapshot, id: "E-utc", document_id: "utc", event_id: "e-utc", title: "UTC晚间公告", excerpt: "utc", source_name: "公告", source_tier: "filing", source_verified: true, published_at: utcEvening, first_seen_at: 1, url: null, sentiment: "positive", pool: "fact" as const, coverage: "full_text" as const, duplicate_count: 1, provenance: { retracted: true } },
    { ...snapshot, id: "E-cst", document_id: "cst", event_id: "e-cst", title: "北京上午公告", excerpt: "cst", source_name: "公告", source_tier: "filing", source_verified: false, published_at: shanghaiMorning, first_seen_at: 1, url: null, sentiment: "positive", pool: "fact" as const, coverage: "excerpt" as const, duplicate_count: 0 },
  ];
  const fresh = { ...snapshot, snapshot_id: "fresh-snapshot", evidence, timeline: [
    { date: "2026-09-15", positive: 1, negative: 0, discussion_count: 0, sentiment_balance: 0.2, close: 10, volume: 1000, industry_return: 0.1, industry_breadth: 50 },
    { date: "2026-09-14", positive: 0, negative: 1, discussion_count: 0, sentiment_balance: -0.2, close: 11, volume: 1100, industry_return: -0.1, industry_breadth: 40 },
  ], metrics: { price_return_7d_pct: 2, sentiment_balance: 0.2 }, metric_references: { M2: ["price_return_7d_pct"] } };
  const staleAnalysis = { ...previous, stale: true, snapshot: { ...fresh, snapshot_id: "old-snapshot", evidence: [] } };
  postJson.mockImplementation(async (path: string) => {
    if (path.endsWith("/snapshot")) return fresh;
    if (path.endsWith("/latest")) return { analysis: staleAnalysis };
    if (path.endsWith("/history")) throw new Error("history offline");
    throw new Error(path);
  });
  await act(async () => { renderer = create(<SentimentPanel initialCode="600000.SH" llmSettings={null} />); });
  const text = () => textOf(renderer!.toJSON());
  await click("情绪");
  expect(text()).toContain("缺失数据不记为零");
  expect(text()).toContain("已有更新的数据");
  expect(text()).toContain("当前阶段");
  expect(text()).not.toContain("上次结论");
  expect(text()).not.toContain("UTC晚间公告");
  expect(text()).toContain("重试加载");
  expect(text()).toContain("先配置模型");
  await click("查看新证据");
  expect(text()).toContain("上次结论");
  expect(text()).toContain("UTC晚间公告");
  expect(text()).toContain("北京上午公告");
  const clickDay = async (day: string) => {
    const button = renderer!.root.findAllByType("button").find((item) => String(item.props["aria-label"] ?? "").startsWith(day));
    expect(button, day).toBeDefined();
    await act(async () => button!.props.onClick());
  };
  await clickDay("2026-09-15");
  expect(text()).toContain("UTC晚间公告");
  expect(text()).toContain("已撤回");
  expect(text()).not.toContain("北京上午公告");
  await clickDay("2026-09-14");
  expect(text()).toContain("北京上午公告");
  expect(text()).not.toContain("UTC晚间公告");
  const select = renderer!.root.findByType("select");
  await act(async () => select.props.onChange({ target: { value: "close" } }));
  const marks = renderer!.root.findAll((node) => node.props.className === "sentiment-line-hit");
  expect(marks.map((mark) => mark.props["data-bottom"])).toEqual(["8%", "88%"]);
});

it("shares one stock across news and sentiment and only prefills agent", async () => {
  const onAskAgent = vi.fn();
  await act(async () => {
    renderer = create(<SentimentPanel initialCode="600000.SH" watchlist={[{ code: "600000.SH", name: "浦发银行" }]} onAskAgent={onAskAgent} llmSettings={null} />);
  });
  expect(textOf(renderer!.toJSON())).toContain("消息资料子视图");
  expect(renderer!.root.findByProps({ className: "sentiment-view sentiment-view-analysis" }).props.hidden).toBe(true);
  await click("消息内换股票");
  expect(renderer!.root.findByProps({ id: "sentiment-stock" }).props.value).toBe("000001.SZ");
  await click("情绪");
  expect(textOf(renderer!.toJSON())).toContain("000001.SZ");
  await click("交给 Agent");
  expect(onAskAgent).toHaveBeenCalledTimes(1);
  expect(onAskAgent.mock.calls[0][0]).toContain("000001.SZ");
  expect(onAskAgent.mock.calls[0][0]).toContain("证据不足");
});
