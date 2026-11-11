import { describe, expect, it } from "vitest";
import { workspaceValue } from "./workspaceSchema";

describe("workspace schema market selection", () => {
  it("persists the selected market accepted by the settings control", () => {
    expect(workspaceValue("app.market", "cn")).toBe("cn");
    expect(workspaceValue("app.market", "hk")).toBe("hk");
    expect(() => workspaceValue("app.market", "other")).toThrow("Invalid workspace market");
  });

  it("validates separate Hong Kong workspace state with the same bounded contracts", () => {
    expect(workspaceValue("hk.news.stock", "00700.HK")).toBe("00700.HK");
    expect(workspaceValue("hk.news.question", "港股研究草稿")).toBe("港股研究草稿");
    expect(workspaceValue("hk.agent.draft:conversation", "查询港股")).toBe("查询港股");
    expect(workspaceValue("hk.agent.conversations", [])).toEqual([]);
    expect(() => workspaceValue("hk.news.question", "x".repeat(32001))).toThrow();
    expect(() => workspaceValue("hk.credentials", "secret")).toThrow();
  });
});
