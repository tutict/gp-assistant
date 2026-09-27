import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  getJson: vi.fn(),
  postJson: vi.fn(),
  getTauriListen: vi.fn(),
  isTauriRuntime: vi.fn(),
}));

vi.mock("./tauri", () => tauri);

import { gepaLabAvailable, startGepaRun } from "./gepaLab";

describe("GEPA frontend adapter", () => {
  beforeEach(() => {
    tauri.getJson.mockReset();
    tauri.postJson.mockReset();
    tauri.getTauriListen.mockReset();
    tauri.isTauriRuntime.mockReset();
  });

  it("is unavailable outside a Tauri desktop runtime", () => {
    tauri.isTauriRuntime.mockReturnValue(false);
    expect(gepaLabAvailable()).toBe(false);
  });

  it("is available on Tauri mobile runtimes", () => {
    tauri.isTauriRuntime.mockReturnValue(true);
    expect(gepaLabAvailable()).toBe(true);
  });

  it("starts a profile-scoped run through the Tauri route", async () => {
    tauri.isTauriRuntime.mockReturnValue(true);
    tauri.postJson.mockResolvedValue({ run_id: "run-1", status: "running" });
    const llm = { model: "mock-model", base_url: "http://127.0.0.1:11434/v1" };
    await startGepaRun(llm, "value_compounder_v1", 60);
    expect(tauri.postJson).toHaveBeenCalledWith("/api/agent/gepa/start", {
      llm,
      profile_id: "value_compounder_v1",
      max_metric_calls: 60,
    });
  });
});
