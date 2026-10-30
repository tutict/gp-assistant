import type { ReactElement } from "react";
import { WorkspaceProvider, WorkspaceCredentialsReady } from "../../hooks/useWorkspace";
import { workspaceTestHarness } from "../../test/workspace";
let workspaceTest = workspaceTestHarness();
function create(element: ReactElement, options?: Parameters<typeof createRenderer>[1]) { return createRenderer(<WorkspaceProvider store={workspaceTest.store}>{element}</WorkspaceProvider>, options); }
import { renderToStaticMarkup } from "react-dom/server";
import { act, create as createRenderer, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ResearchMessage } from "../../types";

const tauriMocks = vi.hoisted(() => ({
  getJson: vi.fn(),
  postJson: vi.fn(),
  mobile: false,
}));
vi.mock("../../lib/tauri", () => ({
  getJson: tauriMocks.getJson,
  postJson: tauriMocks.postJson,
  isMobileTauriRuntime: () => tauriMocks.mobile,
}));
const { getJson, postJson } = tauriMocks;

import {
  EvidenceInspector,
  NewsRagPanel,
  NewsRagView,
  formatResearchUpdatedAt,
  researchOperationNotice,
  validateResearchFile,
} from "./NewsRagPanel";

const message: ResearchMessage = {
  id: "message-1",
  document_id: "document-1",
  stock_code: "600000.SH",
  title: "公司发布年度业绩预告",
  summary: "净利润预计同比增长。",
  sentiment: "positive",
  source_tier: "filing",
  source_name: "公司公告",
  published_at: "2026-07-21T08:00:00Z",
  unread: true,
};

const configuredLlmSettings = {
  active_provider_id: "test",
  providers: [{
    id: "test",
    name: "Test",
    provider: "openai-compatible",
    model: "test-model",
    api_key: "test-key",
  }],
};

function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(textOf).join("");
  if (value && typeof value === "object" && "children" in value) {
    return textOf((value as { children?: unknown }).children);
  }
  return "";
}

async function openQuestions(renderer: ReactTestRenderer) {
  await act(async () => renderer.root.findByProps({ "aria-label": "打开研究问答" }).props.onClick());
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

describe("NewsRagPanel", () => {
  beforeEach(() => {
    workspaceTest = workspaceTestHarness();
    tauriMocks.mobile = false;
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 1,
          chunk_count: 1,
          unread_count: 1,
          messages: [message],
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.startsWith("/api/research/messages")) {
        return Promise.resolve({ items: [message] });
      }
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    postJson.mockResolvedValue({});
    vi.stubGlobal("navigator", { onLine: true });
    vi.stubGlobal("document", { visibilityState: "visible", addEventListener: vi.fn(), removeEventListener: vi.fn() });
    vi.stubGlobal("window", {
      setInterval: vi.fn(() => 1),
      clearInterval: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    });
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  });

  afterEach(() => {
    vi.clearAllMocks();
    workspaceTest.store.dispose();
    vi.unstubAllGlobals();
  });

  it("keeps questions and historical answers out of the reading stream until requested", async () => {
    postJson.mockResolvedValue({ answers: [{ id: "a", question: "旧问题", answer: "历史答案", citations: [] }] });
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads")
      ? { items: [{ id: "thread", stock_code: "600000.SH", title: "研究", updated_at_epoch_ms: 1 }] }
      : path.includes("/messages") ? { items: [message] } : { document_count: 9 });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    expect(renderer.root.findAllByType("textarea")).toHaveLength(0);
    expect(textOf(renderer.toJSON())).not.toContain("历史答案");
    const stream = renderer.root.findByProps({ className: "research-stream" });
    expect(stream.findAllByProps({ className: "research-answers" })).toHaveLength(0);
    await act(async () => renderer.root.findByProps({ "aria-label": "打开研究问答" }).props.onClick());
    expect(textOf(renderer.toJSON())).toContain("历史答案");
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "未发送草稿" } }));
    await act(async () => renderer.root.findByProps({ "aria-label": "关闭研究问答" }).props.onClick());
    expect(renderer.root.findAllByType("textarea")).toHaveLength(0);
    await act(async () => renderer.root.findByProps({ "aria-label": "打开研究问答" }).props.onClick());
    expect(renderer.root.findByType("textarea").props.value).toBe("未发送草稿");
    await act(async () => renderer.unmount());
  });

  it("orders messages by actual publish instant, groups Shanghai dates, and puts unknown dates last", async () => {
    const items = [
      { ...message, id: "old", title: "旧消息", published_at: "2026-10-27T23:00:00+08:00" },
      { ...message, id: "unknown", title: "无日期", published_at: null },
      { ...message, id: "utc", title: "UTC晚间", published_at: "2026-10-28T20:00:00Z" },
      { ...message, id: "late", title: "同日较晚", published_at: "2026-10-29T13:00:00+08:00" },
      { ...message, id: "tie", title: "同刻消息", published_at: "2026-10-29T13:00:00+08:00" },
    ];
    getJson.mockImplementation(async (path: string) => path.includes("/messages") ? { items } : { items: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    const groups = renderer.root.findAll(node => node.type === "section" && String(node.props.className).startsWith("research-event-group"));
    expect(groups.map(group => textOf(group.findByType("header")))).toEqual(["2026-10-293", "2026-10-271", "日期未提供1"]);
    const events = renderer.root.findAll(node => node.type === "button" && String(node.props.className).startsWith("research-event "));
    expect(events.map(event => textOf(event.findByType("strong")))).toEqual(["同日较晚", "同刻消息", "UTC晚间", "旧消息", "无日期"]);
    expect(items.map(item => item.id)).toEqual(["old", "unknown", "utc", "late", "tie"]);
    await act(async () => events[0].props.onClick());
    await act(async () => events[1].props.onClick());
    expect(renderer.root.findAllByProps({ className: "research-event-detail" })).toHaveLength(1);
    expect(events.filter(event => event.props["aria-expanded"])).toHaveLength(1);
    await act(async () => renderer.unmount());
  });

  it("returns question evidence to its still-mounted question and keeps its draft", async () => {
    const citation = { citation_id: "C1", title: "证据标题", excerpt: "摘录", source_tier: "filing", source_name: "公告" };
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads") ? { items: [{ id: "thread", stock_code: "600000.SH", title: "研究" }] } : { items: [] });
    postJson.mockResolvedValue({ answers: [{ id: "a", question: "问题", answer: "回答[C1]", citations: [citation] }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await act(async () => renderer.root.findByProps({ "aria-label": "打开研究问答" }).props.onClick());
    const input = renderer.root.findByType("textarea");
    await act(async () => input.props.onChange({ target: { value: "证据前草稿" } }));
    await act(async () => renderer.root.findByProps({ className: "research-inline-citation" }).props.onClick());
    expect(renderer.root.findByProps({ className: "research-question-surface" }).props.hidden).toBe(true);
    expect(renderer.root.findByType("textarea")).toBe(input);
    await act(async () => renderer.root.findByProps({ "aria-label": "返回研究问答" }).props.onClick());
    expect(renderer.root.findByProps({ className: "research-question-surface" }).props.hidden).toBe(false);
    expect(renderer.root.findByType("textarea").props.value).toBe("证据前草稿");
    await act(async () => renderer.unmount());
  });

  it("retains citation history when returning to questions and selecting another visible citation", async () => {
    const citations = [
      { citation_id: "C1", title: "第一条证据", excerpt: "摘录一", source_tier: "filing", source_name: "公告" },
      { citation_id: "C2", title: "第二条证据", excerpt: "摘录二", source_tier: "filing", source_name: "公告" },
    ];
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads")
      ? { items: [{ id: "thread", stock_code: "600000.SH", title: "研究" }] } : { items: [] });
    postJson.mockResolvedValue({ answers: [{ id: "a", question: "问题", answer: "回答[C1][C2]", citations }] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "保留草稿" } }));
    await act(async () => renderer.root.findAllByProps({ className: "research-inline-citation" })[0].props.onClick());
    await act(async () => renderer.root.findByProps({ "aria-label": "返回研究问答" }).props.onClick());
    expect(renderer.root.findByProps({ className: "research-question-surface" }).props.hidden).toBe(false);
    await act(async () => renderer.root.findAllByProps({ className: "research-inline-citation" })[1].props.onClick());
    expect(textOf(renderer.root.findByProps({ "aria-label": "引用历史" }))).toContain("2 / 2");
    const previous = renderer.root.findByProps({ "aria-label": "上一条证据" });
    expect(previous.props.disabled).toBe(false);
    await act(async () => previous.props.onClick());
    expect(textOf(renderer.root.findByProps({ className: "research-evidence-card" }))).toContain("第一条证据");
    await act(async () => renderer.root.findByProps({ "aria-label": "返回研究问答" }).props.onClick());
    expect(renderer.root.findByType("textarea").props.value).toBe("保留草稿");
    await act(async () => renderer.unmount());
  });

  it("returns message evidence to the same single expanded message without opening questions", async () => {
    const citation = { citation_id: "C1", title: "公告证据", excerpt: "摘录", source_tier: "filing", source_name: "公告" };
    getJson.mockImplementation(async (path: string) => path.includes("/messages") ? { items: [{ ...message, citations: [citation] }] } : { items: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    const event = renderer.root.find(node => node.type === "button" && String(node.props.className).startsWith("research-event "));
    await act(async () => event.props.onClick());
    const detail = renderer.root.findByProps({ className: "research-event-detail" });
    await act(async () => detail.findByType("button").props.onClick());
    expect(renderer.root.findAllByType("textarea")).toHaveLength(0);
    await act(async () => renderer.root.findByProps({ "aria-label": "返回消息" }).props.onClick());
    expect(renderer.root.findByProps({ className: "research-event-detail" })).toBe(detail);
    expect(event.props["aria-expanded"]).toBe(true);
    expect(renderer.root.findAllByProps({ className: "research-evidence has-selection" })).toHaveLength(0);
    await act(async () => renderer.unmount());
  });

  it("keeps question drafts while the embedded view is hidden, and clears old answers and citations on a controlled stock switch", async () => {
    const pending = deferred<{ items: ResearchMessage[] }>();
    const citation = { citation_id: "C1", title: "旧股票证据", excerpt: "摘录", source_tier: "filing", source_name: "公告" };
    getJson.mockImplementation(async (path: string) => path.includes("stock_code=000001.SZ") ? pending.promise
      : path.endsWith("/threads") ? { items: [{ id: "thread-a", stock_code: "600000.SH", title: "股票A研究" }] } : { items: [message] });
    postJson.mockResolvedValue({ answers: [{ id: "a", question: "旧问题", answer: "旧股票回答[C1]", citations: [citation] }] });
    const onCodeChange = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel embedded code="600000.SH" onCodeChange={onCodeChange} />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "切换标签前的草稿" } }));
    await act(async () => renderer.update(<WorkspaceProvider store={workspaceTest.store}><NewsRagPanel embedded active={false} code="600000.SH" onCodeChange={onCodeChange} /></WorkspaceProvider>));
    expect(renderer.root.findAllByType("textarea")).toHaveLength(0);
    await act(async () => renderer.update(<WorkspaceProvider store={workspaceTest.store}><NewsRagPanel embedded active code="600000.SH" onCodeChange={onCodeChange} /></WorkspaceProvider>));
    expect(renderer.root.findByType("textarea").props.value).toBe("切换标签前的草稿");
    await act(async () => renderer.root.findByProps({ className: "research-inline-citation" }).props.onClick());
    await act(async () => renderer.update(<WorkspaceProvider store={workspaceTest.store}><NewsRagPanel embedded code="000001.SZ" onCodeChange={onCodeChange} /></WorkspaceProvider>));
    expect(textOf(renderer.toJSON())).not.toContain("旧股票回答");
    expect(textOf(renderer.toJSON())).not.toContain("旧股票证据");
    expect(renderer.root.findByType("textarea").props.value).toBe("切换标签前的草稿");
    await act(async () => pending.resolve({ items: [] }));
    expect(renderer.root.findAllByProps({ className: "research-daily-brief" })).toHaveLength(0);
    await act(async () => renderer.unmount());
  });

  it.each(["ctrlKey", "metaKey"])("submits from the question sheet with %s+Enter and ignores IME composition", async (modifier) => {
    postJson.mockImplementation(async (path: string) => path.endsWith("/create") ? { id: "keyboard-thread", stock_code: "600000.SH" } : { answer: "键盘回答", citations: [] });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    const input = renderer.root.findByType("textarea");
    await act(async () => input.props.onChange({ target: { value: "核查公告" } }));
    const event = { key: "Enter", [modifier]: true, preventDefault: vi.fn(), nativeEvent: { isComposing: false } };
    await act(async () => { input.props.onCompositionStart(); input.props.onKeyDown(event); });
    expect(postJson).not.toHaveBeenCalledWith("/api/research/query", expect.anything());
    await act(async () => { input.props.onCompositionEnd(); input.props.onKeyDown(event); });
    expect(postJson).toHaveBeenCalledWith("/api/research/query", expect.objectContaining({ query: "核查公告", stock_code: "600000.SH", thread_id: "keyboard-thread" }));
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    await act(async () => renderer.unmount());
  });

  it("announces failed queries inside the open question Sheet and preserves the draft for retry", async () => {
    let failed = true;
    postJson.mockImplementation(async (path: string) => {
      if (path.endsWith("/create")) return { id: "retry-thread", stock_code: "600000.SH", title: "重试会话" };
      if (path.endsWith("/query")) { if (failed) throw new Error("查询服务暂时离线"); return { answer: "重试后的回答", citations: [] }; }
      return {};
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "保留待核查的问题" } }));
    await act(async () => renderer.root.findByProps({ className: "research-composer" }).props.onSubmit({ preventDefault: vi.fn() }));
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("查询服务暂时离线");
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("提问失败");
    expect(dialog.findByType("textarea").props.value).toBe("保留待核查的问题");
    expect(dialog.findByProps({ className: "research-composer-send" }).props.disabled).toBe(false);
    failed = false;
    await act(async () => dialog.findByProps({ className: "research-composer" }).props.onSubmit({ preventDefault: vi.fn() }));
    expect(textOf(dialog)).toContain("重试后的回答");
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(postJson.mock.calls.filter(([path]) => path.endsWith("/query")).map(([, request]) => request.thread_id)).toEqual(["retry-thread", "retry-thread"]);
    await act(async () => renderer.unmount());
  });

  it("announces selected-thread load failures in the Sheet and retries the selected thread with the draft intact", async () => {
    const threads = [{ id: "a", stock_code: "600000.SH", title: "会话A" }, { id: "b", stock_code: "600000.SH", title: "会话B" }];
    let failed = true;
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads") ? { items: threads } : path.includes("/messages") ? { items: [message] } : {});
    postJson.mockImplementation(async (path: string, request: { thread_id?: string }) => {
      if (path.endsWith("/detail") && request.thread_id === "b") { if (failed) throw new Error("会话B读取失败"); return { answers: [{ id: "b-answer", question: "B问题", answer: "B历史回答", citations: [] }] }; }
      return { answers: [] };
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "会话切换草稿" } }));
    const selectB = renderer.root.find(node => node.type === "button" && node.props.className === "research-thread-main" && textOf(node).includes("会话B"));
    await act(async () => selectB.props.onClick());
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("会话B读取失败");
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("会话加载失败");
    expect(dialog.findByType("textarea").props.value).toBe("会话切换草稿");
    expect(workspaceTest.store.get("news.thread", "")).toBe("b");
    failed = false;
    await act(async () => dialog.findAllByType("button").find(button => textOf(button) === "重试加载会话")!.props.onClick());
    expect(textOf(dialog)).toContain("B历史回答");
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(dialog.findByType("textarea").props.value).toBe("会话切换草稿");
    await act(async () => renderer.unmount());
  });

  it.each(["create", "delete"])("shows %s conversation failures locally and keeps draft and selected context until retry succeeds", async (operation) => {
    const thread = { id: "kept-thread", stock_code: "600000.SH", title: "保留的会话" };
    let failed = true;
    let deleted = false;
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads") ? { items: deleted ? [] : [thread] } : path.includes("/messages") ? { items: [message] } : {});
    postJson.mockImplementation(async (path: string) => {
      if (path.endsWith("/detail")) return { answers: [{ id: "kept-answer", question: "既有问题", answer: "既有历史回答", citations: [] }] };
      if (path.endsWith(`/${operation}`)) { if (failed) throw new Error("会话操作暂时失败"); if (operation === "delete") { deleted = true; return {}; } return { id: "new-thread", stock_code: "600000.SH", title: "新会话" }; }
      return {};
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "保留会话操作草稿" } }));
    const action = () => renderer.root.findByProps({ "aria-label": operation === "create" ? "新建研究会话" : "删除研究会话：保留的会话" });
    if (operation === "delete") await act(async () => action().props.onClick());
    await act(async () => action().props.onClick());
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain(operation === "create" ? "新建会话失败" : "删除会话失败");
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("会话操作暂时失败");
    expect(dialog.findByType("textarea").props.value).toBe("保留会话操作草稿");
    expect(workspaceTest.store.get("news.thread", "")).toBe("kept-thread");
    expect(textOf(dialog)).toContain("既有历史回答");
    failed = false;
    if (operation === "delete") await act(async () => action().props.onClick());
    await act(async () => action().props.onClick());
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(dialog.findByType("textarea").props.value).toBe("保留会话操作草稿");
    expect(workspaceTest.store.get("news.thread", "")).toBe(operation === "create" ? "new-thread" : "");
    await act(async () => renderer.unmount());
  });

  it("does not present reading refresh errors as question failures", async () => {
    postJson.mockImplementation(async (path: string) => { if (path.endsWith("/refresh")) throw new Error("消息刷新失败"); return {}; });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await act(async () => renderer.root.find(node => node.type === "button" && textOf(node) === "更新").props.onClick());
    await openQuestions(renderer);
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(textOf(renderer.root.findByProps({ className: "research-error" }))).toContain("消息刷新失败");
    await act(async () => renderer.unmount());
  });

  it("ignores a rejected query from the prior stock instead of announcing it in the new stock question Sheet", async () => {
    let rejectQuery!: (reason: Error) => void;
    const query = new Promise((_, reject) => { rejectQuery = reject; });
    const threads = [{ id: "a", stock_code: "600000.SH", title: "A会话" }, { id: "b", stock_code: "000001.SZ", title: "B会话" }];
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads") ? { items: threads } : path.includes("/messages") ? { items: [] } : {});
    postJson.mockImplementation(async (path: string, request: { thread_id?: string }) => {
      if (path.endsWith("/query")) return query;
      if (path.endsWith("/detail")) return { answers: request.thread_id === "b" ? [{ id: "b-answer", question: "B问题", answer: "新股票历史回答", citations: [] }] : [] };
      return {};
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" watchlist={[{ code: "600000.SH", name: "A" }, { code: "000001.SZ", name: "B" }]} />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "仍未发送完的草稿" } }));
    await act(async () => renderer.root.findByProps({ className: "research-composer" }).props.onSubmit({ preventDefault: vi.fn() }));
    await act(async () => renderer.root.find(node => node.type === "button" && String(node.props.className).includes("research-stock-row") && textOf(node).includes("000001.SZ")).props.onClick());
    await act(async () => rejectQuery(new Error("旧股票查询失败")));
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(textOf(dialog)).toContain("新股票历史回答");
    expect(textOf(dialog)).not.toContain("旧股票查询失败");
    expect(dialog.findByType("textarea").props.value).toBe("仍未发送完的草稿");
    await act(async () => renderer.unmount());
  });

  it.each(["success", "rejection"])("keeps a new conversation's answers and draft when a prior thread retry settles with %s", async (settlement) => {
    const threadA = { id: "retry-a", stock_code: "600000.SH", title: "旧会话A" };
    const threadB = { id: "new-b", stock_code: "600000.SH", title: "新会话B" };
    let created = false;
    let aLoads = 0;
    let resolveRetry!: (value: unknown) => void;
    let rejectRetry!: (reason: Error) => void;
    const pendingRetry = new Promise((resolve, reject) => { resolveRetry = resolve; rejectRetry = reject; });
    getJson.mockImplementation(async (path: string) => path.endsWith("/threads") ? { items: created ? [threadB, threadA] : [threadA] } : path.includes("/messages") ? { items: [message] } : {});
    postJson.mockImplementation(async (path: string, request: { thread_id?: string }) => {
      if (path.endsWith("/detail") && request.thread_id === threadA.id) {
        aLoads += 1;
        if (aLoads === 1) throw new Error("A初次加载失败");
        return pendingRetry;
      }
      if (path.endsWith("/create")) { created = true; return threadB; }
      if (path.endsWith("/query")) return { answer: "B自己的回答", citations: [] };
      return { answers: [] };
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel initialCode="600000.SH" />); });
    await openQuestions(renderer);
    const dialog = renderer.root.findByProps({ role: "dialog", "aria-label": "研究问答" });
    expect(textOf(dialog.findByProps({ role: "alert" }))).toContain("A初次加载失败");
    await act(async () => dialog.findAllByType("button").find(button => textOf(button) === "重试加载会话")!.props.onClick());
    expect(aLoads).toBe(2);
    await act(async () => dialog.findByType("textarea").props.onChange({ target: { value: "创建B时保留的草稿" } }));
    await act(async () => dialog.findByProps({ "aria-label": "新建研究会话" }).props.onClick());
    expect(workspaceTest.store.get("news.thread", "")).toBe(threadB.id);
    expect(dialog.findByType("textarea").props.value).toBe("创建B时保留的草稿");
    await act(async () => dialog.findByProps({ className: "research-composer" }).props.onSubmit({ preventDefault: vi.fn() }));
    expect(postJson).toHaveBeenCalledWith("/api/research/query", expect.objectContaining({ thread_id: threadB.id }));
    await act(async () => dialog.findByType("textarea").props.onChange({ target: { value: "B新的未发送草稿" } }));
    await act(async () => settlement === "success"
      ? resolveRetry({ answers: [{ id: "a-answer", question: "A旧问题", answer: "A迟到的旧回答", citations: [] }] })
      : rejectRetry(new Error("A迟到的重试失败")));
    expect(workspaceTest.store.get("news.thread", "")).toBe(threadB.id);
    expect(textOf(dialog)).toContain("B自己的回答");
    expect(textOf(dialog)).not.toContain("A迟到的旧回答");
    expect(textOf(dialog)).not.toContain("A迟到的重试失败");
    expect(dialog.findAllByProps({ role: "alert" })).toHaveLength(0);
    expect(dialog.findAllByType("button").filter(button => textOf(button) === "重试加载会话")).toHaveLength(0);
    expect(dialog.findByType("textarea").props.value).toBe("B新的未发送草稿");
    await act(async () => renderer.unmount());
  });

  it("restores unsent question and thread while offline without submitting", async () => {
    workspaceTest.store.dispose();
    workspaceTest = workspaceTestHarness(undefined, { "news.question": "unsent question", "news.thread": "offline-thread" });
    getJson.mockRejectedValue(new Error("offline"));
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel />); });
    await openQuestions(renderer);
    expect(renderer.root.findByType("textarea").props.value).toBe("unsent question");
    expect(workspaceTest.store.get("news.thread", "")).toBe("offline-thread");
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => renderer.unmount());
  });

  it("does not overwrite question typed during asynchronous restore", async () => {
    const loading = deferred<import("../../lib/workspaceStore").WorkspaceSnapshot>();
    workspaceTest.transport.load = () => loading.promise;
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel />); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "typed now" } }));
    await act(async () => loading.resolve({ schemaVersion: 1, revision: 0, values: { "news.question": "older" } }));
    expect(renderer.root.findByType("textarea").props.value).toBe("typed now");
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => renderer.unmount());
  });

  it("blocks model submission but not drafting during credential initialization", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<WorkspaceCredentialsReady.Provider value={false}><NewsRagPanel /></WorkspaceCredentialsReady.Provider>); });
    await openQuestions(renderer);
    await act(async () => renderer.root.findByType("textarea").props.onChange({ target: { value: "local only" } }));
    const send = renderer.root.find(node => node.type === "button" && node.props.className === "research-composer-send");
    expect(send.props.disabled).toBe(true);
    expect(renderer.root.findByType("textarea").props.disabled).not.toBe(true);
    expect(postJson).not.toHaveBeenCalled();
    await act(async () => renderer.unmount());
  });

  it("formats the overview refresh timestamp for the desktop update label", () => {
    const text = formatResearchUpdatedAt({
      schema_version: 2,
      document_count: 1,
      chunk_count: 1,
      unread_count: 0,
      updated_at_epoch_ms: Date.UTC(2026, 0, 1, 0, 34),
      messages: [],
    });

    expect(text).toMatch(/^\d{2}:\d{2}$/);
  });

  it("loads the selected stock inbox and marks an unread event read", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={null}
          watchlist={[{ code: "600000.SH", name: "浦发银行" }]}
          initialCode="600000.SH"
        />,
      );
    });

    expect(textOf(renderer.toJSON())).toContain("公司发布年度业绩预告");
    expect(getJson).toHaveBeenCalledWith(
      "/api/research/messages?stock_code=600000.SH&limit=120",
    );
    const event = renderer.root.find((node) =>
      node.type === "button" &&
      typeof node.props.className === "string" &&
      node.props.className.includes("research-event"),
    );
    expect(textOf(event)).toContain("公司公告");
    expect(textOf(event)).toContain("公司发布年度业绩预告");
    expect(textOf(event)).toContain("净利润预计同比增长。");
    await act(async () => {
      await event.props.onClick();
    });
    expect(textOf(renderer.root.findByProps({ className: "research-event-detail" })))
      .toContain("净利润预计同比增长。");
    expect(postJson).toHaveBeenCalledWith("/api/research/mark-read", {
      message_ids: ["message-1"],
    });
    expect(event.props.className).not.toContain("unread");
    await act(async () => {
      renderer.unmount();
    });
  });

  it("marks a stock read using its inbox count after selecting it", async () => {
    const otherMessage: ResearchMessage = {
      ...message,
      id: "message-2",
      stock_code: "000001.SZ",
      title: "平安银行公告",
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) return Promise.resolve({
        schema_version: 2,
        document_count: 2,
        chunk_count: 2,
        unread_count: 2,
        unread_by_stock: { "600000.SH": 1, "000001.SZ": 1 },
        retrieval: { vector: { ready: false } },
      });
      if (path.includes("stock_code=600000.SH")) return Promise.resolve({ items: [message] });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [otherMessage] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null}
        watchlist={[
          { code: "600000.SH", name: "浦发银行" },
          { code: "000001.SZ", name: "平安银行" },
        ]}
        initialCode="600000.SH" />);
    });

    const target = renderer.root.find(node => node.type === "button" && String(node.props.className).includes("research-stock-row") && textOf(node).includes("000001.SZ"));
    await act(async () => target.props.onClick());
    const markRead = renderer.root.find(node => node.type === "button" && textOf(node) === "全部已读");
    await act(async () => { markRead.props.onClick(); });

    expect(postJson).toHaveBeenCalledWith("/api/research/mark-read", {
      stock_code: "000001.SZ",
    });
    expect(renderer.root.findAllByProps({
      "aria-label": "标记 000001.SZ 全部已读",
    })).toHaveLength(0);
    await act(async () => { renderer.unmount(); });
  });

  it("closes the evidence inspector with Escape and exposes a desktop close control", async () => {
    const citation = {
      citation_id: "C1",
      title: "公告证据",
      excerpt: "公告摘录",
      source_tier: "filing",
      source_name: "公司公告",
    };
    const thread = {
      id: "thread-evidence",
      title: "证据研究",
      stock_code: "600000.SH",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 2,
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) return Promise.resolve({
        schema_version: 2,
        document_count: 1,
        chunk_count: 1,
        unread_count: 0,
        retrieval: { vector: { ready: false } },
      });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [thread] });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/detail") {
        return Promise.resolve({ answers: [{
          id: "answer-evidence",
          question: "问题",
          answer: "回答 [C1]",
          citations: [citation],
        }] });
      }
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null}
        watchlist={[{ code: "600000.SH", name: "浦发银行" }]}
        initialCode="600000.SH" />);
    });
    expect(renderer.root.findAllByProps({ className: "research-evidence" })).toHaveLength(0);
    await openQuestions(renderer);
    const citationButton = renderer.root.findByProps({ className: "research-inline-citation" });
    await act(async () => { citationButton.props.onClick(); });
    const close = renderer.root.findByProps({ "aria-label": "关闭证据检查器" });
    expect(close.props.className).not.toContain("research-mobile-close");
    expect(renderer.root.findByProps({ className: "research-evidence has-selection" })).toBeTruthy();
    const keydown = vi.mocked(document.addEventListener).mock.calls.filter(([type]) => type === "keydown").at(-1)?.[1] as (event: unknown) => void;
    await act(async () => keydown({ key: "Escape", preventDefault: vi.fn(), stopPropagation: vi.fn() }));
    expect(renderer.root.findAllByProps({ className: "research-evidence has-selection" })).toHaveLength(0);
    expect(renderer.root.findByProps({ className: "research-question-surface" }).props.hidden).toBe(false);
    await act(async () => { renderer.unmount(); });
  });

  it("scrolls the newest answer card into view", async () => {
    const thread = {
      id: "thread-scroll",
      title: "滚动研究",
      stock_code: "600000.SH",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 2,
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) return Promise.resolve({
        schema_version: 2,
        document_count: 1,
        chunk_count: 1,
        unread_count: 0,
        retrieval: { vector: { ready: false } },
      });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [thread] });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/detail") return Promise.resolve({ answers: [] });
      if (path === "/api/research/query") return Promise.resolve({ answer: "最新回答", citations: [] });
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    let answerMock: { scrollIntoView: ReturnType<typeof vi.fn>; className?: string } | undefined;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={configuredLlmSettings}
        watchlist={[{ code: "600000.SH", name: "浦发银行" }]}
        initialCode="600000.SH" />, {
        createNodeMock: (element) => {
          const props = element.props as { className?: string };
          if (element.type === "article") {
            answerMock ||= { scrollIntoView: vi.fn(), className: props.className };
            answerMock.className = props.className;
            return answerMock;
          }
          return { scrollIntoView: vi.fn(), className: props.className, querySelectorAll: () => [], querySelector: () => null, focus: vi.fn() };
        },
      });
    });
    await openQuestions(renderer);
    const composer = renderer.root.findByProps({ className: "research-composer" });
    await act(async () => {
      composer.findByType("textarea").props.onChange({ target: { value: "新问题" } });
    });
    await act(async () => {
      composer.props.onSubmit({ preventDefault: vi.fn() });
      await Promise.resolve();
      await Promise.resolve();
    });
    const answer = renderer.root.find((node) => node.type === "article"
      && node.props.className.includes("research-answer"));
    expect(answerMock).toBe(answer.instance);
    expect(answerMock?.scrollIntoView).toHaveBeenCalledWith({ block: "end", behavior: "smooth" });
    await act(async () => { renderer.unmount(); });
  });

  it("labels official policy messages with their mapping scope", async () => {
    const policyMessage: ResearchMessage = {
      ...message,
      id: "policy-message",
      title: "国务院发布电池产业支持政策",
      source_tier: "policy_official",
      scope_type: "industry",
      scope_tags: ["电池"],
      mapped_stock_codes: ["600000.SH"],
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) return Promise.resolve({
        schema_version: 2, document_count: 1, chunk_count: 1, unread_count: 1,
        messages: [policyMessage], retrieval: { vector: { ready: false } },
      });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [policyMessage] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null}
        watchlist={[{ code: "600000.SH", name: "浦发银行" }]} initialCode="600000.SH" />);
    });
    const policyEvent = renderer.root.find((node) => node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-event"));
    expect(textOf(policyEvent)).not.toContain("官方政策");
    expect(textOf(policyEvent)).not.toContain("行业 · 电池");
    expect(textOf(renderer.toJSON())).toContain("公司公告");
    await act(async () => { await policyEvent.props.onClick(); });
    const details = renderer.root.findByProps({ className: "research-event-detail" });
    expect(textOf(details)).toContain("官方政策");
    expect(textOf(details)).toContain("行业");
    expect(textOf(details)).toContain("电池");
    await act(async () => { renderer.unmount(); });
  });

  it("exposes a visible label for the research question input", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    await openQuestions(renderer);
    const textarea = renderer.root.findByType("textarea");
    expect(textarea.props.id).toBeTruthy();
    const label = renderer.root.find((node) =>
      node.type === "label" && node.props.htmlFor === textarea.props.id
    );
    expect(textOf(label)).toContain("研究问题");
    await act(async () => { renderer.unmount(); });
  });

  it("moves the research-only risk boundary to the send affordance and empty footer", async () => {
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) return Promise.resolve({
        schema_version: 2, document_count: 0, chunk_count: 0, unread_count: 0,
        retrieval: { vector: { ready: false } },
      });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    await openQuestions(renderer);
    const send = renderer.root.find((node) => node.type === "button"
      && node.props.className === "research-composer-send");
    expect(send.props.title).toContain("仅供研究，不构成投资建议");
    expect(send.props["aria-label"]).toContain("仅供研究，不构成投资建议");
    expect(renderer.root.findAllByProps({ className: "research-empty-boundary" })).toHaveLength(0);
    expect(textOf(renderer.root.findAllByProps({ className: "research-risk-boundary" })[0]))
      .toBe("仅供研究，不构成投资建议。");
    await act(async () => { renderer.unmount(); });
  });

  it("omits the inner research title bar when the workspace header owns the actions", async () => {
    const onHeaderToolsChange = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} embedded onHeaderToolsChange={onHeaderToolsChange} />);
    });
    expect(renderer.root.findAllByProps({ className: "research-topbar" })).toHaveLength(0);
    expect(textOf(renderer.toJSON())).not.toContain("BM25");
    expect(renderer.root.findAllByProps({ className: "research-eyebrow" })).toHaveLength(0);
    expect(textOf(renderer.root.findByProps({ className: "research-risk-boundary" }))).toBe("仅供研究，不构成投资建议。");
    expect(onHeaderToolsChange).toHaveBeenCalled();
    const tools = onHeaderToolsChange.mock.calls.at(-1)?.[0];
    expect(tools.vectorReady).toBe(false);
    expect(typeof tools.onRefresh).toBe("function");
    expect(typeof tools.onOpenKnowledge).toBe("function");
    await act(async () => { renderer.unmount(); });
  });

  it("uses a single-row mobile composer with an embedded send button", async () => {
    Object.assign(window, { matchMedia: vi.fn(() => ({matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn()})) });
    tauriMocks.mobile = true;
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    await openQuestions(renderer);
    const row = renderer.root.findByProps({ className: "research-composer-row" });
    expect(row.findAllByType("textarea")).toHaveLength(1);
    expect(row.findAll((node) => node.type === "button"
      && node.props.className === "research-composer-send")).toHaveLength(1);
    expect(renderer.root.findByType("textarea").props.placeholder).toContain("询问公告、财务或消息");
    await act(async () => { renderer.unmount(); });
  });

  it("keeps the daily brief collapsed behind one native disclosure on every viewport", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    const brief = renderer.root.find((node) => node.type === "details"
      && node.props.className === "research-daily-brief");
    expect(brief.props.open).toBeUndefined();
    expect(textOf(brief.findByType("summary"))).toContain("今日新增 0 条");
    expect(textOf(brief.findByType("summary"))).not.toContain("文档");
    expect(textOf(brief.findByType("summary"))).not.toContain("利好");
    expect(renderer.root.findAll((node) => node.props.className === "research-daily-brief")).toHaveLength(1);
    await act(async () => { renderer.unmount(); });
  });

  it("keeps the compatibility evidence view available to the agent page", () => {
    const html = renderToStaticMarkup(
      <NewsRagView
        result={{
          sentiment_groups: {
            positive: [{ title: "公告证据", summary: "已披露事项", source_tier: "filing" }],
            negative: [],
            mixed: [],
            uncertain: [],
          },
        } as never}
      />,
    );
    expect(html).toContain("消息证据");
    expect(html).toContain("公告证据");
  });

  it("queries local research evidence and shows the risk boundary without a model", async () => {
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/create") {
        return Promise.resolve({
          id: "thread-local",
          title: "本地研究",
          stock_code: "",
          created_at_epoch_ms: 1,
          updated_at_epoch_ms: 1,
        });
      }
      if (path === "/api/research/query") {
        return Promise.resolve({
          id: "answer-local",
          answer: "本地证据回答",
          citations: [],
        });
      }
      return Promise.resolve({});
    });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });
    await openQuestions(renderer);
    expect(textOf(renderer.toJSON())).toContain("未配置模型，将只用已导入资料回答");

    const composer = renderer.root.find((node) => node.type === "form"
      && node.props.className === "research-composer");
    await act(async () => {
      composer.findByType("textarea").props.onChange({ target: { value: "hi" } });
    });
    await act(async () => {
      await composer.props.onSubmit({ preventDefault: vi.fn() });
    });

    expect(postJson).toHaveBeenCalledWith("/api/research/threads/create", {
      title: "综合研究",
      stock_code: null,
    });
    const queryCall = postJson.mock.calls.find(([path]) => path === "/api/research/query");
    expect(queryCall?.[1]).toMatchObject({
      query: "hi",
      stock_code: null,
      thread_id: "thread-local",
      top_k: 8,
    });
    expect(queryCall?.[1]?.llm).toBeUndefined();
    expect(textOf(renderer.toJSON())).toContain("本地证据回答");
    expect(textOf(renderer.toJSON())).toContain("仅供研究，不构成投资建议。");
    expect(textOf(renderer.toJSON())).not.toContain("BM25");
    await act(async () => { renderer.unmount(); });
  });

  it("offers pack import and rollback on Android without desktop-only maintenance", async () => {
    tauriMocks.mobile = true;
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={null}
          watchlist={[{ code: "600000.SH", name: "浦发银行" }]}
          initialCode="600000.SH"
        />,
      );
    });

    await act(async () => renderer.root.findByProps({ "aria-label": "更多研究操作" }).props.onClick());
    const knowledgeButton = renderer.root.find(node => node.type === "button" && textOf(node) === "知识库管理");
    await act(async () => knowledgeButton.props.onClick());
    const drawerText = textOf(renderer.toJSON());
    expect(drawerText).toContain("导入 SQLite v2 / 旧版包");
    expect(drawerText).toContain("回滚导入");
    expect(drawerText).not.toContain("公网 HTTPS 地址");
    expect(drawerText).not.toContain("重建 FTS");
    expect(drawerText).not.toContain("生成向量");
    await act(async () => {
      renderer.unmount();
    });
  });

  it("rejects oversized files before reading them", () => {
    expect(() => validateResearchFile(
      { name: "too-large.pdf", size: 25 * 1024 * 1024 + 1 },
      "/api/research/import-pdf",
    )).toThrow("25 MB");
    expect(() => validateResearchFile(
      { name: "portable.sqlite", size: 25 * 1024 * 1024 + 1 },
      "/api/research/pack/import",
    )).not.toThrow();
    expect(() => validateResearchFile(
      { name: "too-large.sqlite", size: 64 * 1024 * 1024 + 1 },
      "/api/research/pack/import",
    )).toThrow("64 MB");
  });

  it("surfaces skipped PDF pages as an OCR warning", () => {
    expect(researchOperationNotice({
      kind: "pdf",
      page_count: 4,
      extracted_page_count: 2,
      skipped_pages: [2, 4],
      ocr_required: true,
    })).toEqual({
      tone: "warning",
      text: "已导入 2/4 页；第 2、4 页没有可提取文本，需要 OCR 后重新导入。",
    });
  });

  it("does not let a slower previous stock request replace the current workspace", async () => {
    const pendingB = deferred<{ items: ResearchMessage[] }>();
    const messageB: ResearchMessage = {
      ...message,
      id: "message-b",
      stock_code: "000001.SZ",
      title: "平安银行旧请求结果",
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 2,
          chunk_count: 2,
          unread_count: 2,
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.includes("stock_code=000001.SZ")) return pendingB.promise;
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [message] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={null}
          watchlist={[
            { code: "600000.SH", name: "浦发银行" },
            { code: "000001.SZ", name: "平安银行" },
          ]}
          initialCode="600000.SH"
        />,
      );
    });
    const stockButton = (code: string) => renderer.root.find((node) =>
      node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes(code),
    );
    await act(async () => { stockButton("000001.SZ").props.onClick(); });
    await act(async () => { stockButton("600000.SH").props.onClick(); });
    pendingB.resolve({ items: [messageB] });
    await act(async () => { await pendingB.promise; });

    const rendered = textOf(renderer.toJSON());
    expect(rendered).toContain("公司发布年度业绩预告");
    expect(rendered).not.toContain("平安银行旧请求结果");
    await act(async () => { renderer.unmount(); });
  });

  it("clears the prior stock evidence while the newly selected inbox is loading", async () => {
    const pendingMessages = deferred<{ items: ResearchMessage[] }>();
    const messageB: ResearchMessage = {
      ...message,
      id: "message-b-loading",
      stock_code: "000001.SZ",
      title: "平安银行新股票消息",
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 1,
          chunk_count: 1,
          unread_count: 1,
          messages: [message],
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.includes("stock_code=000001.SZ")) return pendingMessages.promise;
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [message] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} watchlist={[
        { code: "600000.SH", name: "浦发银行" },
        { code: "000001.SZ", name: "平安银行" },
      ]} initialCode="600000.SH" />);
    });
    expect(textOf(renderer.toJSON())).toContain("公司发布年度业绩预告");

    const targetStock = renderer.root.find((node) => node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes("000001.SZ"));
    await act(async () => { targetStock.props.onClick(); });

    const loadingMarkup = textOf(renderer.toJSON());
    expect(loadingMarkup).toContain("正在打开研究消息中心");
    expect(loadingMarkup).not.toContain("公司发布年度业绩预告");
    expect(renderer.root.findAllByProps({ className: "research-empty" })).toHaveLength(0);
    await act(async () => { pendingMessages.resolve({ items: [messageB] }); });
    expect(textOf(renderer.toJSON())).toContain("平安银行新股票消息");
    await act(async () => { renderer.unmount(); });
  });

  it("does not let an old manual refresh reclaim the workspace after switching stocks", async () => {
    const pendingRefresh = deferred<Record<string, never>>();
    const messageB: ResearchMessage = {
      ...message,
      id: "message-b-current",
      stock_code: "000001.SZ",
      title: "平安银行当前消息",
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 2,
          chunk_count: 2,
          unread_count: 2,
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.includes("stock_code=000001.SZ")) return Promise.resolve({ items: [messageB] });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [message] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/refresh") return pendingRefresh.promise;
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={null}
          watchlist={[
            { code: "600000.SH", name: "浦发银行" },
            { code: "000001.SZ", name: "平安银行" },
          ]}
          initialCode="600000.SH"
        />,
      );
    });
    const refreshButton = renderer.root.find((node) =>
      node.type === "button" && textOf(node) === "更新",
    );
    await act(async () => { refreshButton.props.onClick(); });
    const targetStock = renderer.root.find((node) =>
      node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes("000001.SZ"),
    );
    await act(async () => { targetStock.props.onClick(); });
    pendingRefresh.resolve({});
    await act(async () => { await pendingRefresh.promise; });

    const rendered = textOf(renderer.toJSON());
    expect(rendered).toContain("平安银行当前消息");
    expect(rendered).not.toContain("公司发布年度业绩预告");
    await act(async () => { renderer.unmount(); });
  });

  it("does not append a slower previous-stock answer after switching stocks", async () => {
    const pendingAnswer = deferred<{ answer: string; citations: never[] }>();
    const threadA = {
      id: "thread-a",
      title: "浦发研究",
      stock_code: "600000.SH",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 2,
    };
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 1,
          chunk_count: 1,
          unread_count: 0,
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [threadA] });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/detail") return Promise.resolve({ answers: [] });
      if (path === "/api/research/query") return pendingAnswer.promise;
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={configuredLlmSettings}
          watchlist={[
            { code: "600000.SH", name: "浦发银行" },
            { code: "000001.SZ", name: "平安银行" },
          ]}
          initialCode="600000.SH"
        />,
      );
    });
    await openQuestions(renderer);
    const composer = renderer.root.find((node) => node.type === "form"
      && node.props.className === "research-composer");
    const textarea = composer.findByType("textarea");
    await act(async () => { textarea.props.onChange({ target: { value: "旧股票问题" } }); });
    await act(async () => { composer.props.onSubmit({ preventDefault: vi.fn() }); });
    const targetStock = renderer.root.find((node) =>
      node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes("000001.SZ"),
    );
    await act(async () => { targetStock.props.onClick(); });
    pendingAnswer.resolve({ answer: "不应出现的旧股票回答", citations: [] });
    await act(async () => { await pendingAnswer.promise; });

    expect(textOf(renderer.toJSON())).not.toContain("不应出现的旧股票回答");
    await act(async () => { renderer.unmount(); });
  });

  it("does not continue a query when its thread creation becomes stale", async () => {
    const pendingThread = deferred<{
      id: string;
      title: string;
      stock_code: string;
      created_at_epoch_ms: number;
      updated_at_epoch_ms: number;
    }>();
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/create") return pendingThread.promise;
      return Promise.resolve({});
    });
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={configuredLlmSettings}
          watchlist={[
            { code: "600000.SH", name: "浦发银行" },
            { code: "000001.SZ", name: "平安银行" },
          ]}
          initialCode="600000.SH"
        />,
      );
    });
    await openQuestions(renderer);
    const composer = renderer.root.find((node) => node.type === "form"
      && node.props.className === "research-composer");
    await act(async () => {
      composer.findByType("textarea").props.onChange({ target: { value: "需要新会话" } });
    });
    await act(async () => { composer.props.onSubmit({ preventDefault: vi.fn() }); });
    const targetStock = renderer.root.find((node) =>
      node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes("000001.SZ"),
    );
    await act(async () => { targetStock.props.onClick(); });
    pendingThread.resolve({
      id: "stale-thread",
      title: "过期会话",
      stock_code: "600000.SH",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 1,
    });
    await act(async () => { await pendingThread.promise; });

    expect(postJson).not.toHaveBeenCalledWith("/api/research/query", expect.anything());
    expect(textOf(renderer.toJSON())).not.toContain("过期会话");
    await act(async () => { renderer.unmount(); });
  });

  it("loads the matching research thread after switching stocks", async () => {
    const threads = [
      {
        id: "thread-a",
        title: "浦发研究",
        stock_code: "600000.SH",
        created_at_epoch_ms: 1,
        updated_at_epoch_ms: 2,
      },
      {
        id: "thread-b",
        title: "平安研究",
        stock_code: "000001.SZ",
        created_at_epoch_ms: 1,
        updated_at_epoch_ms: 3,
      },
    ];
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({
          schema_version: 2,
          document_count: 1,
          chunk_count: 1,
          unread_count: 0,
          messages: [],
          retrieval: { vector: { ready: false } },
        });
      }
      if (path.startsWith("/api/research/messages")) {
        return Promise.resolve({ items: [] });
      }
      if (path === "/api/research/threads") return Promise.resolve({ items: threads });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/detail") return Promise.resolve({ answers: [] });
      return Promise.resolve({});
    });

    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <NewsRagPanel
          llmSettings={null}
          watchlist={[
            { code: "600000.SH", name: "浦发银行" },
            { code: "000001.SZ", name: "平安银行" },
          ]}
          initialCode="600000.SH"
        />,
      );
    });
    expect(postJson).toHaveBeenCalledWith("/api/research/threads/detail", {
      thread_id: "thread-a",
    });

    const targetStock = renderer.root.find((node) =>
      node.type === "button"
      && typeof node.props.className === "string"
      && node.props.className.includes("research-stock-row")
      && textOf(node).includes("000001.SZ"),
    );
    await act(async () => {
      targetStock.props.onClick();
    });

    expect(postJson).toHaveBeenCalledWith("/api/research/threads/detail", {
      thread_id: "thread-b",
    });
    await act(async () => {
      renderer.unmount();
    });
  });

  it("deletes a research thread after confirmation and clears its active answers", async () => {
    const thread = {
      id: "thread-delete",
      title: "待删除会话",
      stock_code: "",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 2,
    };
    let deleted = false;
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({ schema_version: 2, document_count: 0, chunk_count: 0,
          unread_count: 0, retrieval: { vector: { ready: false } } });
      }
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: deleted ? [] : [thread] });
      return Promise.resolve({});
    });
    postJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads/detail") {
        return Promise.resolve({ answers: [{ id: "answer-1", question: "旧问题", answer: "旧回答", citations: [] }] });
      }
      if (path === "/api/research/threads/delete") {
        deleted = true;
        return Promise.resolve({ deleted: 1 });
      }
      return Promise.resolve({});
    });
    vi.stubGlobal("window", { setInterval: vi.fn(() => 1), clearInterval: vi.fn() });

    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });
    await openQuestions(renderer);
    expect(textOf(renderer.toJSON())).toContain("旧回答");
    const deleteButton = renderer.root.findByProps({
      "aria-label": "删除研究会话：待删除会话",
    });
    await act(async () => { deleteButton.props.onClick(); });
    expect(textOf(renderer.toJSON())).toContain("确认删除？");
    await act(async () => { deleteButton.props.onClick(); });
    expect(postJson).toHaveBeenCalledWith("/api/research/threads/delete", {
      thread_id: "thread-delete",
    });
    expect(textOf(renderer.toJSON())).not.toContain("旧回答");
    await act(async () => { renderer.unmount(); });
  });

  it("requires a second in-app confirmation before deleting a research thread", async () => {
    const thread = {
      id: "thread-cancel",
      title: "保留会话",
      stock_code: "",
      created_at_epoch_ms: 1,
      updated_at_epoch_ms: 2,
    };
    getJson.mockImplementation((path: string) => {
      if (path === "/api/research/threads") return Promise.resolve({ items: [thread] });
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({ schema_version: 2, document_count: 0, chunk_count: 0,
          unread_count: 0, retrieval: { vector: { ready: false } } });
      }
      return Promise.resolve({});
    });
    vi.stubGlobal("window", { setInterval: vi.fn(() => 1), clearInterval: vi.fn() });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });
    await openQuestions(renderer);
    const deleteButton = renderer.root.findByProps({
      "aria-label": "删除研究会话：保留会话",
    });
    await act(async () => { deleteButton.props.onClick(); });
    expect(textOf(renderer.toJSON())).toContain("确认删除？");
    expect(postJson).not.toHaveBeenCalledWith("/api/research/threads/delete", expect.anything());
    expect(textOf(renderer.toJSON())).toContain("保留会话");
    await act(async () => { renderer.unmount(); });
  });

  it("sends an empty watchlist to screening instead of asking for a stock that is not there", async () => {
    getJson.mockImplementation((path: string) => {
      if (path.startsWith("/api/research/overview")) {
        return Promise.resolve({ schema_version: 2, document_count: 0, chunk_count: 0, unread_count: 0, retrieval: { vector: { ready: false } } });
      }
      if (path.startsWith("/api/research/messages")) return Promise.resolve({ items: [] });
      if (path === "/api/research/threads") return Promise.resolve({ items: [] });
      return Promise.resolve({});
    });
    const onGoToScreen = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} watchlist={[]} onGoToScreen={onGoToScreen} />);
    });
    await act(async () => { await Promise.resolve(); });
    const rendered = textOf(renderer.toJSON());
    expect(rendered).toContain("先去选股");
    expect(rendered).toContain("去选股收藏");
    expect(rendered).not.toContain("选择一只自选股并立即更新");
    const button = renderer.root.find((node) => node.type === "button" && textOf(node).includes("去选股收藏"));
    await act(async () => { button.props.onClick(); });
    expect(onGoToScreen).toHaveBeenCalled();
    await act(async () => { renderer.unmount(); });
  });

});

it("labels historical citations whose original evidence is unavailable", () => {
  const citation = {citation_id:"C1",document_id:"d1",chunk_id:"c1",title:"历史证据",excerpt:"旧内容",source_tier:"filing" as const,source_name:"公告",lexical_score:1,retrieval_score:1,unavailable:true};
  expect(renderToStaticMarkup(<EvidenceInspector citation={citation} />)).toContain("原证据不可用");
});
