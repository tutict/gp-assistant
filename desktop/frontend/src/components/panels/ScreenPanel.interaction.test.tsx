import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FilterCriteria } from "../FilterBar";
import type { AdaptiveScreenRequest } from "../../types";

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
  marketScope: "",
  resultLimit: 10,
  sortBy: "score",
  sortDir: "desc",
  scoreProfile: "quality",
};

const restrictiveCriteria: FilterCriteria = {
  ...criteria,
  requireInstitutionBuyRatio: true,
  minRoe: "15",
  maxPe: "30",
  industry: "传媒",
  marketScope: "北交所",
  sortBy: "roe",
  sortDir: "asc",
};

function runButton(renderer: ReactTestRenderer) {
  return renderer.root.find(
    (node) => node.type === "button" && node.props.className === "run-btn",
  );
}

function textContent(renderer: ReactTestRenderer): string {
  return renderer.root
    .findAll((node) => typeof node.children[0] === "string")
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string")
    .join(" ");
}

function expectNoCustomFilters(criteriaPayload: Record<string, unknown>) {
  for (const field of [
    "industry",
    "market_scope",
    "min_roe",
    "max_pe",
    "min_deducted_net_profit_billion",
    "min_deducted_net_profit_growth_rate",
  ]) {
    expect(criteriaPayload).not.toHaveProperty(field);
  }
  expect(criteriaPayload.include_st).toBe(false);
  expect(criteriaPayload.require_institution_buy_ratio_gt_sell_ratio).toBe(false);
}

async function renderPanel(
  onCriteriaChange: (criteria: FilterCriteria) => void = () => undefined,
  panelCriteria: FilterCriteria = criteria,
  onRunBacktest?: (screenSpec?: AdaptiveScreenRequest, criteriaSnapshot?: FilterCriteria) => void,
) {
  let renderer!: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <ScreenPanel
        criteria={panelCriteria}
        onCriteriaChange={onCriteriaChange}
        watchlist={[]}
        onWatchlistChange={() => undefined}
        onRunBacktest={onRunBacktest}
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

  it("runs adaptive screening with isolated full-universe criteria", async () => {
    postJsonMock.mockResolvedValue({ total: 0, returned: 0, items: [], groups: [] });
    const renderer = await renderPanel(undefined, restrictiveCriteria);

    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(postJsonMock).toHaveBeenCalledWith("/api/screen", expect.any(Object));
    expectNoCustomFilters(postJsonMock.mock.calls[0][1].criteria);
  });

  it("does not render fixed adaptive mode and horizon controls", async () => {
    const renderer = await renderPanel();

    expect(renderer.root.findAll((node) => node.props.id === "adaptiveMode")).toHaveLength(0);
    expect(renderer.root.findAll((node) => node.props.className === "adaptive-horizon")).toHaveLength(0);
    expect(renderer.root.findAll((node) => node.children.includes("高级过滤"))).toHaveLength(0);
    expect(renderer.root.findAll((node) => node.props.id === "adaptiveScreen-industry")).toHaveLength(0);
  });

  it("uses segmented sorting and chip toggles only for custom-screen criteria interactions", async () => {
    const onCriteriaChange = vi.fn();
    const renderer = await renderPanel(onCriteriaChange);
    const customTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("自定义选股"),
    );

    await act(async () => {
      customTab.props.onClick();
    });

    const sortGroup = renderer.root.find(
      (node) => node.props.role === "group" && node.props["aria-labelledby"] === "customScreen-sortDir-label",
    );
    const ascending = sortGroup.findAllByType("button").find((node) => node.children.includes("升序"));
    const includeSt = renderer.root.findAll(
      (node) => node.type === "input" && node.props.type === "checkbox",
    )[0];

    expect(renderer.root.findAll((node) => node.props.id === "customScreen-sortDir")).toHaveLength(0);
    expect(ascending?.props["aria-pressed"]).toBe(false);

    await act(async () => {
      ascending?.props.onClick();
    });
    expect(onCriteriaChange).toHaveBeenCalledWith(expect.objectContaining({ sortDir: "asc" }));

    await act(async () => {
      includeSt.props.onChange({ target: { checked: true } });
    });
    expect(onCriteriaChange).toHaveBeenCalledWith(expect.objectContaining({ includeSt: true }));
  });

  it("runs custom screening with independent industry and market-scope filters", async () => {
    postJsonMock.mockResolvedValue({ total: 0, returned: 0, items: [], groups: [] });
    const renderer = await renderPanel(undefined, {
      ...criteria,
      industry: "影视院线",
      marketScope: "北交所",
    });
    const customTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("自定义选股"),
    );

    await act(async () => {
      customTab.props.onClick();
    });
    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/custom-screen",
      expect.objectContaining({
        criteria: expect.objectContaining({
          industry: "影视院线",
          market_scope: "北交所",
          min_deducted_net_profit_billion: 0,
          min_deducted_net_profit_growth_rate: 10,
        }),
      }),
    );
  });

  it("does not render a completed request after switching to another tab", async () => {
    let resolveRequest: (value: unknown) => void = () => undefined;
    postJsonMock.mockReturnValue(new Promise((resolve) => {
      resolveRequest = resolve;
    }));
    const renderer = await renderPanel();
    const customTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("自定义选股"),
    );
    const adaptiveTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("智能选股"),
    );

    await act(async () => {
      customTab.props.onClick();
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });
    await act(async () => {
      adaptiveTab.props.onClick();
    });
    await act(async () => {
      resolveRequest({ total: 0, returned: 0, items: [], groups: [] });
      await Promise.resolve();
    });

    expect(textContent(renderer)).not.toContain("暂无匹配股票");
    expect(textContent(renderer)).toContain("点击运行查看当前模式的全市场筛选结果");
  });

  it("requests ten stocks per concept group while retaining the five-stock group threshold", async () => {
    postJsonMock.mockResolvedValue({ total: 0, returned: 0, groups: [] });
    const renderer = await renderPanel(undefined, restrictiveCriteria);
    const conceptTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("概念分组"),
    );

    await act(async () => {
      conceptTab.props.onClick();
    });
    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/sector-screen",
      expect.objectContaining({
        group_by: "concept",
        per_sector_limit: 10,
        min_sector_candidates: 5,
      }),
    );
    expectNoCustomFilters(postJsonMock.mock.calls[0][1].criteria);
  });

  it.each([
    ["板块分组", "/api/sector-screen"],
    ["趋势选股", "/api/trend-screen"],
  ])("keeps %s independent from custom-screen filters", async (tabLabel, endpoint) => {
    postJsonMock.mockResolvedValue({ total: 0, returned: 0, items: [], groups: [] });
    const renderer = await renderPanel(undefined, restrictiveCriteria);
    const tab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes(tabLabel),
    );

    await act(async () => {
      tab.props.onClick();
    });
    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(postJsonMock).toHaveBeenCalledWith(endpoint, expect.any(Object));
    expectNoCustomFilters(postJsonMock.mock.calls[0][1].criteria);
  });

  it("passes the originating non-custom tab criteria into backtest navigation", async () => {
    postJsonMock.mockResolvedValue({
      total: 1,
      returned: 1,
      items: [{
        stock: { code: "000001.SZ", name: "行业样本", industry: "银行Ⅱ", price: 10 },
        score: 12,
        reasons: [],
      }],
      groups: [],
    });
    const onRunBacktest = vi.fn();
    const renderer = await renderPanel(undefined, restrictiveCriteria, onRunBacktest);
    const trendTab = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("趋势选股"),
    );

    await act(async () => {
      trendTab.props.onClick();
    });
    await act(async () => {
      await runButton(renderer).props.onClick();
    });
    const backtestButton = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("回测"),
    );
    await act(async () => {
      backtestButton.props.onClick();
    });

    expect(onRunBacktest).toHaveBeenCalledWith(undefined, expect.objectContaining({
      industry: "",
      marketScope: "",
      minRoe: "",
    }));
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
