import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, describe, expect, it, vi } from "vitest";
import { StockResearchPanel } from "./StockResearchPanel";

const api = vi.hoisted(() => ({ getJson: vi.fn() }));
vi.mock("../../lib/tauri", () => api);
let renderer: ReactTestRenderer | undefined;
afterEach(async () => { if (renderer) await act(async () => renderer?.unmount()); renderer = undefined; vi.clearAllMocks(); });

describe("StockResearchPanel", () => {
  it("only reads local status and clearly shows missing PIT evidence", async () => {
    api.getJson.mockResolvedValue({ available: false, datasets: [], runs: [], active_parameter_set: { algorithm_version: "adaptive_swing_v1", parameter_set_id: "fixed-v1", scoring_revision: "unit-rank-fix/v1" } });
    await act(async () => { renderer = create(<StockResearchPanel />); });
    const text = JSON.stringify(renderer?.toJSON());
    expect(text).toContain("fixed-v1");
    expect(text).toContain("暂无 PIT 数据");
    expect(api.getJson.mock.calls.map((call) => call[0])).toEqual(["/api/stock-research/status"]);
  });

  it("loads a selected report without starting import, calibration or promotion", async () => {
    api.getJson.mockResolvedValueOnce({
      available: true,
      datasets: [{ dataset_id: "demo", name: "样例", start_date: "2026-01-01", end_date: "2026-03-31", real_data: false, source: "fixture" }],
      runs: [{ run_id: "run-demo", dataset_id: "demo", kind: "evaluation", status: "complete" }],
      active_parameter_set: { algorithm_version: "adaptive_swing_v1", parameter_set_id: "fixed-v1" },
    }).mockResolvedValueOnce({ run_id: "run-demo", data_quality: { passed: false, real_data: false, issues: ["历史状态缺失"] }, metrics: { total_return: 0 }, qualification_verified: false });
    await act(async () => { renderer = create(<StockResearchPanel />); });
    const button = renderer!.root.findAllByType("button").find((item) => item.props["aria-label"] === "查看研究报告 run-demo");
    expect(button).toBeDefined();
    await act(async () => { await button!.props.onClick(); });
    const text = JSON.stringify(renderer?.toJSON());
    expect(text).toContain("历史状态缺失");
    expect(text).toContain("合成样例");
    expect(text).toContain("run-demo");
    expect(text).toContain("报告标识");
    expect(text).toContain("本报告参数版本");
    expect(text).not.toContain("验证通过");
    expect(api.getJson.mock.calls.map((call) => call[0])).toEqual(["/api/stock-research/status", "/api/stock-research/report?run_id=run-demo"]);
  });
});
