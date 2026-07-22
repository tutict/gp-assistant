import { renderToStaticMarkup } from "react-dom/server";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ResearchMessage } from "../../types";

const tauriMocks = vi.hoisted(() => ({
  getJson: vi.fn(),
  postJson: vi.fn(),
}));
vi.mock("../../lib/tauri", () => ({
  getJson: tauriMocks.getJson,
  postJson: tauriMocks.postJson,
  isMobileTauriRuntime: () => false,
}));
const { getJson, postJson } = tauriMocks;

import { NewsRagPanel, NewsRagView } from "./NewsRagPanel";

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

function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(textOf).join("");
  if (value && typeof value === "object" && "children" in value) {
    return textOf((value as { children?: unknown }).children);
  }
  return "";
}

describe("NewsRagPanel", () => {
  beforeEach(() => {
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
});
