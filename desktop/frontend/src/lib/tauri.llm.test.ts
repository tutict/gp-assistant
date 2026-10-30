import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { postJson } from "./tauri";
import { normalizeLlmDiagnostic } from "./llmSettingsDraft";

beforeEach(() => { vi.stubGlobal("window", { location: { href: "http://127.0.0.1/" },
  setTimeout: globalThis.setTimeout, clearTimeout: globalThis.clearTimeout }); });
afterEach(vi.unstubAllGlobals);
describe("LLM diagnostic transport", () => {
  it("preserves HTTP status with a structured error response", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ code: "auth", message: "密钥未通过" }), { status: 401 })));
    const error = await postJson("/api/llm/test", { model: "fixture" }, { timeoutMs: 1000 }).catch(value => value);
    expect(error).toBeInstanceOf(Error);
    expect(error).toHaveProperty("status", 401);
    expect(normalizeLlmDiagnostic(error)).toMatchObject({ code: "auth", message: "密钥未通过", status: 401 });
  });
});
