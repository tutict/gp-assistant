import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
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
  NewsRagPanel,
  NewsRagView,
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

describe("NewsRagPanel", () => {
  beforeEach(() => {
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
    vi.stubGlobal("document", { visibilityState: "visible" });
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
    vi.unstubAllGlobals();
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
    await act(async () => {
      await event.props.onClick();
    });
    expect(postJson).toHaveBeenCalledWith("/api/research/mark-read", {
      message_ids: ["message-1"],
    });
    expect(event.props.className).not.toContain("unread");
    await act(async () => {
      renderer.unmount();
    });
  });

  it("marks a non-selected stock read using its inbox count", async () => {
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

    const markRead = renderer.root.findByProps({
      "aria-label": "标记 000001.SZ 全部已读",
    });
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
    const close = renderer.root.findByProps({ "aria-label": "关闭证据检查器" });
    expect(close.props.className).not.toContain("research-mobile-close");
    const citationButton = renderer.root.findByProps({ className: "research-inline-citation" });
    await act(async () => { citationButton.props.onClick(); });
    expect(renderer.root.findByProps({ className: "research-evidence has-selection" })).toBeTruthy();

    const keydownCalls = (window.addEventListener as unknown as { mock: { calls: unknown[][] } }).mock.calls;
    const keydown = keydownCalls.filter(([type]) => type === "keydown").at(-1)?.[1] as ((event: unknown) => void) | undefined;
    expect(keydown).toBeTypeOf("function");
    await act(async () => { keydown?.({ key: "Escape", preventDefault: vi.fn() }); });
    expect(renderer.root.findByProps({ className: "research-evidence" })).toBeTruthy();
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
          return { scrollIntoView: vi.fn(), className: props.className };
        },
      });
    });
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
    expect(textOf(renderer.toJSON())).toContain("官方政策");
    expect(textOf(renderer.toJSON())).toContain("行业 · 电池");
    expect(textOf(renderer.toJSON())).toContain("公司公告");
    await act(async () => { renderer.unmount(); });
  });

  it("exposes a visible label for the research question input", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

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

    const send = renderer.root.find((node) => node.type === "button"
      && node.props.className === "research-composer-send");
    expect(send.props.title).toContain("仅供研究，不构成投资建议");
    expect(send.props["aria-label"]).toContain("仅供研究，不构成投资建议");
    expect(textOf(renderer.root.findByProps({ className: "research-empty-boundary" })))
      .toBe("仅供研究，不构成投资建议。");
    expect(renderer.root.findAllByProps({ className: "research-risk-boundary" }))
      .toHaveLength(0);
    await act(async () => { renderer.unmount(); });
  });

  it("uses a single-row mobile composer with an embedded send button", async () => {
    tauriMocks.mobile = true;
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    const row = renderer.root.findByProps({ className: "research-composer-row" });
    expect(row.findAllByType("textarea")).toHaveLength(1);
    expect(row.findAll((node) => node.type === "button"
      && node.props.className === "research-composer-send")).toHaveLength(1);
    expect(renderer.root.findByType("textarea").props.placeholder).toContain("询问公告、财务或消息");
    await act(async () => { renderer.unmount(); });
  });

  it("renders the daily brief as a collapsed native disclosure", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    const brief = renderer.root.find((node) => node.type === "details"
      && node.props.className === "research-daily-brief research-daily-brief-mobile");
    const desktopBrief = renderer.root.find((node) => node.type === "section"
      && node.props.className === "research-daily-brief research-daily-brief-desktop");
    expect(brief.props.open).toBeUndefined();
    expect(textOf(brief.findByType("summary"))).toContain("今日摘要");
    expect(textOf(brief.findByType("summary"))).toContain("利好");
    expect(textOf(desktopBrief)).toContain("今日摘要");
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

  it("shows an API setup prompt and skips research queries without a model", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });

    const composer = renderer.root.find((node) => node.type === "form"
      && node.props.className === "research-composer");
    await act(async () => {
      composer.findByType("textarea").props.onChange({ target: { value: "hi" } });
    });
    await act(async () => {
      composer.props.onSubmit({ preventDefault: vi.fn() });
      await Promise.resolve();
    });

    expect(textOf(renderer.toJSON())).toContain("请先配置 API 和模型");
    expect(postJson).not.toHaveBeenCalledWith("/api/research/threads/create", expect.anything());
    expect(postJson).not.toHaveBeenCalledWith("/api/research/query", expect.anything());
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

    const knowledgeButton = renderer.root.find((node) =>
      node.type === "button" && node.props["aria-label"] === "资料包同步",
    );
    await act(async () => {
      knowledgeButton.props.onClick();
    });

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
      node.type === "button" && textOf(node).includes("立即更新"),
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
    const deleteButton = renderer.root.findByProps({
      "aria-label": "删除研究会话：保留会话",
    });
    await act(async () => { deleteButton.props.onClick(); });
    expect(textOf(renderer.toJSON())).toContain("确认删除？");
    expect(postJson).not.toHaveBeenCalledWith("/api/research/threads/delete", expect.anything());
    expect(textOf(renderer.toJSON())).toContain("保留会话");
    await act(async () => { renderer.unmount(); });
  });
});
