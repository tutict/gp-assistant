import { describe, expect, it } from "vitest";
import {
  formatPerfRecord,
  perfOutcomeForError,
  shouldEnablePerfDiagnostics,
  type PerfRecord,
} from "./perfDiagnostics";

describe("local mobile performance diagnostics", () => {
  it("enables logging only for mobile development or an explicitly tagged test build", () => {
    expect(shouldEnablePerfDiagnostics({ mobileRuntime: false, dev: true })).toBe(false);
    expect(shouldEnablePerfDiagnostics({ mobileRuntime: true, dev: true })).toBe(true);
    expect(shouldEnablePerfDiagnostics({ mobileRuntime: true, dev: false, testBuildFlag: "1" })).toBe(true);
    expect(shouldEnablePerfDiagnostics({ mobileRuntime: true, dev: false, testBuildFlag: "0" })).toBe(false);
  });

  it("serializes only stage metrics and excludes stock, financial, and credential values", () => {
    const record = {
      flow: "observe",
      stage: "600000.SH",
      elapsedMs: 42.6,
      cacheStrategy: "refresh",
      stockCount: 1,
      candidateCount: 80,
      historyRows: 750,
      financialRecords: 8,
      requestBytes: 1200,
      responseBytes: 6400,
      outcome: "ok",
      close: 12.34,
      token: "secret-token",
    } as unknown as PerfRecord;

    const line = formatPerfRecord(record);

    expect(line.startsWith("[gp-perf] ")).toBe(true);
    expect(JSON.parse(line.slice("[gp-perf] ".length))).toEqual({
      flow: "observe",
      stage: "unknown",
      elapsed_ms: 43,
      cache_strategy: "refresh",
      stock_count: 1,
      candidate_count: 80,
      history_rows: 750,
      financial_records: 8,
      request_bytes: 1200,
      response_bytes: 6400,
      outcome: "ok",
    });
    expect(line).not.toContain("600000");
    expect(line).not.toContain("12.34");
    expect(line).not.toContain("secret-token");
  });

  it("labels timeout failures without including their messages", () => {
    expect(perfOutcomeForError(new Error("移动端观察超过 35 秒未返回"))).toBe("timeout");
    expect(perfOutcomeForError(new Error("offline"))).toBe("error");
  });
});
