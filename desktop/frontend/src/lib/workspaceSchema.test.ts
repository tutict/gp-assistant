import { describe, expect, it } from "vitest";
import { workspaceValue } from "./workspaceSchema";

describe("workspace schema market selection", () => {
  it("persists the selected market accepted by the settings control", () => {
    expect(workspaceValue("app.market", "cn")).toBe("cn");
    expect(workspaceValue("app.market", "hk")).toBe("hk");
    expect(() => workspaceValue("app.market", "other")).toThrow("Invalid workspace market");
  });
});
