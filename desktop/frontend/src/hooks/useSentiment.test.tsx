import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useSentiment } from "./useSentiment";
import type { SentimentAnalysis } from "../types/sentiment";
const { postJson } = vi.hoisted(() => ({ postJson: vi.fn() }));
vi.mock("../lib/tauri", () => ({ postJson }));
let state!: ReturnType<typeof useSentiment>;
let renderer: ReactTestRenderer | undefined;
const old = { analysis_id: "old", stock_code: "000001.SZ", snapshot: { snapshot_id: "frozen" } } as SentimentAnalysis;
const llm = { model: "fixture", base_url: "http://localhost" };
function Probe({ code, codes = [] }: { code: string; codes?: string[] }) {
  state = useSentiment(code, codes);
  return null;
}
async function mount(codes?: string[]) {
  await act(async () => { renderer = create(<Probe code="000001.SZ" codes={codes} />); });
}
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  postJson.mockImplementation(async (path: string) => {
    if (path.endsWith("/latest")) return { analysis: old };
    if (path.endsWith("/history")) return { items: [old] };
    if (path.endsWith("/snapshot")) return { snapshot_id: "fresh" };
    if (path.endsWith("/start")) return { run_id: "run" };
    if (path.endsWith("/status")) return { run_id: "run", status: "running" };
    if (path.endsWith("/cancel")) return { cancelled: true };
    if (path.endsWith("/followup")) return { analysis_id: "old", answer: "快照回答", evidence_ids: [], created_at: 1 };
  });
});
afterEach(async () => { await act(async () => renderer?.unmount()); renderer = undefined; vi.clearAllMocks(); vi.unstubAllGlobals(); });

it("loads this stock without sending an industry hint or calling a model", async () => {
  await mount(["600000.SH", "000001.SZ"]);
  expect(state.analysis).toEqual(old);
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/snapshot", { stock_code: "000001.SZ", window_days: 30 });
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/history", { stock_code: "000001.SZ" });
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/history", { stock_codes: ["000001.SZ", "600000.SH"] });
  expect(postJson.mock.calls.some(([, body]) => body && typeof body === "object" && "industry" in body)).toBe(false);
  await act(async () => state.start(undefined));
  expect(state.error).toContain("配置");
  expect(postJson.mock.calls.some(([path]) => path.endsWith("/start"))).toBe(false);
});

it("starts manually and cancellation preserves previous analysis", async () => {
  await mount();
  await act(async () => state.start(llm));
  expect(state.run?.status).toBe("running");
  await act(async () => state.cancel());
  expect(state.run?.status).toBe("cancelled");
  expect(state.analysis).toEqual(old);
});

it("does not let a late status response undo cancellation", async () => {
  let resolveStatus: (value: unknown) => void = () => undefined;
  postJson.mockImplementation((path: string) => {
    if (path.endsWith("/status")) return new Promise((resolve) => { resolveStatus = resolve; });
    if (path.endsWith("/start")) return Promise.resolve({ run_id: "run" });
    if (path.endsWith("/cancel")) return Promise.resolve({ cancelled: true });
    if (path.endsWith("/latest")) return Promise.resolve({ analysis: old });
    if (path.endsWith("/history")) return Promise.resolve({ items: [old] });
    return Promise.resolve({ snapshot_id: "fresh" });
  });
  await mount();
  await act(async () => state.start(llm));
  await act(async () => state.cancel());
  await act(async () => { resolveStatus({ run_id: "run", stock_code: "000001.SZ", status: "running", stage: "仍在跑", progress: 40 }); });
  expect(state.run?.status).toBe("cancelled");
});

it("binds followup to selected immutable analysis", async () => {
  await mount();
  await act(async () => { await state.ask("反证是什么？", llm); });
  expect(state.followups[0].answer).toBe("快照回答");
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/followup", { analysis_id: "old", question: "反证是什么？", llm });
});

it("cancels a start that resolves after switching stock", async () => {
  await mount();
  let resolve!: (value: { run_id: string }) => void;
  postJson.mockImplementation((path: string) => path.endsWith("/start")
    ? new Promise((done) => { resolve = done; })
    : Promise.resolve(path.endsWith("/latest") ? { analysis: null } : path.endsWith("/history") ? { items: [] } : path.endsWith("/cancel") ? { cancelled: true } : { snapshot_id: "next" }));
  let pending!: Promise<void>;
  await act(async () => { pending = state.start(llm); });
  await act(async () => { renderer!.update(<Probe code="600000.SH" />); });
  await act(async () => { resolve({ run_id: "old-run" }); await pending; });
  expect(state.run).toBeNull();
  expect(state.analysis).toBeNull();
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/cancel", { run_id: "old-run" });
});

it("cancels the active run when the panel unmounts", async () => {
  await mount();
  await act(async () => state.start(llm));
  await act(async () => renderer?.unmount());
  renderer = undefined;
  expect(postJson).toHaveBeenCalledWith("/api/sentiment/cancel", { run_id: "run" });
});

it("keeps previous analysis when refreshing data fails", async () => {
  await mount();
  postJson.mockRejectedValue(new Error("offline"));
  await act(async () => state.retry());
  expect(state.analysis).toEqual(old);
  expect(state.error).toContain("offline");
});
