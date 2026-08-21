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
    vi.stubGlobal("window", { setInterval: vi.fn(() => 1), clearInterval: vi.fn() });
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

  it("keeps the research-only risk boundary visible beside the question input", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    expect(textOf(renderer.toJSON())).toContain("仅供研究，不构成投资建议");
    await act(async () => { renderer.unmount(); });
  });

  it("keeps the full risk boundary visible on mobile", async () => {
    tauriMocks.mobile = true;
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<NewsRagPanel llmSettings={null} />);
    });

    const riskBoundary = renderer.root.findByProps({ className: "research-risk-boundary" });
    expect(textOf(riskBoundary)).toBe("仅供研究，不构成投资建议。");
    expect(renderer.root.findByType("textarea").props.placeholder).not.toContain("仅供研究");
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
    const confirmMock = vi.fn(() => true);
    vi.stubGlobal("window", { setInterval: vi.fn(() => 1), clearInterval: vi.fn(), confirm: confirmMock });

    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });
    expect(textOf(renderer.toJSON())).toContain("旧回答");
    const deleteButton = renderer.root.findByProps({
      "aria-label": "删除研究会话：待删除会话",
    });
    await act(async () => { deleteButton.props.onClick(); });

    expect(confirmMock).toHaveBeenCalledWith("确认删除研究会话“待删除会话”？历史问答将一并删除。");
    expect(postJson).toHaveBeenCalledWith("/api/research/threads/delete", {
      thread_id: "thread-delete",
    });
    expect(textOf(renderer.toJSON())).not.toContain("旧回答");
    await act(async () => { renderer.unmount(); });
  });

  it("does not delete a research thread when confirmation is cancelled", async () => {
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
    const confirmMock = vi.fn(() => false);
    vi.stubGlobal("window", { setInterval: vi.fn(() => 1), clearInterval: vi.fn(), confirm: confirmMock });
    let renderer!: ReactTestRenderer;
    await act(async () => { renderer = create(<NewsRagPanel llmSettings={null} />); });
    const deleteButton = renderer.root.findByProps({
      "aria-label": "删除研究会话：保留会话",
    });
    await act(async () => { deleteButton.props.onClick(); });
    expect(confirmMock).toHaveBeenCalledTimes(1);
    expect(postJson).not.toHaveBeenCalledWith("/api/research/threads/delete", expect.anything());
    expect(textOf(renderer.toJSON())).toContain("保留会话");
    await act(async () => { renderer.unmount(); });
  });
});
