import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { MarketCapability } from "../../lib/marketDomain";
import { MarketCapabilityStatus } from "./MarketCapabilityStatus";

describe("MarketCapabilityStatus", () => {
  it("translates A-share history and strict-backtest gaps without exposing internal field names", () => {
    const capability: MarketCapability = {market:"cn",id:"backtest",status:"limited",reason:null,
      coverage:{requested_count:5579,available_count:378,missing_count:5201,start_date:null,end_date:null,
        missing_fields:["history_freshness_per_security","daily_history","minimum_history_depth","point_in_time_financial_availability","delisted_universe","corporate_actions","exchange_calendar_and_trading_restrictions","strict_out_of_sample_fold_count"]},
      sources:[{source_id:"legacy_market_cache",domain:"daily_history",market:"cn",fetched_at:null,observed_at:null,disclosed_at:null,report_period:null,freshness:"unknown",verification_status:"unverified",quality_flags:["per_security_freshness_unknown"],coverage:null}]};
    const html=renderToStaticMarkup(<MarketCapabilityStatus label="A 股回测" capability={capability} />);
    expect(html).toContain("缺少日线历史");
    expect(html).toContain("逐证券历史日期待核对");
    expect(html).toContain("缺少点时财报可用日期");
    expect(html).toContain("缺少历史退市名单");
    expect(html).toContain("缺少公司行动记录");
    expect(html).not.toContain("point in time financial availability");
    expect(html).not.toContain("per security freshness unknown");
  });
  it("explains checked A-share sources and genuinely unavailable report metrics in Chinese", () => {
    const capability: MarketCapability = {market:"cn",id:"financial_snapshot",status:"limited",reason:null,
      coverage:{requested_count:5579,available_count:5578,missing_count:1,start_date:null,end_date:null,missing_fields:["per_security_financial_field_coverage"]},
      sources:[{source_id:"eastmoney_cn_financial",domain:"financial_snapshot",market:"cn",fetched_at:"1794103200000",observed_at:null,disclosed_at:"2026-10-30",report_period:"2026Q3",freshness:"fresh",verification_status:"source_reported",quality_flags:["reported_eps_missing","roe_not_applicable_non_positive_equity"],coverage:null},
        {source_id:"sse_szse_listed_bse_quote",domain:"security_master",market:"cn",fetched_at:"1794103200000",observed_at:null,disclosed_at:null,report_period:null,freshness:"fresh",verification_status:"scope_checked",quality_flags:["bse_quote_directory_scope"],coverage:null}]};
    const html=renderToStaticMarkup(<MarketCapabilityStatus label="A 股财务" capability={capability} />);
    expect(html).toContain("东方财富定期报告");
    expect(html).toContain("来源已核对");
    expect(html).toContain("沪深交易所与北交所行情目录");
    expect(html).toContain("净资产为负，ROE 不适用");
    expect(html).toContain("来源未提供部分 EPS");
    expect(html).toContain("报告期：2026Q3");
    expect(html).not.toContain("reported eps missing");
  });
  it("keeps technical evidence collapsed behind a readable summary", () => {
    const capability: MarketCapability = {
      market: "cn",
      id: "observe",
      status: "limited",
      reason: "A-share observation uses cached quote/history; provider rights and per-security history freshness are not evidenced",
      coverage: null,
      sources: [{
        source_id: "tencent",
        domain: "quote",
        market: "cn",
        fetched_at: String(Date.parse("2026-10-27T08:00:00Z")),
        observed_at: "2026-10-27",
        disclosed_at: null,
        report_period: null,
        freshness: "fresh",
        verification_status: "unverified",
        quality_flags: ["provider_rights_unverified"],
        coverage: null,
      }],
    };

    const html = renderToStaticMarkup(
      <MarketCapabilityStatus label="A 股观察" capability={capability} />,
    );

    expect(html).toContain("数据依据 · 1 个来源");
    expect(html).not.toContain("<details open=");
    expect(html).toContain("来源：腾讯行情");
    expect(html).toContain("核验：未核验");
    expect(html).toContain("授权待确认");
    expect(html).not.toContain("质量标记：provider_rights_unverified");
    expect(html).toContain("数据域：行情");
    expect(html).toContain("抓取：2026-10-27 08:00 UTC");
    expect(html).toContain("A 股观察显示本地行情和历史数据；数据授权与逐证券历史新鲜度尚未核验。");
  });

  it("shows compact coverage and keeps missing evidence out of the primary line", () => {
    const capability: MarketCapability = {
      market: "cn",
      id: "financial_snapshot",
      status: "limited",
      reason: "Cached financial snapshots exist for only part of the universe; provider rights and point-in-time publication dates are not evidenced",
      coverage: {
        requested_count: 5927,
        available_count: 4719,
        missing_count: 1208,
        start_date: null,
        end_date: null,
        missing_fields: ["provider_rights", "point_in_time_publication_date_missing"],
      },
      sources: [],
    };

    const html = renderToStaticMarkup(
      <MarketCapabilityStatus label="A 股财务" capability={capability} />,
    );

    expect(html).toContain("覆盖 4719/5927 · 待补齐 1208 条");
    expect(html).toContain("查看缺失项 · 2 项");
    expect(html).toContain("数据授权待确认");
    expect(html).not.toContain("缺失证据：provider_rights");
  });

  it("keeps raw capability errors behind a short user-facing message", () => {
    const html = renderToStaticMarkup(
      <MarketCapabilityStatus
        label="A 股观察"
        error={'Unexpected token \'<\', "<!doctype" is not valid JSON'}
      />,
    );

    expect(html).toContain("能力状态暂时无法读取，相关数据状态未知。");
    expect(html).toContain("<details class=\"market-capability-details\"><summary>错误详情</summary>");
    expect(html).toContain("Unexpected token");
  });

  it("shows HK quote gaps and source evidence in Chinese", () => {
    const capability: MarketCapability = {
      market: "hk", id: "screening", status: "limited",
      reason: "required data coverage is incomplete",
      coverage: { requested_count: 3, available_count: 2, missing_count: 1, start_date: null, end_date: null, missing_fields: ["currency_matched_cached_quote"] },
      sources: [{
        source_id: "hk_market_cache", domain: "quote", market: "hk",
        fetched_at: "2026-11-07T08:00:00Z", observed_at: "2026-11-06", disclosed_at: null, report_period: null,
        freshness: "fresh", verification_status: "unverified",
        quality_flags: ["ordinary_share_user_confirmed_records_present", "currency_matched"], coverage: null,
      }],
    };
    const html = renderToStaticMarkup(<MarketCapabilityStatus label="港股筛选" capability={capability} />);
    expect(html).toContain("待补齐 1 条");
    expect(html).toContain("所需数据尚未全部覆盖");
    expect(html).toContain("有效港币行情");
    expect(html).toContain("港股本地缓存");
    expect(html).toContain("报价币种与证券一致");
    expect(html).not.toContain("required data coverage");
    expect(html).not.toContain("currency matched");
    expect(html).not.toContain("ordinary share user confirmed");
  });

  it("shows excluded currency counters separately from missing quotes", () => {
    const capability: MarketCapability = {
      market: "hk", id: "screening", status: "available", reason: null,
      coverage: { requested_count: 2756, available_count: 2756, missing_count: 0, start_date: null, end_date: null, missing_fields: [] },
      quote_scope: { currency: "HKD", excluded_count: 24 },
    };
    const html = renderToStaticMarkup(<MarketCapabilityStatus label="港股筛选" capability={capability} />);
    expect(html).toContain("港股筛选 · 可用");
    expect(html).toContain("覆盖 2756/2756 · 已覆盖");
    expect(html).toContain("港币（HKD）");
    expect(html).toContain("24 个其他币种交易柜台暂未纳入");
    expect(html).not.toContain("待补齐");
    expect(html).not.toContain("查看缺失项");
  });
});
