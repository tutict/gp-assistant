#!/usr/bin/env node

import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const frontendRequire = createRequire(
  new URL("../desktop/frontend/package.json", import.meta.url),
);
const { chromium } = frontendRequire("playwright");

const option = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index < 0 ? fallback : process.argv[index + 1];
};
const checkOnly = process.argv.includes("--check-only");
const serveBuild = process.argv.includes("--serve");
let baseUrl = option("--url", "http://127.0.0.1:4173").replace(/\/$/, "");
const date = new Date().toISOString().slice(0, 10);
const checkSearchOverlay = process.argv.includes('--check-search-overlay');
const checkNewsPage = process.argv.includes("--check-news-page");
const checkNewsInteractions = process.argv.includes("--check-news-interactions");
const checkBacktestPage = process.argv.includes("--check-backtest-page");
const headerSettingsOnly = process.argv.includes("--header-settings-only");
const observeSummaryOnly = process.argv.includes("--observe-summary-only");
const newsPageOnly = process.argv.includes("--news-page-only");
const backtestPageOnly = process.argv.includes("--backtest-page-only");
const stableReport = process.argv.includes("--stable-report");
const defaultOutput = fileURLToPath(
  new URL("../artifacts/ui-shots/" + date + "/", import.meta.url),
);
const outputRoot = resolve(option("--output", defaultOutput));
const buildRoot = resolve(fileURLToPath(new URL("../desktop/mobile-dist/", import.meta.url)));
const devices = [
  { name: "phone-360", width: 360, height: 780, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-430", width: 430, height: 932, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "tablet-768", width: 768, height: 1024, dpr: 2, mobile: true, theme: "dark", density: "comfortable" },
  { name: "desktop-1280", width: 1280, height: 860, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-2560", width: 2560, height: 1440, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1440-compact", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "compact" },
];

async function openCustomScreen(page) {
  const tab = page.getByRole("tab", { name: "自定义选股" }).first();
  if (await tab.count() === 0) return false;
  await tab.click();
  await page.locator(".custom-screen-criteria .criteria-field-grid").first().waitFor({ state: "visible", timeout: 5000 });
  return true;
}

async function openSmartScreen(page) {
  const tab = page.getByRole("tab", { name: "智能选股" }).first();
  if (await tab.count() === 0) return false;
  await tab.click();
  return true;
}

const routes = [
  { name: "screen", hash: "#sectionScreen", ready: ".screen-panel-container" },
  {
    name: "screen-custom",
    hash: "#sectionScreen",
    ready: ".screen-panel-container",
    prepare: async (page) => {
      await openCustomScreen(page);
    },
  },
  { name: "observe", hash: "#sectionObserve", ready: ".observe-panel-container" },
  { name: "backtest", hash: "#sectionBacktest", ready: ".backtest-context" },
  { name: "news", hash: "#sectionNewsRag", ready: ".research-workspace" },
  { name: "agent", hash: "#sectionAgent", ready: ".agent-workspace" },
  {
    name: "settings",
    hash: "#sectionAgent",
    prepare: async (page) => {
      const mobileMenu = page.locator(".agent-mobile-menu");
      if (await mobileMenu.isVisible()) {
        await mobileMenu.click();
        await page.locator(".agent-workspace:not(.rail-collapsed)").waitFor();
      }
      const toggle = page.locator(".llm-settings-toggle");
      await toggle.scrollIntoViewIfNeeded();
      await toggle.click();
      await page.locator(".llm-settings-body").waitFor({ state: "visible" });
    },
  },
];

const mockStocks = [
  { code: "600519.SH", name: "贵州茅台", industry: "食品饮料", price: 1428.6, change_pct: 0.018, pe: 22.4, pb: 7.3, roe: 31.2 },
  { code: "000858.SZ", name: "五粮液", industry: "食品饮料", price: 128.4, change_pct: -0.006, pe: 18.7, pb: 4.6, roe: 24.1 },
  { code: "300750.SZ", name: "宁德时代", industry: "电力设备", price: 286.1, change_pct: 0.012, pe: 19.8, pb: 4.1, roe: 22.7 },
];
const mockBacktestSymbols = [
  ...mockStocks,
  { code: "601318.SH", name: "中国平安" },
  { code: "600036.SH", name: "招商银行" },
  { code: "000333.SZ", name: "美的集团" },
  { code: "002594.SZ", name: "比亚迪" },
  { code: "601012.SH", name: "隆基绿能" },
  { code: "600276.SH", name: "恒瑞医药" },
  { code: "000651.SZ", name: "格力电器" },
];
const mockScreenResult = {
  total: mockStocks.length,
  returned: mockStocks.length,
  items: mockStocks.map((stock, index) => ({
    stock,
    score: 88 - index * 5,
    reasons: ["盈利质量稳定", "趋势强度达标"],
    quality_score: 90 - index * 4,
    trend_score: 84 - index * 3,
    risk_score: 76 - index * 2,
    factor_scores: { quality: 90 - index * 4, trend: 84 - index * 3, risk: 76 - index * 2 },
    score_explanation: "基于质量、趋势与风险因子的桌面回归样例。",
    reason_tags: ["质量", "趋势"],
  })),
  notes: ["截图 harness 注入的确定性榜单数据。"],
};
const mockTrendSeries = Array.from({ length: 90 }, (_, index) => {
  const dateValue = new Date(Date.UTC(2026, 3, 1 + index));
  const close = 118 + index * 0.34 + Math.sin(index / 4) * 3.2;
  return {
    date: dateValue.toISOString().slice(0, 10),
    open: close - 0.8,
    high: close + 1.7,
    low: close - 1.9,
    close,
    volume: 1800000 + index * 21000,
    swl: close - 1.4,
    sws: close + Math.sin(index / 7),
    k: 48 + Math.sin(index / 5) * 22,
    d: 50 + Math.sin(index / 6) * 17,
    j: 44 + Math.sin(index / 4) * 28,
  };
});
const mockObserveResult = {
  source: "ui-harness",
  stock: { ...mockStocks[0], quote_time: "20260814161448" },
  financial_indicators: {
    title: "核心财务指标",
    period: "2026Q2",
    source: "mock",
    items: [
      { metric_key: "roe", label: "ROE", value: 31.2, tone: "positive" },
      { metric_key: "revenue_growth", label: "营收增长", value: 12.6, tone: "positive" },
      { metric_key: "net_profit_growth", label: "净利增长", value: 15.4, tone: "positive" },
    ],
  },
  trend: {
    stock: { ...mockStocks[0], quote_time: "20260814161448" },
    signal: {
      code: "600519.SH",
      date: "2026-07-29",
      close: 148.2,
      previous_close: 146.8,
      close_change_pct: 0.0095,
      support: 142.4,
      resistance: 154.8,
      swl: 147.6,
      sws: 143.2,
      swl_above_sws: true,
      quant_score: 78,
      quant_score_max: 100,
      pattern_score: 8,
      pattern_score_max: 10,
      status: "hold",
      signal_type: "trend_continuation",
      risk_flags: ["low_volume"],
      pattern_signals: ["bottom_accumulation", "dragon_trend_volume"],
      reasons: ["signal_type:trend_continuation", "ma_bull_stack"],
    },
    series: mockTrendSeries,
  },
  capital_evidence: {
    stock_code: "600519.SH",
    composite_score: 74,
    confidence: "high",
    as_of_trade_date: "2026-07-29",
    freshness: "fresh",
    summary: "资金证据与基本面方向一致。",
    sections: [
      { key: "northbound", title: "北向资金", score: 78, available: true, summary: "近五日保持净流入。" },
      { key: "institution", title: "机构席位", score: 69, available: true, summary: "公开席位交易活跃。" },
    ],
  },
  notes: ["截图 harness 注入的确定性观察数据。"],
};
const mockObserveResultCompleteCapital = {
  ...mockObserveResult,
  capital_evidence: {
    ...mockObserveResult.capital_evidence,
    items: [
      {
        category: "fund_flow",
        source: "东方财富个股资金流",
        title: "最新交易日主力资金",
        date: "2026-07-29",
        confidence: "高",
        metrics: {
          主力净流入额: "1.28 亿",
          主力净流入额原值: 128000000,
          主力净占比: "6.4%",
          主力介入度: "中（6.4%）",
        },
      },
      {
        category: "institution_lhb",
        source: "公开龙虎榜",
        title: "龙虎榜机构席位",
        date: "2026-07-29",
        confidence: "高",
        sentiment: "positive",
        metrics: {
          机构买入额: "8200 万",
          机构卖出额: "3100 万",
          机构净买额: "5100 万",
          净买额占成交额比: "2.1%",
          机构买卖比: "2.6x",
          买方机构数: "3",
          卖方机构数: "1",
        },
        seat_detail_status: "complete",
        seats: [
          {
            seat_code: "institution-demo-1",
            name: "机构专用",
            buy_amount: 82000000,
            sell_amount: 31000000,
            net_amount: 51000000,
            direction: "both",
            change_rate: 2.1,
            reason: "换手率承接",
            three_day_activity_count: 3,
            three_day_rise_probability: 0.66,
          },
        ],
      },
      {
        category: "fund_flow",
        source: "Tauri/Rust 本地量价资金代理",
        title: "量价资金代理",
        confidence: "中",
        score: 66,
        metrics: {
          证据类型: "本地日线量价代理",
          推断方向: "偏流入",
          量价热度: "68",
          吸筹强度: "61",
          趋势热度: "72",
          异动热度: "46",
        },
      },
    ],
  },
};
const mockObserveResultUnavailableCapital = {
  ...mockObserveResult,
  capital_evidence: {
    stock_code: "600519.SH",
    composite_score: 52,
    confidence: "medium",
    as_of_trade_date: "2026-07-29",
    freshness: "partial",
    summary: "真实主力资金接口暂不可用，保留本地量价代理参考。",
    sections: [],
    items: [
      { category: "fund_flow_status", source: "东方财富个股资金流", title: "接口暂不可用", date: "2026-07-29", metrics: { 状态: "暂不可用" } },
      {
        category: "fund_flow",
        source: "Tauri/Rust 本地量价资金代理",
        title: "量价资金代理",
        confidence: "中",
        score: 54,
        metrics: {
          证据类型: "本地日线量价代理",
          推断方向: "中性",
          量价热度: "54",
          吸筹强度: "49",
          趋势热度: "57",
          异动热度: "42",
        },
      },
    ],
  },
};
const curve = Array.from({ length: 18 }, (_, index) => ({
  date: new Date(Date.UTC(2025, index, 1)).toISOString().slice(0, 10),
  equity: 1 + index * 0.018 + Math.sin(index / 2) * 0.012,
}));
const benchmarkCurve = curve.map((point, index) => ({
  date: point.date,
  equity: 1 + index * 0.011 + Math.sin(index / 3) * 0.008,
}));
const mockBacktestResult = {
  metrics: {
    total_return: 0.326,
    annualized_return: 0.184,
    max_drawdown: -0.087,
    num_stocks: 10,
    benchmark_total_return: 0.197,
    benchmark_annualized_return: 0.116,
    benchmark_max_drawdown: -0.104,
    excess_return: 0.129,
    total_transaction_cost: 0.006,
    total_turnover: 2.8,
    rebalance_count: 17,
    precision_at_n: 0.64,
  },
  equity_curve: curve,
  benchmark_curve: benchmarkCurve,
  symbols: mockBacktestSymbols.map((stock) => stock.code),
  benchmark_symbols: mockStocks.map((stock) => stock.code),
  strategy_mode: "candidate_snapshot",
  adaptive_release_gate: {
    passed: false,
    checks: [
      {
        key: "annualized_return_delta",
        passed: true,
        actual: 0.068,
        requirement: "年化收益不低于旧策略超过 1 个百分点",
      },
      {
        key: "cached_run_millis",
        passed: false,
        actual: null,
        requirement: "同日缓存运行不超过 2000ms",
      },
    ],
  },
  volatility_snapshots: [
    {
      symbol: "600519.SH",
      name: "贵州茅台",
      date: "2026-08-14",
      close: 1428.6,
      atr: { period: 14, value: 38.2, percent_of_close: 2.6746 },
      bollinger_bands: { period: 20, multiplier: 2, upper: 1518.4, middle: 1432.6, lower: 1346.8, bandwidth_percent: 11.9782, percent_b: 47.6689 },
      donchian_channel: { period: 20, upper: 1532.2, middle: 1435.8, lower: 1339.4, width_percent: 13.4276, position_percent: 46.2656 },
      keltner_channel: { ema_period: 20, atr_period: 10, multiplier: 2, upper: 1506.8, middle: 1430.2, lower: 1353.6, width_percent: 10.7118, position_percent: 48.9569 },
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: 25.06 },
      rvi: { period: 14, value: 58.4 },
    },
    {
      symbol: "000858.SZ",
      name: "五粮液",
      date: "2026-08-14",
      close: 128.4,
      atr: { period: 14, value: 3.1, percent_of_close: 2.4143 },
      bollinger_bands: { period: 20, multiplier: 2, upper: 136.8, middle: 129.2, lower: 121.6, bandwidth_percent: 11.7647, percent_b: 44.7368 },
      donchian_channel: { period: 20, upper: 138.1, middle: 129.4, lower: 120.7, width_percent: 13.4467, position_percent: 44.2529 },
      keltner_channel: { ema_period: 20, atr_period: 10, multiplier: 2, upper: 135.5, middle: 129.1, lower: 122.7, width_percent: 9.9148, position_percent: 44.5313 },
      chaikin_volatility: { ema_period: 10, roc_period: 10, value: -25.06 },
      rvi: { period: 14, value: 46.8 },
    },
  ],
  notes: ["截图 harness 注入的确定性回测数据。"],
};
const mockDataStatus = {
  universe_count: 5231,
  cache_bytes: 67108864,
  quote_trade_date: "20260804",
  current_trade_date: "20260804",
  stale: false,
};

const mockResearchMessagesEmpty = [];
const fixedResearchNow = Date.parse("2026-08-16T23:59:00+08:00");
const mockResearchMessagesData = [
  {
    id: "news-msg-1",
    document_id: "news-doc-1",
    stock_code: "600519.SH",
    title: "三季报预告上修，盈利弹性继续释放",
    summary: "管理层披露的经营节奏继续改善，毛利率与费用率同步优化。",
    sentiment: "positive",
    source_tier: "filing",
    published_at: "2026-08-16T09:10:00Z",
    unread: true,
  },
  {
    id: "news-msg-2",
    document_id: "news-doc-2",
    stock_code: "600519.SH",
    title: "渠道库存回到更健康区间",
    summary: "财务快照显示库存周转恢复正常，终端动销更平滑。",
    sentiment: "bullish",
    source_tier: "financial_snapshot",
    published_at: "2026-08-16T10:05:00Z",
    unread: false,
  },
  {
    id: "news-msg-3",
    document_id: "news-doc-3",
    stock_code: "600519.SH",
    title: "机构研报提示估值压力仍在",
    summary: "部分研报认为短期估值修复已较充分，后续要看旺季兑现。",
    sentiment: "negative",
    source_tier: "research_report",
    published_at: "2026-08-16T11:20:00Z",
    unread: true,
  },
  {
    id: "news-msg-4",
    document_id: "news-doc-4",
    stock_code: "600519.SH",
    title: "行业价格带仍有扰动",
    summary: "行业新闻显示竞品促销动作增加，价格节奏需要继续跟踪。",
    sentiment: "bearish",
    source_tier: "news",
    published_at: "2026-08-16T14:00:00Z",
    unread: false,
  },
  {
    id: "news-msg-5",
    document_id: "news-doc-5",
    stock_code: "600519.SH",
    title: "社区仍在讨论渠道变化",
    summary: "社区消息围绕渠道、动销与补库存节奏展开，结论仍待核查。",
    sentiment: "neutral",
    source_tier: "community",
    published_at: "2026-08-16T15:25:00Z",
    unread: true,
  },
];
const mockResearchCitationOne = {
  citation_id: "C1",
  document_id: "news-doc-1",
  chunk_id: "news-doc-1-chunk-1",
  title: "三季报预告上修",
  excerpt: "公司披露的利润预告上修，反映经营弹性持续释放。",
  source_tier: "filing",
  source_name: "2026 年三季报预告",
  published_at: "2026-08-16T09:10:00Z",
  url: "https://example.com/research/news-doc-1",
  page_number: 12,
  lexical_score: 0.8632,
  vector_score: 0.7521,
  retrieval_score: 0.9014,
};
const mockResearchCitationTwo = {
  citation_id: "C2",
  document_id: "news-doc-2",
  chunk_id: "news-doc-2-chunk-1",
  title: "渠道库存回到健康区间",
  excerpt: "财务快照显示库存周转恢复更平滑，经营质量有所改善。",
  source_tier: "financial_snapshot",
  source_name: "财务快照摘要",
  published_at: "2026-08-16T10:05:00Z",
  url: "https://example.com/research/news-doc-2",
  page_number: 4,
  lexical_score: 0.7114,
  vector_score: 0.6318,
  retrieval_score: 0.8427,
};
const mockResearchCitationThree = {
  citation_id: "C3",
  document_id: "news-doc-5",
  chunk_id: "news-doc-5-chunk-1",
  title: "社区讨论渠道变化",
  excerpt: "社区信息提示市场仍在消化渠道节奏变化，结论需要继续核查。",
  source_tier: "community",
  source_name: "社区讨论摘录",
  published_at: "2026-08-16T15:25:00Z",
  url: "https://example.com/research/news-doc-5",
  page_number: 1,
  lexical_score: 0.6025,
  vector_score: 0.5184,
  retrieval_score: 0.7012,
};
const mockResearchThreads = [
  {
    id: "news-thread-1",
    title: "贵州茅台 研究",
    stock_code: "600519.SH",
    created_at_epoch_ms: Date.parse("2026-08-16T08:00:00Z"),
    updated_at_epoch_ms: Date.parse("2026-08-16T15:30:00Z"),
  },
];
const mockResearchThreadDetail = {
  answers: [
    {
      id: "news-answer-1",
      thread_id: "news-thread-1",
      mode: "model",
      question: "利润弹性主要来自哪里？",
      answer: "盈利弹性主要来自毛利率修复与费用率回落 [C1][C2]。",
      citations: [mockResearchCitationOne, mockResearchCitationTwo],
      created_at_epoch_ms: Date.parse("2026-08-16T15:31:00Z"),
    },
    {
      id: "news-answer-2",
      thread_id: "news-thread-1",
      mode: "evidence_only",
      question: "资金面是否同步改善？",
      answer: "资金面仍需结合后续公告和渠道反馈继续观察 [C3]。",
      citations: [mockResearchCitationThree],
      created_at_epoch_ms: Date.parse("2026-08-16T15:33:00Z"),
    },
  ],
};
const mockResearchOverviewEmpty = {
  schema_version: 2,
  document_count: 0,
  chunk_count: 0,
  unread_count: 0,
  unread_by_stock: {},
  messages: mockResearchMessagesEmpty,
  retrieval: { vector: { ready: false } },
};
const mockResearchOverviewData = {
  schema_version: 2,
  document_count: 18,
  chunk_count: 52,
  unread_count: 3,
  unread_by_stock: { "600519.SH": 3 },
  messages: mockResearchMessagesData,
  retrieval: { vector: { ready: true } },
};

const headerBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "phone-390-dark", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390-light", width: 390, height: 844, dpr: 3, mobile: true, theme: "light", density: "comfortable" },
];

const observeSummaryBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "tablet-768-dark", width: 768, height: 1024, dpr: 2, mobile: true, theme: "dark", density: "comfortable" },
  { name: "tablet-768-light", width: 768, height: 1024, dpr: 2, mobile: true, theme: "light", density: "comfortable" },
  { name: "phone-390-dark", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390-light", width: 390, height: 844, dpr: 3, mobile: true, theme: "light", density: "comfortable" },
];

const refreshToolbarBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
];

const newsPageBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "phone-390-dark", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390-light", width: 390, height: 844, dpr: 3, mobile: true, theme: "light", density: "comfortable" },
];

const backtestPageBaselineDevices = [
  { name: "desktop-1440-dark", width: 1440, height: 900, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1440-light", width: 1440, height: 900, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "desktop-1920-dark", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "dark", density: "comfortable" },
  { name: "desktop-1920-light", width: 1920, height: 1080, dpr: 1, mobile: false, theme: "light", density: "comfortable" },
  { name: "phone-390-dark", width: 390, height: 844, dpr: 3, mobile: true, theme: "dark", density: "comfortable" },
  { name: "phone-390-light", width: 390, height: 844, dpr: 3, mobile: true, theme: "light", density: "comfortable" },
];

const replayConversationId = "ui-replay-conversation";
const replayRunId = "ui-replay-run";
const replayConversation = {
  id: replayConversationId,
  title: "Agent 复盘浏览器门禁",
  mode: "expert",
  messages: [
    { role: "user", content: "复盘这次市场诊断", timestamp: 1_786_423_200_000 },
    {
      role: "assistant",
      content: "市场诊断已完成，可查看运行复盘。",
      timestamp: 1_786_423_204_000,
      runId: replayRunId,
    },
  ],
  createdAt: 1_786_423_200_000,
  updatedAt: 1_786_423_204_000,
};
const mockAgentRunSummary = {
  run_id: replayRunId,
  conversation_id: replayConversationId,
  question: "复盘这次市场诊断",
  mode: "expert",
  status: "completed",
  started_at_epoch_ms: 1_786_423_200_000,
  completed_at_epoch_ms: 1_786_423_204_000,
  duration_ms: 4_000,
};
const mockAgentRunDetail = {
  ...mockAgentRunSummary,
  events: [
    { type: "status", stage: "planning", label: "市场状态诊断" },
    { type: "tool_start", payload: { tool: "adaptive_screen", label: "自适应筛选" } },
    { type: "tool_result", payload: { status: "completed", output_summary: "候选与证据已生成" } },
    { type: "evidence", label: "证据目录已锁定" },
    { type: "final", message: "复盘结论已生成" },
  ],
  result: {
    reply: "市场诊断与决策路径已经完成。",
    action: "data_status",
    answer_sections: [
      { title: "复盘结论", bullets: ["市场状态与证据方向一致。"] },
    ],
    warnings: ["仅用于浏览器门禁验证。"],
    data: { source: "ui-replay-harness", state: "ready" },
  },
};

const contentTypes = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

async function startBuiltAppServer() {
  const indexPath = resolve(buildRoot, "index.html");
  if (!existsSync(indexPath)) {
    throw new Error("Built frontend is missing. Run npm run build:app before --serve.");
  }
  const rootPrefix = buildRoot.endsWith(sep) ? buildRoot : buildRoot + sep;
  const server = createServer((request, response) => {
    const requestUrl = new URL(request.url || "/", "http://127.0.0.1");
    const relativePath = decodeURIComponent(requestUrl.pathname).replace(/^\/+/, "") || "index.html";
    const candidate = resolve(buildRoot, relativePath);
    const allowed = candidate === buildRoot || candidate.startsWith(rootPrefix);
    const filePath = allowed && existsSync(candidate) && statSync(candidate).isFile()
      ? candidate
      : indexPath;
    response.writeHead(200, {
      "content-type": contentTypes[extname(filePath)] || "application/octet-stream",
      "cache-control": "no-store",
    });
    response.end(readFileSync(filePath));
  });
  await new Promise((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Unable to resolve harness server address");
  return {
    server,
    url: "http://127.0.0.1:" + address.port,
  };
}

function deviceUrl(device, hash) {
  const query = new URLSearchParams();
  if (device.theme === "light") query.set("theme", "light");
  if (device.density === "compact") query.set("density", "compact");
  return baseUrl + "/" + (query.size ? "?" + query.toString() : "") + hash;
}

async function installFixedResearchClock(page) {
  await page.addInitScript((fixedNow) => {
    const NativeDate = Date;
    globalThis.Date = new Proxy(NativeDate, {
      apply: () => new NativeDate(fixedNow).toString(),
      construct: (target, args, newTarget) => Reflect.construct(
        target,
        args.length ? args : [fixedNow],
        newTarget,
      ),
      get: (target, property, receiver) => property === "now"
        ? () => fixedNow
        : Reflect.get(target, property, receiver),
    });
  }, fixedResearchNow);
}

async function installHarnessState(page, observeResult = mockObserveResult, researchState = null) {
  await page.addInitScript((stocks) => {
    localStorage.setItem("stock-optimizer-watchlist", JSON.stringify(
      stocks.map((stock) => ({
        code: stock.code,
        name: stock.name,
        industry: stock.industry,
        source: "ui-harness",
        added_at: "2026-08-04T09:00:00.000Z",
      })),
    ));
  }, mockStocks);
  await page.route("**/api/data-sources/status", (route) => route.fulfill({ json: mockDataStatus }));
  await page.route("**/api/data-sources/refresh-universe", (route) => route.fulfill({
    json: { status: mockDataStatus, refreshed: true, notes: ["截图 harness 注入的股票池刷新日志。"] },
  }));
  await page.route("**/api/data-sources/prune-cache", (route) => route.fulfill({
    json: { status: mockDataStatus, removed_files: 2, removed_bytes: 1048576, notes: ["截图 harness 注入的缓存清理日志。"] },
  }));
  await page.route("**/api/stocks?*", (route) => route.fulfill({ json: mockStocks }));
  await page.route("**/api/stocks/*", (route) => {
    const symbol = decodeURIComponent(new URL(route.request().url()).pathname.split("/").pop() || "");
    const stock = mockBacktestSymbols.find((item) => item.code === symbol);
    return route.fulfill({
      status: stock ? 200 : 404,
      json: stock ?? { error: "stock not found" },
    });
  });
  await page.route("**/api/screen", (route) => route.fulfill({ json: mockScreenResult }));
  await page.route("**/api/observe/**", (route) => route.fulfill({ json: observeResult }));
  await page.route("**/api/backtest", (route) => route.fulfill({ json: mockBacktestResult }));
  const researchOverview = researchState?.overview ?? {};
  const researchMessages = researchState?.messages ?? [];
  const researchThreads = researchState?.threads ?? [];
  const researchDetail = researchState?.detail ?? { answers: [] };
  const researchIndexStatus = researchState?.indexStatus ?? {};
  await page.route("**/api/research/overview", (route) => route.fulfill({ json: researchOverview }));
  await page.route("**/api/research/messages?*", (route) => route.fulfill({ json: { items: researchMessages } }));
  await page.route("**/api/research/threads", (route) => route.fulfill({ json: { items: researchThreads } }));
  await page.route("**/api/research/threads/detail", (route) => route.fulfill({ json: researchDetail }));
  await page.route("**/api/research/index-status", (route) => route.fulfill({ json: researchIndexStatus }));
  await page.route("**/api/research/refresh*", (route) => route.fulfill({ json: { refreshed: true } }));
  await page.route("**/api/research/mark-read", (route) => route.fulfill({ json: { updated: 1 } }));
  await page.route("**/api/research/threads/delete", (route) => route.fulfill({ json: { deleted: 1 } }));
  await page.route("**/api/research/threads/create", (route) => route.fulfill({
    json: {
      id: "ui-created-thread",
      title: "新研究会话",
      stock_code: null,
      created_at_epoch_ms: fixedResearchNow,
      updated_at_epoch_ms: fixedResearchNow,
    },
  }));
  await page.route("**/api/research/query", (route) => route.fulfill({
    json: {
      id: "ui-query-answer",
      mode: "model",
      answer: "研究回答 [C1]",
      citations: [mockResearchCitationOne],
      created_at_epoch_ms: fixedResearchNow,
    },
  }));
}

async function captureHeaderBaselines(browser, targetRoot) {
  const reports = [];
  for (const device of headerBaselineDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: device.theme,
    });
    try {
      const page = await context.newPage();
      await installHarnessState(page);
      await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
      await page.locator(".app-header").waitFor({ state: "visible" });
      const directory = resolve(targetRoot, "header-settings", device.name);
      mkdirSync(directory, { recursive: true });
      await page.screenshot({ path: resolve(directory, "header-default.png"), fullPage: false });

      const trigger = page.locator(".settings-trigger");
      await trigger.click();
      const dialog = page.getByRole("dialog", { name: "设置" });
      await dialog.waitFor({ state: "visible" });
      await page.waitForTimeout(240);
      await page.screenshot({ path: resolve(directory, "settings-open.png"), fullPage: false });

      const panel = await dialog.boundingBox();
      if (!panel) throw new Error(`${device.name} settings panel has no bounding box`);
      const bottom = panel.y + panel.height;
      if (device.mobile) {
        if (panel.width < device.width - 2 || Math.abs(bottom - device.height) > 2
          || panel.height > device.height * 0.78 + 2) {
          throw new Error(`${device.name} settings sheet geometry is invalid: ${JSON.stringify(panel)}`);
        }
      } else if (Math.abs(panel.x + panel.width - device.width) > 2
        || Math.abs(panel.height - device.height) > 2) {
        throw new Error(`${device.name} settings drawer geometry is invalid: ${JSON.stringify(panel)}`);
      }

      reports.push({ device: device.name, panel });
    } finally {
      await context.close();
    }
  }
  return reports;
}

async function captureRefreshToolbarBaselines(browser, targetRoot) {
  const captureToolbarClip = async (page, path) => {
    const clip = await page.evaluate(() => {
      const toolbar = document.querySelector(".screen-toolbar-card.screen-toolbar-compact");
      if (!toolbar) return null;
      const boxes = [toolbar.getBoundingClientRect()];
      const panel = document.querySelector(".screen-refresh-maintenance[open] .screen-refresh-maintenance-panel");
      if (panel) boxes.push(panel.getBoundingClientRect());
      const left = Math.max(0, Math.floor(Math.min(...boxes.map((box) => box.left))));
      const top = Math.max(0, Math.floor(Math.min(...boxes.map((box) => box.top))));
      const right = Math.min(window.innerWidth, Math.ceil(Math.max(...boxes.map((box) => box.right))));
      const bottom = Math.min(window.innerHeight, Math.ceil(Math.max(...boxes.map((box) => box.bottom))));
      return { x: left, y: top, width: right - left, height: bottom - top };
    });
    if (!clip) throw new Error("Refresh toolbar clip target is missing");
    await page.screenshot({ path, clip });
  };

  const reports = [];
  for (const device of refreshToolbarBaselineDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: false,
      isMobile: false,
      colorScheme: device.theme,
    });
    try {
      const page = await context.newPage();
      await installHarnessState(page);
      await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
      const toolbar = page.locator(".screen-toolbar-card.screen-toolbar-compact");
      await toolbar.waitFor({ state: "visible" });

      await page.locator(".screen-toolbar-refresh-btn").click();
      const logToggle = page.locator(".refresh-log-toggle");
      await logToggle.waitFor({ state: "visible" });
      await page.waitForTimeout(1600);

      const directory = resolve(targetRoot, "refresh-toolbar", device.name);
      mkdirSync(directory, { recursive: true });

      if (await logToggle.getAttribute("aria-expanded") === "true") {
        await logToggle.click();
        await page.waitForTimeout(260);
      }
      await captureToolbarClip(page, resolve(directory, "collapsed.png"));

      await logToggle.click();
      await page.locator(".refresh-log-shell.open").waitFor({ state: "visible" });
      await page.waitForTimeout(260);
      await captureToolbarClip(page, resolve(directory, "log-expanded.png"));

      await logToggle.click();
      await page.waitForTimeout(260);
      await page.locator(".screen-refresh-maintenance > summary").click();
      await page.locator(".screen-refresh-maintenance[open] .screen-refresh-maintenance-panel").waitFor({ state: "visible" });
      await page.waitForTimeout(180);
      await captureToolbarClip(page, resolve(directory, "maintenance-open.png"));

      reports.push({
        device: device.name,
        states: ["collapsed.png", "log-expanded.png", "maintenance-open.png"],
      });
    } finally {
      await context.close();
    }
  }
  return reports;
}

const boxFor = async (page, selector) => {
  const locator = page.locator(selector).first();
  if (!await locator.count()) return null;
  return locator.evaluate((element) => {
    const box = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return {
      x: Math.round(box.x),
      y: Math.round(box.y),
      width: Math.round(box.width),
      height: Math.round(box.height),
      display: style.display,
      position: style.position,
      visibility: style.visibility,
    };
  });
};

const pageDiagnostics = async (page, routeName) => page.evaluate((name) => {
  const root = document.querySelector("#root");
  const app = document.querySelector(".app");
  const workspace = document.querySelector(".agent-workspace");
  const rail = document.querySelector(".agent-rail");
  const activeView = app?.getAttribute("data-active-view") || "";
  const overlay = document.querySelector("vite-error-overlay, #webpack-dev-server-client-overlay, [data-nextjs-dialog-overlay]");
  const railStyle = rail ? getComputedStyle(rail) : null;
  return {
    route: name,
    title: document.title,
    activeView,
    rootTextLength: root?.textContent?.trim().length || 0,
    rootWidth: root?.clientWidth || 0,
    rootScrollWidth: root?.scrollWidth || 0,
    bodyWidth: document.body.clientWidth,
    bodyScrollWidth: document.body.scrollWidth,
    frameworkOverlay: Boolean(overlay),
    workspaceClass: workspace?.className || "",
    railTransform: railStyle?.transform || "",
    railPosition: railStyle?.position || "",
    activeNavLabels: Array.from(document.querySelectorAll(".nav-link.active")).map((element) => element.textContent?.trim() || ""),
  };
}, routeName);

function assertPageDiagnostics(diagnostics, deviceName) {
  if (diagnostics.rootTextLength < 10) {
    throw new Error(deviceName + "/" + diagnostics.route + " rendered a blank application shell");
  }
  if (diagnostics.frameworkOverlay) {
    throw new Error(deviceName + "/" + diagnostics.route + " rendered a framework error overlay");
  }
  if (diagnostics.bodyScrollWidth > diagnostics.bodyWidth + 1) {
    throw new Error(deviceName + "/" + diagnostics.route + " has horizontal body overflow");
  }
}

async function runShortcutChecks(page, device) {
  await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
  await page.locator("#root").waitFor({ state: "visible" });

  await page.keyboard.press("2");
  await page.waitForFunction(() => window.location.hash === "#sectionObserve");

  await page.keyboard.press("Control+K");
  const search = page.getByLabel("搜索股票");
  if (!await search.evaluate((element) => document.activeElement === element)) {
    throw new Error("Ctrl+K did not focus global stock search");
  }

  await page.keyboard.press("1");
  if (await page.evaluate(() => window.location.hash) !== "#sectionObserve") {
    throw new Error("Number shortcut fired while the search input was focused");
  }

  await search.evaluate((element) => element.blur());
  await page.keyboard.press("?");
  const help = page.getByRole("dialog", { name: "快捷键帮助" });
  await help.waitFor({ state: "visible" });
  await page.keyboard.press("Escape");
  await help.waitFor({ state: "hidden" });

  return {
    ctrlKFocusedSearch: true,
    numberNavigationHash: "#sectionObserve",
    editableTargetIgnored: true,
    helpOpenedAndClosed: true,
  };
}

async function runSearchOverlayChecks(page, device) {
  await page.route('**/api/stock-search?*', (route) => route.fulfill({ json: mockStocks }));
  await page.goto(deviceUrl(device, '#sectionScreen'), { waitUntil: 'networkidle' });
  await page.locator('.header-search input').fill('tc');
  await page.locator('.header-search .stock-suggest button').first().waitFor({ state: 'visible' });

  const layering = await page.evaluate(() => {
    const header = document.querySelector('.app-header');
    const sidebar = document.querySelector('.sidebar');
    const dropdown = document.querySelector('.header-search .stock-suggest');
    if (!header || !sidebar || !dropdown) throw new Error('Search overlay elements are missing');
    const headerZIndex = Number.parseInt(getComputedStyle(header).zIndex, 10);
    const sidebarZIndex = Number.parseInt(getComputedStyle(sidebar).zIndex, 10);
    const dropdownBox = dropdown.getBoundingClientRect();
    const sidebarBox = sidebar.getBoundingClientRect();
    const overlapWidth = Math.max(0, Math.min(dropdownBox.right, sidebarBox.right) - Math.max(dropdownBox.left, sidebarBox.left));
    const overlapHeight = Math.max(0, Math.min(dropdownBox.bottom, sidebarBox.bottom) - Math.max(dropdownBox.top, sidebarBox.top));
    const sampleX = Math.max(dropdownBox.left, sidebarBox.left) + Math.min(8, overlapWidth / 2);
    const sampleY = Math.max(dropdownBox.top, sidebarBox.top) + Math.min(8, overlapHeight / 2);
    const topElement = overlapWidth > 0 && overlapHeight > 0
      ? document.elementFromPoint(sampleX, sampleY)
      : dropdown;
    return {
      headerZIndex,
      sidebarZIndex,
      dropdownIsTopLayer: Boolean(topElement && dropdown.contains(topElement)),
      dropdownBox: {
        x: Math.round(dropdownBox.x),
        y: Math.round(dropdownBox.y),
        width: Math.round(dropdownBox.width),
        height: Math.round(dropdownBox.height),
      },
      sidebarBox: {
        x: Math.round(sidebarBox.x),
        y: Math.round(sidebarBox.y),
        width: Math.round(sidebarBox.width),
        height: Math.round(sidebarBox.height),
      },
      overlap: Math.round(overlapWidth * overlapHeight),
    };
  });

  if (!layering.dropdownIsTopLayer) {
    throw new Error('Header search overlay is obscured by the sidebar: ' + JSON.stringify(layering));
  }
  return layering;
}

async function installAgentReplayState(page) {
  await page.addInitScript(({ conversation, activeConversationId }) => {
    localStorage.setItem("stock-optimizer-agent-conversations", JSON.stringify([conversation]));
    localStorage.setItem("stock-optimizer-agent-active-conversation", JSON.stringify(activeConversationId));
  }, {
    conversation: replayConversation,
    activeConversationId: replayConversationId,
  });
}

async function installAgentReplayRoutes(page, requests) {
  await page.route("**/api/agent/runs?*", (route) => {
    const url = new URL(route.request().url());
    requests.list.push({
      conversationId: url.searchParams.get("conversation_id"),
      url: url.toString(),
    });
    return route.fulfill({ json: { runs: [mockAgentRunSummary] } });
  });
  await page.route("**/api/agent/runs/**", (route) => {
    const url = new URL(route.request().url());
    requests.detail.push(url.toString());
    if (url.pathname !== `/api/agent/runs/${replayRunId}`) {
      return route.fulfill({ status: 404, json: { run: null } });
    }
    return route.fulfill({ json: { run: mockAgentRunDetail } });
  });
}

function assertReplay(condition, message, diagnostics) {
  if (condition) return;
  const suffix = diagnostics === undefined ? "" : ": " + JSON.stringify(diagnostics);
  throw new Error("Agent replay check failed: " + message + suffix);
}

function assertBoxInViewport(box, device, label) {
  assertReplay(Boolean(box), label + " has no bounding box");
  const tolerance = 2;
  assertReplay(
    box.x >= -tolerance
      && box.y >= -tolerance
      && box.x + box.width <= device.width + tolerance
      && box.y + box.height <= device.height + tolerance,
    label + " is outside the viewport",
    { box, viewport: { width: device.width, height: device.height } },
  );
}

async function replayGeometry(page) {
  return page.evaluate(() => {
    const rect = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return null;
      const box = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      return {
        x: box.x,
        y: box.y,
        width: box.width,
        height: box.height,
        right: box.right,
        bottom: box.bottom,
        position: style.position,
        overflowY: style.overflowY,
      };
    };
    const drawer = document.querySelector(".agent-run-drawer");
    const sample = document.elementFromPoint(4, Math.floor(window.innerHeight / 2));
    return {
      viewport: { width: window.innerWidth, height: window.innerHeight },
      bodyScrollWidth: document.body.scrollWidth,
      rootScrollWidth: document.documentElement.scrollWidth,
      drawer: rect(".agent-run-drawer"),
      header: rect(".agent-run-drawer-header"),
      body: rect(".agent-run-detail"),
      back: rect(".agent-run-drawer-back"),
      close: rect(".agent-run-drawer-close"),
      rail: rect(".agent-rail"),
      stage: rect(".agent-chat-stage"),
      drawerOwnsMobileSample: Boolean(drawer && sample && drawer.contains(sample)),
    };
  });
}

function assertAgentReplayGeometry(geometry, device) {
  const tolerance = 2;
  const { drawer, header, body, back, close, rail, stage, viewport } = geometry;
  assertReplay(Boolean(drawer && header && body && back && close && rail && stage), "layout elements are missing", geometry);
  assertReplay(geometry.bodyScrollWidth <= viewport.width + 1, "body has horizontal overflow", geometry);
  assertReplay(geometry.rootScrollWidth <= viewport.width + 1, "document has horizontal overflow", geometry);
  for (const [label, box] of [["drawer", drawer], ["header", header], ["detail body", body], ["back", back], ["close", close]]) {
    assertBoxInViewport(box, device, `${device.name} ${label}`);
  }
  assertReplay(body.y >= header.bottom - tolerance, "drawer header overlaps the scroll body", geometry);
  assertReplay(body.bottom <= drawer.bottom + tolerance, "drawer scroll body is clipped", geometry);
  assertReplay(Math.abs(header.x - drawer.x) <= tolerance && Math.abs(header.right - drawer.right) <= tolerance, "header width does not match drawer", geometry);
  assertReplay(Math.abs(body.x - drawer.x) <= tolerance && Math.abs(body.right - drawer.right) <= tolerance, "scroll body width does not match drawer", geometry);
  assertReplay(["auto", "scroll"].includes(body.overflowY), "detail body is not scrollable", geometry);

  if (device.name === "desktop-1440") {
    assertReplay(drawer.position === "absolute", "desktop drawer is not an overlay", geometry);
    assertReplay(drawer.width >= 440 - tolerance && drawer.width <= 640 + tolerance, "desktop drawer width is outside clamp", geometry);
    assertReplay(Math.abs(drawer.right - stage.right) <= tolerance, "desktop drawer is not aligned to the chat stage", geometry);
    assertReplay(rail.right <= drawer.x + tolerance, "conversation rail overlaps the desktop drawer", geometry);
    return "overlay:" + Math.round(drawer.width) + "px";
  }

  assertReplay(drawer.position === "fixed", "mobile drawer is not fixed", geometry);
  assertReplay(
    Math.abs(drawer.x) <= tolerance
      && Math.abs(drawer.y) <= tolerance
      && Math.abs(drawer.width - viewport.width) <= tolerance
      && Math.abs(drawer.height - viewport.height) <= tolerance,
    "mobile drawer is not a full-screen sheet",
    geometry,
  );
  assertReplay(geometry.drawerOwnsMobileSample, "conversation rail covers the mobile drawer", geometry);
  return "sheet:" + Math.round(drawer.width) + "x" + Math.round(drawer.height);
}

async function runAgentReplayChecks(browser) {
  const replayDevices = devices.filter((device) => ["desktop-1440", "tablet-768", "phone-390"].includes(device.name));
  const reports = [];

  for (const device of replayDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    const consoleIssues = [];
    const requests = { list: [], detail: [] };
    page.on("console", (message) => {
      if (message.type() === "error") consoleIssues.push("console: " + message.text());
    });
    page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

    try {
      await installHarnessState(page);
      await installAgentReplayState(page);
      await installAgentReplayRoutes(page, requests);
      await page.goto(deviceUrl(device, "#sectionAgent"), { waitUntil: "networkidle" });
      await page.locator(".agent-workspace").waitFor({ state: "visible" });

      const historyTrigger = page.locator(device.mobile ? ".agent-mobile-history" : ".agent-thread-history");
      assertReplay(await historyTrigger.isVisible(), `${device.name} history trigger is not visible`);
      assertBoxInViewport(await historyTrigger.boundingBox(), device, `${device.name} history trigger`);

      const currentRequest = page.waitForRequest((request) => {
        const url = new URL(request.url());
        return url.pathname === "/api/agent/runs" && url.searchParams.get("conversation_id") === replayConversationId;
      });
      await historyTrigger.click();
      await currentRequest;

      const dialog = page.getByRole("dialog", { name: "Agent 运行复盘" });
      await dialog.waitFor({ state: "visible" });
      const runRow = dialog.locator(".agent-run-select");
      await runRow.waitFor({ state: "visible" });
      const currentScope = dialog.getByRole("button", { name: "当前会话" });
      const allScope = dialog.getByRole("button", { name: "全部运行" });
      assertBoxInViewport(await currentScope.boundingBox(), device, `${device.name} current scope`);
      assertBoxInViewport(await allScope.boundingBox(), device, `${device.name} all scope`);
      assertReplay(requests.list[0]?.conversationId === replayConversationId, "current scope omitted conversation_id", requests.list);

      const allRequest = page.waitForRequest((request) => {
        const url = new URL(request.url());
        return url.pathname === "/api/agent/runs" && !url.searchParams.has("conversation_id");
      });
      await allScope.click();
      await allRequest;
      await runRow.waitFor({ state: "visible" });
      assertReplay(requests.list.some((request) => request.conversationId === null), "all scope retained conversation_id", requests.list);

      const detailRequest = page.waitForRequest((request) => new URL(request.url()).pathname === `/api/agent/runs/${replayRunId}`);
      await runRow.click();
      await detailRequest;
      const detail = dialog.locator(".agent-run-detail");
      await detail.waitFor({ state: "visible" });
      await dialog.locator(".agent-run-overview").getByText(mockAgentRunSummary.question).waitFor();
      assertReplay(await dialog.locator(".agent-run-timeline-item").count() >= 4, "timeline events were not rendered");
      await dialog.locator(".agent-answer-sections").getByText("复盘结论").waitFor();
      await dialog.locator(".agent-warning-list").getByText("仅用于浏览器门禁验证。").waitFor();
      await dialog.locator(".agent-run-result").waitFor({ state: "visible" });

      const backButton = dialog.getByRole("button", { name: "返回运行列表" });
      await page.waitForFunction(() => document.activeElement?.classList.contains("agent-run-drawer-back"));
      assertReplay(await backButton.evaluate((element) => document.activeElement === element), "focus left the drawer after list-to-detail navigation");
      const layout = assertAgentReplayGeometry(await replayGeometry(page), device);

      await page.keyboard.press("Escape");
      await dialog.waitFor({ state: "hidden" });
      assertReplay(await historyTrigger.evaluate((element) => document.activeElement === element), "Escape did not restore focus to the history trigger");

      const directTrigger = page.locator(".agent-message-replay");
      assertReplay(await directTrigger.isVisible(), `${device.name} direct replay trigger is not visible`);
      assertBoxInViewport(await directTrigger.boundingBox(), device, `${device.name} direct replay trigger`);
      const directRequest = page.waitForRequest((request) => new URL(request.url()).pathname === `/api/agent/runs/${replayRunId}`);
      await directTrigger.click();
      await directRequest;
      await dialog.locator(".agent-run-detail").waitFor({ state: "visible" });
      await dialog.locator(".agent-run-overview").getByText(mockAgentRunSummary.question).waitFor();
      assertReplay(requests.detail.length === 2, "direct replay did not load the exact detail", requests.detail);
      await page.keyboard.press("Escape");
      await dialog.waitFor({ state: "hidden" });
      assertReplay(await directTrigger.evaluate((element) => document.activeElement === element), "direct replay did not restore focus");
      assertReplay(consoleIssues.length === 0, `${device.name} logged browser errors`, consoleIssues);

      reports.push({
        device: device.name,
        layout,
        currentRequests: requests.list.filter((request) => request.conversationId === replayConversationId).length,
        allRequests: requests.list.filter((request) => request.conversationId === null).length,
        detailRequests: requests.detail.length,
      });
    } finally {
      await context.close();
    }
  }

  console.log("Agent replay checks passed: " + reports.map((report) => (
    `${report.device}[${report.layout},current=${report.currentRequests},all=${report.allRequests},detail=${report.detailRequests}]`
  )).join(" | "));
}

async function captureDenseStates(page, device, targetRoot) {
  const directory = resolve(targetRoot, "desktop-1440-data");
  mkdirSync(directory, { recursive: true });

  await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
  await openCustomScreen(page);
  await page.screenshot({ path: resolve(directory, "screen-custom-criteria.png"), fullPage: false });

  await openSmartScreen(page);
  await page.locator(".header-search input").fill("");
  await page.locator(".screen-panel-container .run-btn").click();
  await page.locator(".stock-row").first().waitFor();
  await page.screenshot({ path: resolve(directory, "screen-dense.png"), fullPage: false });

  await page.goto(deviceUrl(device, "#sectionObserve"), { waitUntil: "networkidle" });
  await page.locator("#observeCode").fill("600519.SH");
  await page.locator(".observe-run-btn").click();
  await page.locator(".observe-result").waitFor();
  await page.screenshot({ path: resolve(directory, "observe-dense.png"), fullPage: false });

  await page.goto(deviceUrl(device, "#sectionBacktest"), { waitUntil: "networkidle" });
  await page.getByLabel("运行回测").click();
  await page.locator(".backtest-result").waitFor();
  await page.screenshot({ path: resolve(directory, "backtest-dense.png"), fullPage: false });

  return [
    "screen-custom-criteria.png",
    "screen-dense.png",
    "observe-dense.png",
    "backtest-dense.png",
  ];
}

async function assertBacktestRuntimeColors(page) {
  const readColors = () => page.evaluate(() => {
    const resolvedToken = (token) => {
      const probe = document.createElement("span");
      probe.style.color = `var(${token})`;
      document.body.appendChild(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    };
    const styleFor = (selector, property = "color") => {
      const pseudoElement = selector.endsWith("::before") ? "::before" : null;
      const elementSelector = pseudoElement ? selector.slice(0, -8) : selector;
      const element = document.querySelector(elementSelector);
      if (!element) throw new Error(`Missing color audit target: ${selector}`);
      return getComputedStyle(element, pseudoElement)[property];
    };
    return {
      tokens: {
        text: resolvedToken("--text"),
        textSecondary: resolvedToken("--text-secondary"),
        rise: resolvedToken("--rise"),
        fall: resolvedToken("--fall"),
        success: resolvedToken("--success"),
        error: resolvedToken("--error"),
        chartLine1: resolvedToken("--chart-line-1"),
        headline: getComputedStyle(document.documentElement).getPropertyValue("--fs-headline").trim(),
      },
      summary: styleFor(".volatility-interpretation-summary"),
      positiveMetric: styleFor(".metric-strip .metric strong.positive"),
      negativeMetric: styleFor(".metric-strip .metric strong.negative"),
      directionalVolatility: styleFor(".volatility-grid .volatility-value.positive"),
      neutralVolatility: styleFor(".volatility-grid > div:last-child .volatility-value"),
      gatePassed: styleFor(".backtest-gate-status.passed::before", "backgroundColor"),
      gateFailed: styleFor(".backtest-gate-status.failed::before", "backgroundColor"),
      smallMetricLabel: styleFor(".metric-strip .metric > span"),
      volatilityLabel: styleFor(".volatility-symbol-control > span"),
      interpretationMeta: styleFor(".volatility-interpretation > header span"),
      heroFontSize: styleFor(".metric-strip .metric-hero > strong", "fontSize"),
      portfolioCurve: styleFor(".equity-chart-line.is-portfolio", "stroke"),
    };
  });

  const positive = await readColors();
  const expectedPositive = {
    summary: positive.tokens.text,
    positiveMetric: positive.tokens.rise,
    negativeMetric: positive.tokens.fall,
    directionalVolatility: positive.tokens.rise,
    neutralVolatility: positive.tokens.text,
    gatePassed: positive.tokens.success,
    gateFailed: positive.tokens.error,
    smallMetricLabel: positive.tokens.textSecondary,
    volatilityLabel: positive.tokens.textSecondary,
    interpretationMeta: positive.tokens.textSecondary,
    heroFontSize: positive.tokens.headline,
    portfolioCurve: positive.tokens.chartLine1,
  };
  for (const [key, expected] of Object.entries(expectedPositive)) {
    if (positive[key] !== expected) {
      throw new Error(`Backtest color audit failed for ${key}: expected ${expected}, received ${positive[key]}`);
    }
  }

  const select = page.locator(".volatility-symbol-control select");
  await select.selectOption("000858.SZ");
  await page.locator(".volatility-grid .volatility-value.negative").waitFor({ state: "visible" });
  const negativeVolatility = await page.locator(".volatility-grid .volatility-value.negative")
    .evaluate((element) => getComputedStyle(element).color);
  if (negativeVolatility !== positive.tokens.fall) {
    throw new Error(`Backtest color audit failed for negative volatility: expected ${positive.tokens.fall}, received ${negativeVolatility}`);
  }

  return { ...positive, negativeVolatility };
}

async function captureBacktestPageBaselines(browser, targetRoot) {
  const reports = [];
  for (const device of backtestPageBaselineDevices) {
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      hasTouch: device.mobile,
      isMobile: device.mobile,
      colorScheme: device.theme,
    });
    const page = await context.newPage();
    const consoleIssues = [];
    page.on("console", (message) => {
      if (message.type() === "warning" || message.type() === "error") {
        consoleIssues.push(message.type() + ": " + message.text());
      }
    });
    page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

    try {
      await installHarnessState(page);
      await page.goto(deviceUrl(device, "#sectionBacktest"), { waitUntil: "networkidle" });
      await page.getByLabel("运行回测").click();
      await page.locator(".backtest-result").waitFor({ state: "visible" });
      await page.locator(".backtest-volatility").waitFor({ state: "visible" });
      await page.waitForTimeout(180);

      const diagnostics = await pageDiagnostics(page, "backtest-page");
      assertPageDiagnostics(diagnostics, device.name);
      const directory = resolve(targetRoot, "backtest-page", device.name);
      mkdirSync(directory, { recursive: true });
      const mobileNavigationOverride = await page.addStyleTag({
        content: "@media (max-width: 768px) { .sidebar { visibility: hidden !important; } }",
      });
      await page.screenshot({ path: resolve(directory, "backtest.png"), fullPage: true });
      const volatility = page.locator(".backtest-volatility");
      await volatility.scrollIntoViewIfNeeded();
      const stickyHeaderOverride = await page.addStyleTag({
        content: ".app-header { visibility: hidden !important; }",
      });
      await volatility.screenshot({ path: resolve(directory, "volatility.png") });
      await stickyHeaderOverride.evaluate((element) => element.remove());
      await mobileNavigationOverride.evaluate((element) => element.remove());
      const runtimeColors = await assertBacktestRuntimeColors(page);
      if (consoleIssues.length) {
        throw new Error(`${device.name} logged browser issues: ${consoleIssues.join(" | ")}`);
      }
      reports.push({
        device: device.name,
        page: `backtest-page/${device.name}/backtest.png`,
        volatility: `backtest-page/${device.name}/volatility.png`,
        selector: ".backtest-volatility",
        diagnostics,
        runtimeColors,
      });
    } finally {
      await context.close();
    }
  }
  return reports;
}

function backtestPageReport(backtestPageBaselines) {
  if (stableReport) return { backtestPageBaselines };
  return { baseUrl, generatedAt: new Date().toISOString(), backtestPageBaselines };
}

async function captureObserveSummaryBaselines(browser, targetRoot) {
  const scenarios = [
    { name: "complete-capital", result: mockObserveResultCompleteCapital },
    { name: "capital-unavailable", result: mockObserveResultUnavailableCapital },
  ];
  const reports = [];

  for (const scenario of scenarios) {
    for (const device of observeSummaryBaselineDevices) {
      const context = await browser.newContext({
        viewport: { width: device.width, height: device.height },
        screen: { width: device.width, height: device.height },
        deviceScaleFactor: device.dpr,
        hasTouch: device.mobile,
        isMobile: device.mobile,
        colorScheme: device.theme,
      });
      const page = await context.newPage();
      const consoleIssues = [];
      page.on("console", (message) => {
        if (message.type() === "warning" || message.type() === "error") {
          consoleIssues.push(message.type() + ": " + message.text());
        }
      });
      page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

      try {
        await installHarnessState(page, scenario.result);
        await page.goto(deviceUrl(device, "#sectionObserve"), { waitUntil: "networkidle" });
        await page.locator("#observeCode").fill("600519.SH");
        await page.locator(".observe-run-btn").click();
        const summary = page.locator(".observe-decision-summary");
        await summary.waitFor({ state: "visible" });
        await summary.scrollIntoViewIfNeeded();
        await page.waitForTimeout(180);

        const directory = resolve(targetRoot, "observe-summary", scenario.name, device.name);
        mkdirSync(directory, { recursive: true });
        const fileName = "summary.png";
        await summary.screenshot({ path: resolve(directory, fileName) });
        const box = await summary.boundingBox();
        const diagnostics = await pageDiagnostics(page, `observe-summary-${scenario.name}`);
        assertPageDiagnostics(diagnostics, device.name);
        if (consoleIssues.length) {
          throw new Error(`${device.name}/${scenario.name} logged browser issues: ${consoleIssues.join(" | ")}`);
        }
        reports.push({ scenario: scenario.name, device: device.name, file: `observe-summary/${scenario.name}/${device.name}/${fileName}`, box });
      } finally {
        await context.close();
      }
    }
  }

  return reports;
}

async function assertNewsPageState(page, device, scenarioName) {
  const riskBoundary = page.locator(".research-risk-boundary");
  await riskBoundary.waitFor({ state: "visible" });
  const riskText = (await riskBoundary.innerText()).trim();
  if (riskText !== "仅供研究，不构成投资建议。") {
    throw new Error(`${device.name}/${scenarioName} does not show the full research risk boundary`);
  }

  const composerBox = await page.locator(".research-composer").boundingBox();
  if (!composerBox || (device.mobile && composerBox.height > 120.5)) {
    throw new Error(`${device.name}/${scenarioName} composer exceeds the 120px mobile limit: ${JSON.stringify(composerBox)}`);
  }

  const checks = {
    riskBoundary: riskText,
    composerHeight: Math.round(composerBox.height * 10) / 10,
  };

  if (device.mobile) {
    const touchTargets = await page.locator(
      ".research-mobile-inbox-button, .research-actions button",
    ).evaluateAll((elements) => elements.map((element) => {
      const box = element.getBoundingClientRect();
      return {
        label: element.getAttribute("aria-label") || element.getAttribute("title") || element.textContent?.trim(),
        width: box.width,
        height: box.height,
      };
    }));
    const undersized = touchTargets.filter((target) => target.width < 43.5 || target.height < 43.5);
    if (touchTargets.length < 3 || undersized.length) {
      throw new Error(`${device.name}/${scenarioName} has undersized topbar touch targets: ${JSON.stringify(undersized)}`);
    }
    checks.touchTargets = touchTargets;

    if (scenarioName === "empty") {
      const emptyActions = await page.locator(".research-empty-actions button")
        .evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().height));
      if (!emptyActions.length || emptyActions.some((height) => height < 43.5)) {
        throw new Error(`${device.name}/${scenarioName} has undersized empty-state actions: ${JSON.stringify(emptyActions)}`);
      }
      checks.emptyActionHeights = emptyActions;
    }
  }

  if (!device.mobile && scenarioName === "empty") {
    const evidenceOverflow = await page.locator(".research-evidence").evaluate((element) => ({
      clientHeight: element.clientHeight,
      scrollHeight: element.scrollHeight,
    }));
    if (evidenceOverflow.scrollHeight > evidenceOverflow.clientHeight + 1) {
      throw new Error(`${device.name}/${scenarioName} evidence empty state scrolls: ${JSON.stringify(evidenceOverflow)}`);
    }
    checks.evidenceOverflow = evidenceOverflow;
  }

  if (scenarioName === "data") {
    const summaryCounts = await page.locator(".research-stat > strong").allTextContents();
    if (summaryCounts.length !== 4 || summaryCounts.slice(0, 3).some((value) => Number(value) === 0)) {
      throw new Error(`${device.name}/${scenarioName} fixture is no longer counted as today: ${JSON.stringify(summaryCounts)}`);
    }
    checks.summaryCounts = summaryCounts;
  }

  return checks;
}

async function captureNewsPageBaselines(browser, targetRoot) {
  const scenarios = [
    {
      name: "empty",
      researchState: {
        overview: mockResearchOverviewEmpty,
        messages: mockResearchMessagesEmpty,
        threads: [],
        detail: { answers: [] },
        indexStatus: {},
      },
      selectCitation: false,
    },
    {
      name: "data",
      researchState: {
        overview: mockResearchOverviewData,
        messages: mockResearchMessagesData,
        threads: mockResearchThreads,
        detail: mockResearchThreadDetail,
        indexStatus: { schema_version: 1, document_count: 18, chunk_count: 52 },
      },
      selectCitation: true,
    },
  ];
  const reports = [];

  for (const scenario of scenarios) {
    for (const device of newsPageBaselineDevices) {
      const context = await browser.newContext({
        viewport: { width: device.width, height: device.height },
        screen: { width: device.width, height: device.height },
        deviceScaleFactor: device.dpr,
        hasTouch: device.mobile,
        isMobile: device.mobile,
        colorScheme: device.theme,
      });
      const page = await context.newPage();
      const consoleIssues = [];
      page.on("console", (message) => {
        if (message.type() === "warning" || message.type() === "error") {
          consoleIssues.push(message.type() + ": " + message.text());
        }
      });
      page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

      try {
        await installFixedResearchClock(page);
        await installHarnessState(page, mockObserveResult, scenario.researchState);
        await page.goto(deviceUrl(device, "#sectionNewsRag"), { waitUntil: "networkidle" });
        await page.locator("#root").waitFor({ state: "visible" });
        await page.locator(".research-workspace").waitFor({ state: "visible" });
        await page.mouse.move(device.width / 2, Math.min(180, device.height / 3));
        await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());

        if (scenario.selectCitation && !device.mobile) {
          const citation = page.locator(".research-inline-citation").first();
          await citation.waitFor({ state: "visible" });
          await citation.click();
          await page.waitForTimeout(160);
          await page.locator(".research-stream-body").evaluate((element) => element.scrollTo(0, 0));
        }

        const checks = await assertNewsPageState(page, device, scenario.name);

        const directory = resolve(targetRoot, "news", scenario.name, device.name);
        mkdirSync(directory, { recursive: true });
        await page.screenshot({ path: resolve(directory, "news.png"), fullPage: false });
        const diagnostics = await pageDiagnostics(page, `news-${scenario.name}`);
        assertPageDiagnostics(diagnostics, device.name);
        if (consoleIssues.length) {
          throw new Error(`${device.name}/${scenario.name} logged browser issues: ${consoleIssues.join(" | ")}`);
        }
        reports.push({
          scenario: scenario.name,
          device: device.name,
          file: `news/${scenario.name}/${device.name}/news.png`,
          box: await boxFor(page, ".research-workspace"),
          checks,
        });
      } finally {
        await context.close();
      }
    }
  }

  const phoneDark = newsPageBaselineDevices.find((device) => device.name === "phone-390-dark");
  if (!phoneDark) return reports;

  const overlayStates = [
    {
      name: "phone-inbox-open",
      file: "inbox-open.png",
      open: async (page) => {
        await page.locator(".research-mobile-inbox-button").click();
        await page.locator(".research-inbox.mobile-open").waitFor({ state: "visible" });
      },
    },
    {
      name: "phone-evidence-open",
      file: "evidence-open.png",
      open: async (page) => {
        const citation = page.locator(".research-inline-citation").first();
        await citation.waitFor({ state: "visible" });
        await citation.click();
        await page.locator(".research-evidence.has-selection").waitFor({ state: "visible" });
      },
    },
  ];

  for (const overlay of overlayStates) {
    const context = await browser.newContext({
      viewport: { width: phoneDark.width, height: phoneDark.height },
      screen: { width: phoneDark.width, height: phoneDark.height },
      deviceScaleFactor: phoneDark.dpr,
      hasTouch: phoneDark.mobile,
      isMobile: phoneDark.mobile,
      colorScheme: phoneDark.theme,
    });
    const page = await context.newPage();
    const consoleIssues = [];
    page.on("console", (message) => {
      if (message.type() === "warning" || message.type() === "error") {
        consoleIssues.push(message.type() + ": " + message.text());
      }
    });
    page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));

    try {
      await installFixedResearchClock(page);
      await installHarnessState(page, mockObserveResult, {
        overview: mockResearchOverviewData,
        messages: mockResearchMessagesData,
        threads: mockResearchThreads,
        detail: mockResearchThreadDetail,
        indexStatus: { schema_version: 1, document_count: 18, chunk_count: 52 },
      });
      await page.goto(deviceUrl(phoneDark, "#sectionNewsRag"), { waitUntil: "networkidle" });
      await page.locator("#root").waitFor({ state: "visible" });
      await page.locator(".research-workspace").waitFor({ state: "visible" });
      await page.mouse.move(phoneDark.width / 2, Math.min(180, phoneDark.height / 3));
      await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
      await page.locator(".research-inline-citation").first().waitFor({ state: "visible" });
      await overlay.open(page);
      await page.waitForTimeout(160);
      const checks = await assertNewsPageState(page, phoneDark, "data");

      const directory = resolve(targetRoot, "news", overlay.name, phoneDark.name);
      mkdirSync(directory, { recursive: true });
      await page.screenshot({ path: resolve(directory, overlay.file), fullPage: false });
      const diagnostics = await pageDiagnostics(page, overlay.name);
      assertPageDiagnostics(diagnostics, phoneDark.name);
      if (consoleIssues.length) {
        throw new Error(`${phoneDark.name}/${overlay.name} logged browser issues: ${consoleIssues.join(" | ")}`);
      }
      reports.push({
        scenario: overlay.name,
        device: phoneDark.name,
        file: `news/${overlay.name}/${phoneDark.name}/${overlay.file}`,
        box: await boxFor(page, ".research-workspace"),
        checks,
      });
    } finally {
      await context.close();
    }
  }

  const interactionStates = [
    {
      name: "event-expanded",
      open: async (page) => {
        await page.locator(".research-event").first().click();
        await page.locator(".research-event.selected").waitFor({ state: "visible" });
      },
    },
    {
      name: "evidence-history",
      open: async (page) => {
        const citations = page.locator(".research-inline-citation");
        await citations.nth(0).click();
        await citations.nth(1).click();
        await page.getByRole("button", { name: "上一条证据" }).waitFor({ state: "visible" });
      },
    },
    {
      name: "delete-confirmation",
      open: async (page) => {
        const deleteButton = page.getByRole("button", { name: /删除研究会话/ }).first();
        await deleteButton.click();
        await page.getByText("确认删除？").waitFor({ state: "visible" });
      },
    },
    {
      name: "skeleton-loading",
      open: async (page) => {
        await page.route("**/api/research/messages?*", async (route) => {
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 500));
          await route.fulfill({ json: { items: mockResearchMessagesData } });
        });
        await page.evaluate(() => {
          const button = document.querySelector(".research-stock-row:not(.active)");
          button?.click();
        });
        await page.locator(".research-skeleton-list").waitFor({ state: "visible" });
      },
    },
  ];
  const interactionDevices = newsPageBaselineDevices;
  for (const state of interactionStates) {
    for (const device of interactionDevices) {
      const context = await browser.newContext({
        viewport: { width: device.width, height: device.height },
        screen: { width: device.width, height: device.height },
        deviceScaleFactor: device.dpr,
        hasTouch: device.mobile,
        isMobile: device.mobile,
        colorScheme: device.theme,
      });
      const page = await context.newPage();
      try {
      await installFixedResearchClock(page);
      await installHarnessState(page, mockObserveResult, {
        overview: mockResearchOverviewData,
        messages: mockResearchMessagesData,
        threads: mockResearchThreads,
        detail: mockResearchThreadDetail,
        indexStatus: { schema_version: 1, document_count: 18, chunk_count: 52 },
      });
      await page.goto(deviceUrl(device, "#sectionNewsRag"), { waitUntil: "networkidle" });
      await page.locator(".research-workspace").waitFor({ state: "visible" });
      if (device.mobile && ["delete-confirmation", "skeleton-loading"].includes(state.name)) {
        await page.locator(".research-mobile-inbox-button").click();
        await page.locator(".research-inbox.mobile-open").waitFor({ state: "visible" });
      }
      await state.open(page);
      await page.waitForTimeout(160);
      const directory = resolve(targetRoot, "news", state.name, device.name);
      mkdirSync(directory, { recursive: true });
      await page.screenshot({ path: resolve(directory, `${state.name}.png`), fullPage: false });
      reports.push({
        scenario: state.name,
        device: device.name,
        file: `news/${state.name}/${device.name}/${state.name}.png`,
        box: await boxFor(page, ".research-workspace"),
      });
    } finally {
        await context.close();
      }
    }
  }

  return reports;
}

async function runNewsInteractionChecks(browser) {
  const device = devices.find((item) => item.name === "desktop-1440");
  const context = await browser.newContext({
    viewport: { width: device.width, height: device.height },
    screen: { width: device.width, height: device.height },
    deviceScaleFactor: device.dpr,
    colorScheme: "dark",
  });
  const page = await context.newPage();
  const markReadRequests = [];
  await installFixedResearchClock(page);
  await page.addInitScript(() => {
    localStorage.setItem("stock-optimizer-llm-settings", JSON.stringify({
      active_provider_id: "ui-test",
      providers: [{
        id: "ui-test",
        name: "UI 测试模型",
        provider: "openai-compatible",
        base_url: "https://example.com/v1",
        model: "ui-test-model",
        api_key: "ui-test-key",
      }],
    }));
  });
  await installHarnessState(page, mockObserveResult, {
    overview: mockResearchOverviewData,
    messages: mockResearchMessagesData,
    threads: mockResearchThreads,
    detail: mockResearchThreadDetail,
    indexStatus: { schema_version: 1, document_count: 18, chunk_count: 52 },
  });
  await page.route("**/api/research/mark-read", (route) => {
    markReadRequests.push(JSON.parse(route.request().postData() || "{}"));
    return route.fulfill({ json: { updated: 3 } });
  });
  await page.goto(deviceUrl(device, "#sectionNewsRag"), { waitUntil: "networkidle" });
  await page.locator(".research-workspace").waitFor({ state: "visible" });

  const firstEvent = page.locator(".research-event").first();
  await firstEvent.click();
  await page.locator(".research-event.selected:not(.unread)").waitFor({ state: "visible" });
  const eventExpanded = await firstEvent.getAttribute("aria-expanded");
  const eventClass = await firstEvent.getAttribute("class");
  const eventText = await firstEvent.innerText();
  if (eventExpanded !== "true" || !eventClass?.includes("selected") || eventText.includes("未读")) {
    throw new Error("event click did not expand/select and mark read: " + JSON.stringify({
      expanded: eventExpanded,
      className: eventClass,
      text: eventText,
      requests: markReadRequests,
    }));
  }
  await page.keyboard.press("Escape");
  if (await firstEvent.getAttribute("aria-expanded") !== "false") {
    throw new Error("Escape did not collapse selected event");
  }

  const allRead = page.getByRole("button", { name: "全部已读", exact: true });
  await allRead.click();
  await page.getByText("已全部标为已读").waitFor({ state: "visible" });
  if (await page.locator(".research-event .research-pill").count() !== 0
    || !markReadRequests.some((payload) => Array.isArray(payload.message_ids)
      && payload.message_ids.length >= 2)) {
    throw new Error("all-read did not clear visible unread messages");
  }

  const textarea = page.locator(".research-composer textarea");
  await textarea.fill("验证快捷提交");
  await textarea.press("Control+Enter");
  await page.getByText("研究回答").waitFor({ state: "visible" });

  const citations = page.locator(".research-inline-citation");
  await citations.nth(0).click();
  await citations.nth(1).click();
  const previous = page.getByRole("button", { name: "上一条证据" });
  if (await previous.isDisabled()) throw new Error("citation history did not retain the previous citation");
  await previous.click();
  await page.locator(".research-evidence-card h2").filter({ hasText: "三季报预告上修" }).waitFor({ state: "visible" });

  const deleteButton = page.getByRole("button", { name: /删除研究会话/ }).first();
  await deleteButton.click();
  await page.getByText("确认删除？").waitFor({ state: "visible" });
  await deleteButton.click();
  await page.waitForTimeout(80);
  if (await page.getByText("贵州茅台 研究").count() !== 0) {
    throw new Error("confirmed thread deletion did not remove the session");
  }

  await context.close();
  console.log("News interaction checks passed: expand/escape/all-read/ctrl-enter/delete/citation-history");
}

let localServer = null;
if (serveBuild) {
  const started = await startBuiltAppServer();
  localServer = started.server;
  baseUrl = started.url;
}

const browser = await chromium.launch();
try {
  if (checkOnly || checkSearchOverlay) {
    const device = devices.find((item) => item.name === "desktop-1440");
    const context = await browser.newContext({
      viewport: { width: device.width, height: device.height },
      screen: { width: device.width, height: device.height },
      deviceScaleFactor: device.dpr,
      colorScheme: "dark",
    });
    const page = await context.newPage();
    const consoleIssues = [];
    page.on("console", (message) => {
      if (message.type() === "error") consoleIssues.push(message.text());
    });
    page.on("pageerror", (error) => consoleIssues.push(error.message));
    await installHarnessState(page);
    const shortcutChecks = await runShortcutChecks(page, device);
    const searchOverlayChecks = checkSearchOverlay ? await runSearchOverlayChecks(page, device) : null;
    if (checkSearchOverlay) console.log('Desktop search overlay checks passed: ' + JSON.stringify(searchOverlayChecks));
    const diagnostics = await pageDiagnostics(page, "shortcuts");
    assertPageDiagnostics(diagnostics, device.name);
    if (consoleIssues.length) {
      throw new Error("Desktop shortcut checks logged errors: " + consoleIssues.join(" | "));
    }
    console.log("Desktop shortcut checks passed: " + JSON.stringify(shortcutChecks));
    await context.close();
    await runAgentReplayChecks(browser);
    if (checkNewsInteractions) await runNewsInteractionChecks(browser);
    if (checkNewsPage) {
      mkdirSync(outputRoot, { recursive: true });
      const newsPageBaselines = await captureNewsPageBaselines(browser, outputRoot);
      writeFileSync(
        resolve(outputRoot, "news-page-report.json"),
        JSON.stringify({ baseUrl, generatedAt: new Date().toISOString(), newsPageBaselines }, null, 2) + "\n",
      );
      console.log("News page checks passed: " + newsPageBaselines.length + " captures");
    }
    if (checkBacktestPage) {
      mkdirSync(outputRoot, { recursive: true });
      const backtestPageBaselines = await captureBacktestPageBaselines(browser, outputRoot);
      writeFileSync(
        resolve(outputRoot, "backtest-page-report.json"),
        JSON.stringify(backtestPageReport(backtestPageBaselines), null, 2) + "\n",
      );
      console.log("Backtest page checks passed: " + backtestPageBaselines.length + " captures");
    }
  } else if (backtestPageOnly) {
    mkdirSync(outputRoot, { recursive: true });
    const backtestPageBaselines = await captureBacktestPageBaselines(browser, outputRoot);
    writeFileSync(
      resolve(outputRoot, "backtest-page-report.json"),
      JSON.stringify(backtestPageReport(backtestPageBaselines), null, 2) + "\n",
    );
    console.log("Backtest page screenshots written to " + outputRoot + " (" + backtestPageBaselines.length + " captures)");
  } else if (observeSummaryOnly) {
    mkdirSync(outputRoot, { recursive: true });
    const observeSummaryBaselines = await captureObserveSummaryBaselines(browser, outputRoot);
    writeFileSync(
      resolve(outputRoot, "observe-summary-report.json"),
      JSON.stringify({ baseUrl, generatedAt: new Date().toISOString(), observeSummaryBaselines }, null, 2) + "\n",
    );
    console.log("Observe summary screenshots written to " + outputRoot + " (" + observeSummaryBaselines.length + " captures)");
  } else if (newsPageOnly) {
    mkdirSync(outputRoot, { recursive: true });
    const newsPageBaselines = await captureNewsPageBaselines(browser, outputRoot);
    writeFileSync(
      resolve(outputRoot, "news-page-report.json"),
      JSON.stringify({ baseUrl, generatedAt: new Date().toISOString(), newsPageBaselines }, null, 2) + "\n",
    );
    console.log("News page screenshots written to " + outputRoot + " (" + newsPageBaselines.length + " captures)");
  } else if (headerSettingsOnly) {
    mkdirSync(outputRoot, { recursive: true });
    const headerBaselines = await captureHeaderBaselines(browser, outputRoot);
    writeFileSync(
      resolve(outputRoot, "header-settings-report.json"),
      JSON.stringify({ baseUrl, generatedAt: new Date().toISOString(), headerBaselines }, null, 2) + "\n",
    );
    console.log("Header/settings screenshots written to " + outputRoot);
  } else {
    mkdirSync(outputRoot, { recursive: true });
    const report = {
      baseUrl,
      generatedAt: new Date().toISOString(),
      devices: [],
      denseStates: [],
      headerBaselines: [],
      refreshToolbarBaselines: [],
      newsPageBaselines: [],
    };

    for (const device of devices) {
      const consoleIssues = [];
      const context = await browser.newContext({
        viewport: { width: device.width, height: device.height },
        screen: { width: device.width, height: device.height },
        deviceScaleFactor: device.dpr,
        hasTouch: device.mobile,
        isMobile: device.mobile,
        colorScheme: device.theme,
      });
      const page = await context.newPage();
      await installHarnessState(page);
      page.on("console", (message) => {
        if (message.type() === "warning" || message.type() === "error") {
          consoleIssues.push(message.type() + ": " + message.text());
        }
      });
      page.on("pageerror", (error) => consoleIssues.push("pageerror: " + error.message));
      const deviceReport = { device, routes: [], interaction: null, shortcuts: null, consoleIssues };

      for (const route of routes) {
        await page.goto(deviceUrl(device, route.hash), { waitUntil: "networkidle" });
        await page.locator("#root").waitFor({ state: "visible" });
        if (route.ready) await page.locator(route.ready).waitFor({ state: "visible" });
        if (route.name === "agent" && device.mobile) {
          await page.locator(".agent-workspace.rail-collapsed").waitFor();
          await page.waitForTimeout(260);
        }
        await page.mouse.move(device.width / 2, Math.min(180, device.height / 3));
        await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
        if (route.prepare) await route.prepare(page);
        const directory = resolve(outputRoot, device.name);
        mkdirSync(directory, { recursive: true });
        await page.screenshot({
          path: resolve(directory, route.name + ".png"),
          fullPage: false,
        });
        const diagnostics = await pageDiagnostics(page, route.name);
        assertPageDiagnostics(diagnostics, device.name);
        deviceReport.routes.push({
          ...diagnostics,
          sidebar: await boxFor(page, ".sidebar"),
          workspace: await boxFor(page, ".agent-workspace"),
          rail: await boxFor(page, ".agent-rail"),
          stage: await boxFor(page, ".agent-chat-stage"),
          settings: await boxFor(page, ".llm-settings-body"),
        });
      }

      await page.goto(deviceUrl(device, "#sectionScreen"), { waitUntil: "networkidle" });
      const secondScreenTab = page.locator(".screen-panel-tabs .panel-tab").nth(1);
      await secondScreenTab.click();
      deviceReport.interaction = {
        action: "activate second screen tab",
        activeLabel: await page.locator(".screen-panel-tabs .panel-tab.active").innerText(),
      };

      if (device.name === "desktop-1440") {
        deviceReport.shortcuts = await runShortcutChecks(page, device);
        report.denseStates = await captureDenseStates(page, device, outputRoot);
      }

      report.devices.push(deviceReport);
      await context.close();
    }

    report.headerBaselines = await captureHeaderBaselines(browser, outputRoot);
    report.refreshToolbarBaselines = await captureRefreshToolbarBaselines(browser, outputRoot);
    report.newsPageBaselines = await captureNewsPageBaselines(browser, outputRoot);

    writeFileSync(resolve(outputRoot, "report.json"), JSON.stringify(report, null, 2) + "\n");
    console.log("UI screenshots written to " + outputRoot);
  }
} finally {
  await browser.close();
  if (localServer) {
    await new Promise((resolveClose) => localServer.close(resolveClose));
  }
}
