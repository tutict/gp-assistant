import { describe, expect, it } from "vitest";
import type { ResearchMessage, ResearchOverview, ResearchCitation } from "../types";
import { applyMarkRead, pushCitation } from "./newsInteractions";

const citation = (id: string): ResearchCitation => ({
  citation_id: id,
  document_id: `doc-${id}`,
  chunk_id: `chunk-${id}`,
  title: `证据 ${id}`,
  excerpt: "摘要",
  source_tier: "filing",
  source_name: "公告",
  lexical_score: 1,
  retrieval_score: 1,
});

describe("news interaction helpers", () => {
  it("marks a batch read and decrements overview counts only for unread ids", () => {
    const messages: ResearchMessage[] = [
      { id: "a", document_id: "d", stock_code: "600000.SH", title: "a", summary: "", sentiment: "positive", source_tier: "filing", source_name: "公告", unread: true },
      { id: "b", document_id: "d", stock_code: "600000.SH", title: "b", summary: "", sentiment: "positive", source_tier: "filing", source_name: "公告", unread: false },
      { id: "c", document_id: "d", stock_code: "000001.SZ", title: "c", summary: "", sentiment: "negative", source_tier: "news", source_name: "媒体", unread: true },
    ];
    const overview: ResearchOverview = {
      schema_version: 2, document_count: 1, chunk_count: 1, unread_count: 2,
      unread_by_stock: { "600000.SH": 1, "000001.SZ": 1 }, messages,
    };
    const result = applyMarkRead(messages, overview, ["a", "b", "missing"]);
    expect(result.messages.find((item) => item.id === "a")?.unread).toBe(false);
    expect(result.messages.find((item) => item.id === "c")?.unread).toBe(true);
    expect(result.overview?.unread_count).toBe(1);
    expect(result.overview?.unread_by_stock).toEqual({ "600000.SH": 0, "000001.SZ": 1 });
  });

  it("pushes citations after the current pointer and truncates forward history", () => {
    const a = citation("C1");
    const b = citation("C2");
    const c = citation("C3");
    expect(pushCitation([a, b], 0, c)).toEqual({ stack: [a, c], pointer: 1 });
    expect(pushCitation([a, b], 1, b)).toEqual({ stack: [a, b], pointer: 1 });
  });
});
