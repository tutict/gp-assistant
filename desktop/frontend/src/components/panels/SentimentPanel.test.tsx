import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { SentimentAnalysis, SentimentEvidence, SentimentRun, SentimentSnapshot } from "../../types/sentiment";
import { SentimentPanel } from "./SentimentPanel";

const { postJson } = vi.hoisted(() => ({ postJson: vi.fn() }));
const headerActions = vi.hoisted(() => ({ onRefresh: vi.fn(), onOpenKnowledge: vi.fn() }));
vi.mock("../../lib/tauri", () => ({ postJson }));
vi.mock("./LlmSettingsPanel", () => ({ LlmSettingsPanel: (props: { openRequest?: number }) => <span data-model-open-request={props.openRequest} /> }));
vi.mock("./NewsRagPanel", () => ({
  NewsRagPanel: (props: {
    code?: string;
    onCodeChange?: (code: string) => void;
    onHeaderToolsChange?: (tools: {
      refreshing: boolean;
      vectorReady: boolean;
      onRefresh: () => void;
      onOpenKnowledge: () => void;
      onOpenInbox: () => void;
    }) => void;
  }) => (
    <div>
      <span>消息资料子视图</span>
      <span>{props.code}</span>
      <button type="button" onClick={() => props.onCodeChange?.("000001.SZ")}>消息内换股票</button>
      <button type="button" onClick={() => props.onHeaderToolsChange?.({
        refreshing: false,
        vectorReady: true,
        onRefresh: headerActions.onRefresh,
        onOpenKnowledge: headerActions.onOpenKnowledge,
        onOpenInbox: () => undefined,
      })}>显示顶栏工具</button>
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
  vi.stubGlobal("requestAnimationFrame", (callback: () => void) => { callback(); return 1; });
  vi.stubGlobal("document", { addEventListener: vi.fn(), removeEventListener: vi.fn() });
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
  expect(text()).toContain("配置模型");
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

it("uses accessible icon actions for stock lookup, refresh, and knowledge management", async () => {
  headerActions.onRefresh.mockReset();
  headerActions.onOpenKnowledge.mockReset();
  await act(async () => {
    renderer = create(<SentimentPanel initialCode="600000.SH" llmSettings={null} />);
  });
  await click("显示顶栏工具");

  const lookup = renderer!.root.findByProps({ "aria-label": "查看股票消息" });
  const refresh = renderer!.root.findByProps({ "aria-label": "立即更新消息" });
  await act(async () => renderer!.root.findByProps({ "aria-label": "更多研究操作" }).props.onClick());
  const knowledge = renderer!.root.findAllByType("button").find(item => textOf(item.children) === "知识库管理")!;
  expect(lookup.props.className).toBe("sentiment-icon-action");
  expect(refresh.props.className).toBe("sentiment-icon-action");
  expect(knowledge.props.role).toBe("menuitem");
  expect(lookup.findAllByType("svg")).toHaveLength(1);
  expect(refresh.findAllByType("svg")).toHaveLength(1);
  expect(knowledge.findAllByType("svg")).toHaveLength(1);
  await act(async () => refresh.props.onClick());
  await act(async () => knowledge.props.onClick());
  expect(headerActions.onRefresh).toHaveBeenCalledTimes(1);
  expect(headerActions.onOpenKnowledge).toHaveBeenCalledTimes(1);
});

it("connects the stock search action to its input and keeps its visible label compact", async () => {
  await act(async () => {
    renderer = create(<SentimentPanel initialCode="600000.SH" llmSettings={null} />);
  });

  const toolbar = renderer!.root.findByProps({ className: "sentiment-toolbar" });
  const label = toolbar.find((node) => node.type === "label" && node.props.htmlFor === "sentiment-stock");
  const search = toolbar.find((node) => node.type === "button" && node.props["aria-label"] === "查看股票消息");
  expect(label.props.className).toContain("visually-hidden");
  expect(toolbar.findAllByProps({ className: "stock-code-input" })).toHaveLength(1);
  expect(search.parent).toBe(toolbar);
});


it("folds management into More and opens the shared model dialog by request", async () => {
  await act(async () => { renderer = create(<SentimentPanel initialCode="600000.SH" llmSettings={null} />); });
  await click("显示顶栏工具");
  expect(renderer!.root.findAllByProps({ "aria-label": "知识库管理" })).toHaveLength(0);
  await act(async () => renderer!.root.findByProps({ "aria-label": "更多研究操作" }).props.onClick());
  expect(renderer!.root.findByProps({ "aria-label": "更多研究操作" }).props["aria-expanded"]).toBe(true);
  await click("知识库管理");
  expect(headerActions.onOpenKnowledge).toHaveBeenCalledTimes(1);
  expect(renderer!.root.findByProps({ "aria-label": "更多研究操作" }).props["aria-expanded"]).toBe(false);
  await act(async () => renderer!.root.findByProps({ "aria-label": "更多研究操作" }).props.onClick());
  await click("模型设置");
  expect(renderer!.root.findByProps({ "data-model-open-request": 1 })).toBeDefined();
  expect(renderer!.root.findAllByProps({ className: "sentiment-title" })).toHaveLength(0);
});


it("closes the mobile stock drawer on selection and never installs its effects inside the hidden sentiment view", async () => {
  vi.stubGlobal("window", { matchMedia: (query: string) => ({ matches: query === "(max-width: 768px)", addEventListener: vi.fn(), removeEventListener: vi.fn() }) });
  const body = { style: { overflow: "original" } };
  const addEventListener = vi.fn();
  vi.stubGlobal("document", { body, addEventListener, removeEventListener: vi.fn() });
  postJson.mockImplementation(async (path: string, request: { stock_code?: string }) => {
    const currentCode = request?.stock_code || snapshot.stock_code;
    if (path.endsWith("/snapshot")) return { ...snapshot, stock_code: currentCode };
    if (path.endsWith("/latest")) return { analysis: { ...previous, stock_code: currentCode, summary: `分析仅属于${currentCode}`, snapshot: { ...snapshot, stock_code: currentCode } } };
    if (path.endsWith("/history")) return { items: [] };
    return { items: [] };
  });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode="600000.SH" watchlist={[
    { code: "600000.SH", name: "股票A" }, { code: "000002.SZ", name: "股票B" }, { code: "000001.SZ", name: "股票C" },
  ]} />); });
  const dialogs = () => renderer!.root.findAllByProps({ role: "dialog", "aria-label": "自选股票情绪" });
  const openStock = () => act(async () => renderer!.root.findByProps({ "aria-label": "打开股票列表" }).props.onClick());
  await openStock();
  expect(dialogs()).toHaveLength(1);
  expect(body.style.overflow).toBe("hidden");
  const stockB = dialogs()[0].findAllByType("button").find(button => textOf(button).includes("000002.SZ"))!;
  await act(async () => stockB.props.onClick());
  expect(dialogs()).toHaveLength(0);
  expect(body.style.overflow).toBe("original");
  expect(renderer!.root.findByProps({ id: "sentiment-stock" }).props.value).toBe("000002.SZ");
  expect(textOf(renderer!.toJSON())).toContain("分析仅属于000002.SZ");
  expect(textOf(renderer!.toJSON())).not.toContain("分析仅属于600000.SH");
  await openStock();
  expect(dialogs()).toHaveLength(1);
  await click("消息");
  expect(dialogs()).toHaveLength(0);
  expect(body.style.overflow).toBe("original");
  const handlersBeforeSourceSelection = addEventListener.mock.calls.filter(([type]) => type === "keydown").length;
  await click("消息内换股票");
  expect(renderer!.root.findByProps({ className: "sentiment-view sentiment-view-analysis" }).props.hidden).toBe(true);
  expect(dialogs()).toHaveLength(0);
  expect(addEventListener.mock.calls.filter(([type]) => type === "keydown")).toHaveLength(handlersBeforeSourceSelection);
  expect(body.style.overflow).toBe("original");
  await click("情绪");
  expect(dialogs()).toHaveLength(0);
  expect(textOf(renderer!.toJSON())).toContain("分析仅属于000001.SZ");
  await openStock();
  expect(dialogs()).toHaveLength(1);
});

function evidenceFixture(id: string, title = `冻结来源${id}`): SentimentEvidence {
  return { id, title, excerpt: `摘要${id}`, document_id: id, event_id: id, source_name: "公告", source_tier: "filing", source_verified: true, published_at: "2026-09-15", first_seen_at: snapshot.cutoff, url: null, sentiment: "positive", pool: "fact", coverage: "excerpt", duplicate_count: 0 };
}
function installAnalysis(frozen: SentimentSnapshot, analysisOverrides: Partial<SentimentAnalysis> = {}, fresh = frozen) {
  const analysis = { ...previous, snapshot: frozen, snapshot_id: frozen.snapshot_id, ...analysisOverrides };
  postJson.mockImplementation(async (path: string) => {
    if (path.endsWith("/snapshot")) return fresh;
    if (path.endsWith("/latest")) return { analysis };
    if (path.endsWith("/history")) return { items: [analysis] };
    if (path.endsWith("/followup")) return { analysis_id: analysis.analysis_id, answer: "冻结分析回答", evidence_ids: ["E1", "new-only"], created_at: snapshot.cutoff };
    return { items: [] };
  });
}

it("leads with at most three actual frozen citations in analysis order and folds detailed work", async () => {
  installAnalysis({ ...snapshot, evidence: ["E1", "E2", "E3", "E4"].map(id => evidenceFixture(id)) }, { evidence_ids: ["missing", "E3", "E1", "E3", "E4", "E2"] });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />); });
  const main = renderer!.root.findByProps({ className: "sentiment-main" });
  const key = main.findByProps({ "aria-label": "关键证据" });
  expect(key.findAllByProps({ className: "sentiment-evidence-row" }).map(row => row.props["data-evidence-id"])).toEqual(["E3", "E1", "E4"]);
  const sections = main.children.filter(child => typeof child !== "string");
  expect(sections.findIndex(node => node.props["aria-label"] === "情绪结论")).toBeLessThan(sections.findIndex(node => node.props["aria-label"] === "关键证据"));
  expect(sections.findIndex(node => node.props["aria-label"] === "关键证据")).toBeLessThan(sections.findIndex(node => node.props.className === "sentiment-timeline"));
  for (const label of ["三个分析维度", "详细来源", "分析历史", "策略与参数"]) {
    const fold = main.findByProps({ "aria-label": label });
    expect(fold.type).toBe("details");
    expect(fold.props.open).not.toBe(true);
  }
  expect(main.findAllByType("textarea")).toHaveLength(0);
});

it("keeps metric controls with compact null data and renders finite zero and separated gaps", async () => {
  const points = [0, null, 0].map((value, index) => ({ date: `2026-09-${15 + index}`, positive: null, negative: null, discussion_count: null, sentiment_balance: null, close: value, volume: 10, industry_return: Number.NaN, industry_breadth: null }));
  installAnalysis({ ...snapshot, timeline: points });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />); });
  const timeline = renderer!.root.findByProps({ className: "sentiment-timeline" });
  expect(timeline.findAllByProps({ className: "sentiment-line-plot" })).toHaveLength(0);
  expect(textOf(timeline)).toContain("此指标暂无有效数据");
  const metrics = timeline.findByProps({ "aria-label": "图表指标" });
  expect(metrics.type).toBe("details");
  expect(metrics.props.open).not.toBe(true);
  await click("量价");
  expect(timeline.findAllByProps({ className: "sentiment-line-dot" })).toHaveLength(2);
  expect(timeline.findAllByType("polyline")).toHaveLength(2);
  await click("行业");
  expect(timeline.findAllByProps({ className: "sentiment-line-plot" })).toHaveLength(0);
  await act(async () => metrics.findByType("select").props.onChange({ target: { value: "volume" } }));
  expect(timeline.findAllByProps({ className: "sentiment-line-dot" })).toHaveLength(3);
});

it("makes the no-model analysis action open the shared settings dialog", async () => {
  postJson.mockImplementation(async (path: string) => path.endsWith("/snapshot") ? snapshot : path.endsWith("/latest") ? { analysis: null } : { items: [] });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} llmSettings={null} />); });
  const action = renderer!.root.findAllByType("button").find(node => textOf(node) === "配置模型")!;
  expect(action).toBeDefined();
  expect(action.props.disabled).not.toBe(true);
  await act(async () => action.props.onClick());
  expect(renderer!.root.findByProps({ "data-model-open-request": 1 })).toBeDefined();
});

it("preserves the followup draft and frozen origin through evidence and tab navigation", async () => {
  const frozen = { ...snapshot, evidence: [evidenceFixture("E1")] };
  installAnalysis(frozen, { evidence_ids: ["E1", "new-only"], stale: true }, { ...snapshot, snapshot_id: "fresh", evidence: [evidenceFixture("E1", "新来源E1"), evidenceFixture("new-only")] });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} llmSettings={{ active_provider_id: "fixture", providers: [{ id: "fixture", model: "fixture", base_url: "http://localhost" }] }} />); });
  await click("追问本次分析");
  const question = renderer!.root.findByProps({ id: "sentiment-question" });
  await act(async () => question.props.onChange({ target: { value: "先取得冻结回答" } }));
  await act(async () => renderer!.root.findByProps({ className: "sentiment-followup-form" }).props.onSubmit({ preventDefault: () => undefined }));
  await act(async () => question.props.onChange({ target: { value: "保留追问草稿" } }));
  const citation = renderer!.root.findByProps({ className: "sentiment-followup", hidden: false }).findByProps({ "aria-label": "查看证据 E1" });
  await act(async () => citation.props.onClick());
  expect(renderer!.root.findAllByProps({ role: "dialog" })).toHaveLength(1);
  const drawer = renderer!.root.findByProps({ "aria-label": "情绪证据详情", role: "dialog" });
  expect(textOf(drawer)).toContain("冻结来源E1");
  expect(textOf(drawer)).not.toContain("新来源E1");
  expect(renderer!.root.findByProps({ id: "sentiment-question" })).toBe(question);
  await click("返回追问");
  expect(renderer!.root.findByProps({ id: "sentiment-question" }).props.value).toBe("保留追问草稿");
  await click("消息");
  expect(renderer!.root.findAllByProps({ role: "dialog" })).toHaveLength(0);
  await click("情绪");
  expect(renderer!.root.findByProps({ id: "sentiment-question" }).props.value).toBe("保留追问草稿");
  await act(async () => renderer!.root.findByProps({ className: "sentiment-followup", hidden: false }).findByProps({ "aria-label": "查看证据 new-only" }).props.onClick());
  expect(textOf(renderer!.root.findByProps({ "aria-label": "情绪证据详情", role: "dialog" }))).toContain("无法核验证据");
  expect(textOf(renderer!.root.findByProps({ "aria-label": "情绪证据详情", role: "dialog" }))).not.toContain("冻结来源new-only");
});

it("discloses unavailable coverage without claiming healthy evidence or inventing counts", async () => {
  installAnalysis({ ...snapshot, quality_gates: {}, coverage: { ...snapshot.coverage, facts: undefined, price_days: Number.NaN } as unknown as SentimentSnapshot["coverage"] }, { sufficiency: "充分" });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />); });
  const coverage = renderer!.root.findByProps({ "aria-label": "数据覆盖" });
  expect(coverage.type).toBe("details");
  expect(coverage.props.open).not.toBe(true);
  expect(textOf(coverage.findByType("summary"))).toContain("覆盖待核验");
  expect(textOf(coverage)).toContain("事实 缺失");
  expect(textOf(coverage)).toContain("行情 缺失");
  expect(textOf(coverage)).not.toContain("NaN");
  expect(textOf(coverage)).toContain("质量门槛未提供");
});

it("keeps unavailable counts truthful in frozen aggregate citations", async () => {
  installAnalysis({ ...snapshot, coverage: { ...snapshot.coverage, facts: undefined, discussions: Number.NaN } as unknown as SentimentSnapshot["coverage"] }, { evidence_ids: ["M1"] });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />); });
  await act(async () => renderer!.root.findByProps({ "aria-label": "查看证据 M1" }).props.onClick());
  const drawer = renderer!.root.findByProps({ role: "dialog", "aria-label": "情绪证据详情" });
  expect(textOf(drawer)).toContain("事实 缺失 条，讨论 缺失 条");
  expect(textOf(drawer)).not.toContain("NaN");
});

it("focuses evidence controls during a followup drill-in and returns input selection and scroll", async () => {
  installAnalysis({ ...snapshot, evidence: [evidenceFixture("E1")] }, { evidence_ids: ["E1"] });
  const inputNode = { focus: vi.fn(), blur: vi.fn(), setSelectionRange: vi.fn(), selectionStart: 2, selectionEnd: 4 };
  const closeNode = { focus: vi.fn() };
  const scrollNode = { scrollTop: 230 };
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />, { createNodeMock: node => {
    const props = node.props as { id?: string; "aria-label"?: string; className?: string };
    return props.id === "sentiment-question" ? inputNode : props["aria-label"] === "关闭证据" ? closeNode : props.className === "sentiment-followup-scroll" ? scrollNode : null;
  } }); });
  await click("追问本次分析");
  scrollNode.scrollTop = 230;
  await act(async () => renderer!.root.findAllByProps({ "data-evidence-id": "E1" })[0].props.onClick());
  expect(closeNode.focus).toHaveBeenCalled();
  expect(inputNode.blur).toHaveBeenCalled();
  scrollNode.scrollTop = 0;
  await click("返回追问");
  expect(scrollNode.scrollTop).toBe(230);
  expect(inputNode.focus).toHaveBeenCalled();
  expect(inputNode.setSelectionRange).toHaveBeenLastCalledWith(2, 4);
});

it("shows followup failure beside its composer and retains the draft for retry", async () => {
  installAnalysis(snapshot);
  const fixtureApi = postJson.getMockImplementation()!;
  postJson.mockImplementation(async (path: string, body: unknown) => {
    if (path.endsWith("/followup")) throw new Error("fixture followup unavailable");
    return fixtureApi(path, body);
  });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} llmSettings={{ active_provider_id: "fixture", providers: [{ id: "fixture", model: "fixture", base_url: "http://localhost" }] }} />); });
  await click("追问本次分析");
  await act(async () => renderer!.root.findByProps({ id: "sentiment-question" }).props.onChange({ target: { value: "失败后重试" } }));
  await act(async () => renderer!.root.findByProps({ className: "sentiment-followup-form" }).props.onSubmit({ preventDefault: () => undefined }));
  const dialog = renderer!.root.findByProps({ role: "dialog", "aria-label": "围绕本次分析追问" });
  expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("fixture followup unavailable");
  expect(dialog.findByProps({ id: "sentiment-question" }).props.value).toBe("失败后重试");
  expect(dialog.findAllByType("button").find(button => textOf(button) === "追问")!.props.disabled).toBe(false);
});

it("exposes the actual title and summary when naming key and detailed evidence buttons", async () => {
  const evidence = evidenceFixture("E-accessible", "公司披露研发项目进展");
  installAnalysis({ ...snapshot, evidence: [evidence] }, { evidence_ids: [evidence.id] });
  await act(async () => { renderer = create(<SentimentPanel initialView="sentiment" initialCode={snapshot.stock_code} />); });
  const rows = renderer!.root.findAllByProps({ className: "sentiment-evidence-row", "data-evidence-id": evidence.id });
  expect(rows).toHaveLength(2); // Key evidence and the closed, reachable detailed source list.
  for (const row of rows) {
    // Native button name-from-content applies unless an aria-label overrides it.
    const accessibleName = row.props["aria-label"] || textOf(row);
    expect(accessibleName).toContain(evidence.title);
    expect(accessibleName).toContain(evidence.excerpt);
    expect(accessibleName).toContain(evidence.source_name);
  }
});
