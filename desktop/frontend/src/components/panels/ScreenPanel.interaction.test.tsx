import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FilterCriteria } from "../FilterBar";

const { postJsonMock, getTauriListenMock } = vi.hoisted(() => ({
  postJsonMock: vi.fn(),
  getTauriListenMock: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  postJson: postJsonMock,
  getTauriListen: getTauriListenMock,
}));

import { ScreenPanel } from "./ScreenPanel";

const criteria: FilterCriteria = {
  includeSt: false,
  requireInstitutionBuyRatio: false,
  minRoe: "",
  maxPe: "",
  maxPb: "",
  minMcap: "",
  industry: "",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
  scoreProfile: "quality",
};

function runButton(renderer: ReactTestRenderer) {
  return renderer.root.find(
    (node) => node.type === "button" && node.props.className === "run-btn",
  );
}

function adaptiveModeSelect(renderer: ReactTestRenderer) {
  return renderer.root.find(
    (node) => node.type === "select" && node.props.id === "adaptiveMode",
  );
}

function textContent(renderer: ReactTestRenderer): string {
  return renderer.root
    .findAll((node) => typeof node.children[0] === "string")
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string")
    .join(" ");
}

async function renderPanel() {
  let renderer!: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <ScreenPanel
        criteria={criteria}
        onCriteriaChange={() => undefined}
        watchlist={[]}
        onWatchlistChange={() => undefined}
      />,
    );
  });
  return renderer;
}

describe("ScreenPanel adaptive states", () => {
  beforeEach(() => {
    vi.stubGlobal("window", { location: { href: "http://localhost/" } });
    getTauriListenMock.mockReturnValue(undefined);
  });

  afterEach(() => {
    postJsonMock.mockReset();
    getTauriListenMock.mockReset();
    vi.unstubAllGlobals();
  });

  it("shows adaptive loading immediately and sends the complete swing request", async () => {
    let resolveRequest: (value: unknown) => void = () => undefined;
    postJsonMock.mockReturnValue(
      new Promise((resolve) => {
        resolveRequest = resolve;
      }),
    );
    const renderer = await renderPanel();

    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });

    expect(runButton(renderer).props.disabled).toBe(true);
    expect(
      renderer.root.findAll(
        (node) =>
          typeof node.props.className === "string" &&
          node.props.className.includes("panel-feedback-loading"),
      ),
    ).toHaveLength(1);
    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/screen",
      expect.objectContaining({
        mode: "auto",
        horizon: "swing_10_30d",
        primary_limit: 10,
        exploration_limit: 10,
        run_id: expect.any(String),
      }),
    );

    await act(async () => {
      resolveRequest({ total: 0, returned: 0, items: [], groups: [] });
      await Promise.resolve();
    });
    expect(runButton(renderer).props.disabled).toBe(false);
  });

  it("enables manual regime modes only after the backend confirms adaptive rollout", async () => {
    postJsonMock
      .mockResolvedValueOnce({
        algorithm_version: "legacy_balanced",
        total: 0,
        returned: 0,
        items: [],
        groups: [],
      })
      .mockResolvedValueOnce({
        algorithm_version: "adaptive_swing_v1",
        total: 0,
        returned: 0,
        items: [],
        groups: [],
      });
    const renderer = await renderPanel();

    expect(adaptiveModeSelect(renderer).props.disabled).toBe(true);

    await act(async () => {
      await runButton(renderer).props.onClick();
    });
    expect(adaptiveModeSelect(renderer).props.disabled).toBe(true);

    await act(async () => {
      await runButton(renderer).props.onClick();
    });
    expect(adaptiveModeSelect(renderer).props.disabled).toBe(false);
  });

  it.each([
    "网络请求失败",
    "市场状态/历史数据不足：候选日线覆盖率低于 60%",
  ])("renders request and data-insufficient errors: %s", async (message) => {
    postJsonMock.mockRejectedValueOnce(new Error(message));
    const renderer = await renderPanel();

    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(renderer.root.findAll((node) => node.props.role === "alert")).toHaveLength(1);
    expect(textContent(renderer)).toContain(message);
    expect(runButton(renderer).props.disabled).toBe(false);
  });
});
