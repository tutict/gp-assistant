import type { NewsRefreshReport } from "../types";

const newsTimeFormat = new Intl.DateTimeFormat("zh-CN", { timeZone: "Asia/Shanghai", year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });

export interface NewsRefreshFeedback {
  status: NewsRefreshReport["status"];
  message: string;
}

export function newsRefreshFeedback(news: { notes?: string[]; refresh?: NewsRefreshReport } | undefined): NewsRefreshFeedback {
  const report = news?.refresh;
  if (report) {
    const failedSources = Array.isArray(report.sources) ? [...new Set(report.sources
      .filter(source => source.failed_requests > 0 && source.source_name)
      .map(source => source.source_name))].join("、") : "";
    const sourceDetail = failedSources ? `未更新来源：${failedSources}。` : "";
    if (report.status === "failed") return { status: "failed", message: `消息来源暂时无法更新，已有消息仍可阅读。${sourceDetail}` };
    if (report.status === "partial") return { status: "partial", message: `部分消息来源未更新，已有消息仍可阅读。${sourceDetail}本次读取 ${report.fetched_count} 条消息，原文摘要补全 ${report.summary_count} 条。` };
    if (report.status === "empty") return { status: "empty", message: "来源已检查，当前时间范围没有可用消息，已有消息仍可阅读。" };
    if (report.status === "success") return { status: "success", message: `消息已更新。本次读取 ${report.fetched_count} 条消息，原文摘要补全 ${report.summary_count} 条。` };
  }
  const summaryNotice = news?.notes?.find(note => note.startsWith("港股原文摘要"));
  const sourceFailed = news?.notes?.some(note => note.startsWith("HK ") && note.includes("news refresh failed;"));
  return sourceFailed
    ? { status: "partial", message: `${summaryNotice || "本次新闻来源未能更新。"} 部分新闻来源暂时不可用，已有消息仍可阅读。` }
    : { status: "success", message: summaryNotice || "消息已更新。" };
}

export function formatNewsTime(value: string | number | null | undefined): string {
  if (value == null || value === "" || (typeof value === "number" && (!Number.isSafeInteger(value) || value <= 0))) return "";
  const raw = typeof value === "string" ? value.trim() : value;
  const local = typeof raw === "string" && /^\d{4}-\d{2}-\d{2}(?:[ T]\d{2}:\d{2}(?::\d{2})?)?$/.test(raw)
    ? `${raw.length === 10 ? `${raw}T00:00:00` : raw.replace(" ", "T")}+08:00` : raw;
  const date = new Date(local);
  if (!Number.isFinite(date.getTime())) return "";
  return newsTimeFormat.format(date);
}
