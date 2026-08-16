import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FilterCriteria } from "../FilterBar";
import type { AdaptiveScreenRequest, BacktestResult, WatchlistItem } from "../../types";

const { postJsonMock } = vi.hoisted(() => ({ postJsonMock: vi.fn() }));

vi.mock("../../lib/tauri", () => ({ postJson: postJsonMock }));

import { BacktestPanel } from "./BacktestPanel";

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

const watchlist: WatchlistItem[] = [{ code: "002432.SZ", name: "九安医疗" }];
const result: BacktestResult = {
  metrics: { total_return: 0.12, num_stocks: 1 },
  equity_curve: [{ date: "2026-07-17", equity: 1.12 }],
  symbols: ["002432.SZ"],
};
const adaptiveScreenSpec: AdaptiveScreenRequest = {
  criteria: {
    include_st: false,
    limit: 80,
    sort_by: "score",
    sort_dir: "desc",
    score_profile: "balanced",
  },
  mode: "auto",
  horizon: "swing_10_30d",
  primary_limit: 10,
  exploration_limit: 10,
  run_id: "run-backtest",
};

function textContent(renderer: ReactTestRenderer): string {
  return renderer.root.findAll((node) => typeof node.children[0] === "string")
    .flatMap((node) => node.children)
    .filter((child): child is string => typeof child === "string")
    .join(" ");
}

function runButton(renderer: ReactTestRenderer) {
  return renderer.root.find((node) => node.props["aria-label"] === "运行回测");
}

describe("BacktestPanel interactions", () => {
  beforeEach(() => {
    vi.stubGlobal("window", {
      location: { href: "http://localhost/" },
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        callback(0);
        return 1;
      },
    });
  });

  afterEach(() => {
    postJsonMock.mockReset();
    vi.unstubAllGlobals();
  });

  it("shows industry and market scope for criteria backtests", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BacktestPanel
          criteria={{ ...criteria, industry: "影视院线", marketScope: "北交所" }}
          watchlist={watchlist}
        />,
      );
    });

    expect(textContent(renderer)).toContain("影视院线 / 北交所");
  });

  it("shows the effective adaptive scope instead of persisted custom filters", async () => {
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BacktestPanel
          criteria={{ ...criteria, industry: "影视院线", marketScope: "北交所" }}
          watchlist={watchlist}
          preferredSource={{ source: "criteria", requestId: 1, adaptiveScreenSpec }}
        />,
      );
    });

    expect(textContent(renderer)).toContain("全部行业 / 全部范围");
    expect(textContent(renderer)).not.toContain("影视院线 / 北交所");
  });

  it("uses the originating tab criteria snapshot instead of global custom filters", async () => {
    postJsonMock.mockResolvedValue(result);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BacktestPanel
          criteria={{ ...criteria, industry: "影视院线", marketScope: "北交所" }}
          watchlist={watchlist}
          preferredSource={{
            source: "criteria",
            requestId: 1,
            criteriaSnapshot: { ...criteria, scoreProfile: "balanced" },
          }}
        />,
      );
    });

    expect(textContent(renderer)).toContain("全部行业 / 全部范围");
    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    const payload = postJsonMock.mock.calls[0][1];
    expect(payload.criteria).not.toHaveProperty("industry");
    expect(payload.criteria).not.toHaveProperty("market_scope");
  });

  it("clears the adaptive specification when switching to watchlist", async () => {
    postJsonMock.mockResolvedValue(result);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BacktestPanel
          criteria={criteria}
          watchlist={watchlist}
          preferredSource={{ source: "criteria", requestId: 1, adaptiveScreenSpec }}
        />,
      );
    });
    const watchlistButton = renderer.root.find(
      (node) => node.type === "button" && node.children.includes("自选股"),
    );

    await act(async () => {
      watchlistButton.props.onClick();
    });
    await act(async () => {
      await runButton(renderer).props.onClick();
    });

    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/backtest",
      expect.objectContaining({
        source: "watchlist",
        strategy_mode: "candidate_snapshot",
        stock_codes: ["002432.SZ"],
      }),
      { timeoutMs: 90_000 },
    );
    expect(postJsonMock.mock.calls[0][1]).not.toHaveProperty("adaptive_screen_spec");
  });

  it("enters loading immediately and deduplicates repeated clicks", async () => {
    let resolveRequest: (value: BacktestResult) => void = () => undefined;
    postJsonMock.mockReturnValue(new Promise<BacktestResult>((resolve) => {
      resolveRequest = resolve;
    }));
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<BacktestPanel criteria={criteria} watchlist={watchlist} />);
    });

    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });

    expect(postJsonMock).toHaveBeenCalledTimes(1);
    expect(postJsonMock).toHaveBeenCalledWith(
      "/api/backtest",
      expect.objectContaining({ strategy_mode: "candidate_snapshot" }),
      { timeoutMs: 90_000 },
    );
    expect(runButton(renderer).props["aria-disabled"]).toBe(true);
    expect(renderer.root.findAllByType("button").every((button) => button.props.disabled)).toBe(true);
    expect(renderer.root.findAll((node) => typeof node.props.id === "string" && node.props.id.startsWith("bt"))
      .every((control) => control.props.disabled)).toBe(true);
    expect(textContent(renderer)).toContain("回测计算中");

    await act(async () => {
      resolveRequest(result);
      await Promise.resolve();
    });

    expect(runButton(renderer).props["aria-disabled"]).toBe(false);
    expect(renderer.root.findAll((node) => typeof node.props.id === "string" && node.props.id.startsWith("bt"))
      .every((control) => !control.props.disabled)).toBe(true);
    expect(textContent(renderer)).toContain("002432.SZ");
  });

  it("clears a previous result before reporting an empty watchlist", async () => {
    postJsonMock.mockResolvedValueOnce(result);
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(
        <BacktestPanel
          criteria={criteria}
          watchlist={watchlist}
          preferredSource={{ source: "watchlist", requestId: 1 }}
        />,
      );
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });
    expect(textContent(renderer)).toContain("002432.SZ");

    await act(async () => {
      renderer.update(<BacktestPanel criteria={criteria} watchlist={[]} />);
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });

    expect(textContent(renderer)).toContain("自选股为空");
    expect(textContent(renderer)).not.toContain("002432.SZ");
  });

  it("shows request failures and restores the run button", async () => {
    postJsonMock.mockRejectedValueOnce(new Error("network down"));
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<BacktestPanel criteria={criteria} watchlist={watchlist} />);
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });

    expect(textContent(renderer)).toContain("network down");
    expect(runButton(renderer).props["aria-disabled"]).toBe(false);
  });

  it("discards an in-flight result when an external route changes the source", async () => {
    let resolveRequest: (value: BacktestResult) => void = () => undefined;
    postJsonMock.mockReturnValue(new Promise<BacktestResult>((resolve) => {
      resolveRequest = resolve;
    }));
    const onPreferredSourceConsumed = vi.fn();
    let renderer!: ReactTestRenderer;
    await act(async () => {
      renderer = create(<BacktestPanel criteria={criteria} watchlist={watchlist} />);
    });
    await act(async () => {
      runButton(renderer).props.onClick();
      await Promise.resolve();
    });
    await act(async () => {
      renderer.update(
        <BacktestPanel
          criteria={criteria}
          watchlist={watchlist}
          preferredSource={{ source: "watchlist", requestId: 2 }}
          onPreferredSourceConsumed={onPreferredSourceConsumed}
        />,
      );
    });
    await act(async () => {
      resolveRequest(result);
      await Promise.resolve();
    });

    expect(onPreferredSourceConsumed).toHaveBeenCalledWith(2);
    expect(textContent(renderer)).not.toContain("002432.SZ");
    expect(textContent(renderer)).toContain("选择股票来源并设置参数后运行回测");
  });
});
