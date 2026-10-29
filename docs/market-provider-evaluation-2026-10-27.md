# A/HK market-provider evaluation

**Checked:** 2026-10-29
**Target:** publicly distributed Windows and Android app; end-user BYOK; device-local cache and display
**Decision:** no reviewed provider is approved for production refresh or public-app data display yet.

This is an engineering evidence register, not legal advice. “API documented” means a first-party API reference describes an operation; it does not grant permission to embed that API in this app, cache or display its results, publish derived analytics, or send records to an external model. No provider gate should open without an agreement that names the app and intended user flow.

## A-share-first candidates

| Candidate | Documented data/API coverage | Account, cost and coverage limits | Public-app rights evidence | Decision and missing evidence |
| --- | --- | --- | --- | --- |
| TuShare Pro | First-party API docs list mainland security master, daily quotes, daily basic indicators and financial-statement endpoints. | API access is permission/points-gated and quotas depend on account tier; verified teachers and students receive special 2,000/5,000-point access. The endpoint catalog does not prove delisted-universe completeness, historical point-in-time availability or news/RAG rights. | The service agreement grants a personal, non-transferable, non-commercial, revocable licence for personal viewing and prohibits account sharing. It does not expressly cover this public client's device cache, derived research display or model processing. A user token alone is insufficient evidence for those uses. | **Evaluation only.** Ask TuShare for written confirmation covering this app, individually authorized accounts, local cache/display, derived analytics and any RAG/LLM processing; verify required history and PIT fields. |
| BaoStock | First-party docs describe A-share security basics (including IPO/out dates and listing status), historical/index K-lines, adjustment factors and financial-report endpoints; the service describes itself as free. | Published API access rules include IP restrictions and a 50,000-request daily ceiling. These operations do not prove a complete historical universe, point-in-time financial availability, corporate-action-adjusted total returns or news coverage. | No first-party terms located that clearly grant a publicly distributed third-party app the requested automated access, device cache, display, derived-analytics and model-processing rights. | **Evaluation only.** Obtain written terms for this app and verify domain coverage, retention and freshness before implementation. |
| AKShare / AKTools | Open-source Python library and optional HTTP service expose numerous A-share market, financial, index and news collectors backed by different upstream sites. | Coverage, schema, freshness, authentication and quotas vary by upstream endpoint; the software license does not establish rights to the underlying datasets. Python packaging also does not itself provide the required native Windows/Android user-account flow. | Project guidance limits data use to academic research and places other use at the user's commercial risk; individual upstream site terms and automated-access rights still apply. No unified permission for this public app's caching, display, derived analytics or RAG/LLM processing was found. | **Not approved.** Do not treat the library's open-source license or a reachable upstream endpoint as permission to embed or redistribute data. |
| Moomoo OpenAPI | First-party docs expose quote permissions, security information, historical K-lines, screening and financial/corporate-action APIs for supported market/security codes. | Broker account and per-market quote entitlements apply; real-time permissions and historical quotas vary by market/account. API examples and symbol lookup do not prove a complete delisted universe or PIT financial-publication timestamps. | Moomoo US Terms of Use limit Market Data to personal use and prohibit disseminating or transmitting it to any person; the API docs do not grant this separate public client rights to cache/display derived data or send it to a model. | **Evaluation only.** Obtain written provider/exchange permission for this public client, per-user BYOK, local cache, screen display, derived analytics and any RAG/LLM processing. Verify mainland exchange coverage, delisted securities, corporate actions, security-status history and publication-available dates. |
| Longbridge OpenAPI | First-party docs expose A-share quotes/candles, security information, fundamentals, reports, corporate actions and security news. | Quote access is entitlement-dependent. Its candlestick endpoint lists HK LV1 as included by default, a 100–3,000 unique-symbol monthly quota by account tier, and a 1,000-bar limit per request; longer history uses a separate endpoint. These are API limits, not full-market PIT evidence. | Longbridge US terms prohibit copying/distribution/commercial exploitation of provider information without written consent and state that market data may not be redistributed, published or commercially used without separate authorization. | **Evaluation only.** Obtain written permission for this public app, per-user BYOK, device cache/display, derived data, news excerpts and external model processing; confirm A-share data entitlement and historical PIT/delisted coverage. |
| SSE/SZSE or an exchange-authorized distributor | Exchanges publish official market information and provide product/licensing channels for market data. SSE directs market institutions to its market-data product area for product information, authorization, fees and applications; SZSE exposes an “行情信息授权” service path. | Product, frequency, fields, history, retention and eligible user class depend on the selected product or distributor agreement. | This repository has no signed SSE/SZSE or distributor agreement that covers this app's public client, cache, display, analytics or RAG/LLM processing. Public exchange pages are not treated as an API license. | **License path required.** Request written terms for application-service distribution or end-user display, fields, cache duration, derived analytics and AI processing. |
| Existing anonymous A-share endpoints in this app (Tencent, Eastmoney, Sina, THS, Baidu, CNINFO) | Current code contains working request builders/parsers for quotes, history, finance supplements, capital evidence, news and announcements. | Coverage, freshness and endpoint behavior differ by domain; some endpoints are undocumented, field-index based, or require source-specific headers. Current caches do not provide a complete per-security license/PIT ledger. | The repository has no signed provider or exchange agreement covering this app's public distribution, cache retention, display, derived analytics and RAG/LLM transfer for these endpoints. | **Not approved for a full-capability release.** Keep them identified as legacy technical integrations; attach rights evidence and domain-specific coverage before any capability can be marked release-ready. |

## Hong Kong candidates

| Candidate | Documented coverage | Key data gaps | Public-app rights evidence | Decision |
| --- | --- | --- | --- | --- |
| Longbridge OpenAPI | HK quotes, daily candles, financial reports, corporate actions and security news are documented. HK LV1 availability is account/product-specific. | Per-symbol security metadata is not proof of a complete ordinary-share directory. Financial periods do not, by themselves, establish public-availability dates; delisted universe and corporate-action adjusted history must be verified. | User agreement says market data may not be redistributed, published or commercially used without separate authorization. | **Evaluation only; no adapter is enabled.** Written app/client rights and ordinary-share/PIT coverage are required. |
| Moomoo OpenAPI | HK quotes, historical K-lines, financial statements, company information and corporate-action methods are documented. | Quote rights vary by account/market. Generic stock type is not sufficient evidence of ordinary-share classification. PIT disclosure/publication dates and complete delisted history are unverified. | Personal-use and no-redistribution language applies to market data unless written permission is obtained. | **Evaluation only; no adapter is enabled.** Written display/cache/model rights and authoritative ordinary-share evidence are required. |
| HKEX / HKEXnews | Official securities list, licensed market-data products and issuer disclosure entry points. | The directory's generic `Equity` category does not establish ordinary-share classification. Directory metadata is not parsed verified financial data; an EOD product is not a complete quote, finance, news and RAG stack. | HKEX website terms restrict scraping, systematic retrieval, third-party display and text/data mining absent express written permission; product-specific licenses remain separate. | **Unavailable pending the exact written license/permission and ordinary-share mapping.** See the detailed [HK readiness record](hk-market-provider-readiness.md). |

## Release gates by data domain

| Domain | Minimum evidence before `available` | Current A-share state |
| --- | --- | --- |
| Security directory | Exchange-universe scope, instrument/listing status, effective dates, attribution and public display/cache rights. | Legacy stock-pool cache exists; complete universe and rights are unverified. |
| Quote and K-line | Instrument coverage, market session timestamps, currency/units, freshness, adjustment policy, cache/display rights and quotas. | Existing direct adapters and caches work; rights and per-security freshness evidence are incomplete. |
| Financial snapshot | Field-level definitions/units, fiscal period, actual public availability/disclosure time, restatement handling, cache/display rights. | Cached financial fields exist for a subset; historical public-availability dates are not proven. |
| Screening | Complete, current factor coverage for the requested universe and explicit source/field quality. | Existing A-share strategies remain in place; registry now reports measured cache coverage and missing evidence. |
| Backtest | Point-in-time factors, delisted universe, corporate actions, exchange calendar, suspension/limit rules, fees/board lots and enough strict out-of-sample folds. | The engine exists; current provider/cache records do not establish all publication-time, delisted and corporate-action evidence. |
| News, filings and RAG | Retrieval rights, excerpt display, cache/retention, indexing/embedding, source attribution and explicit permission for external-model transmission. | Existing public-site collectors are technical integrations; reuse rights for public-app RAG are unverified. |

## Operating rules

1. Do not add a provider just because its API has an endpoint or the user's brokerage account can authenticate.
2. Keep provider credentials and OAuth refresh tokens in the native OS vault; never put them in WebView storage, backups, diagnostics, URLs, logs or model prompts.
3. Use fixtures for tests. A fixture, account token, or populated cache never opens a production rights gate.
4. For every approved provider, record the agreement reference, approved app/client, market and account region, purpose, fields, freshness, quota, cache duration, attribution, display/redistribution, derived analytics, model-processing scope, and review date.
5. Until the required evidence exists, return the real cache state with explicit missing rights/coverage fields. Do not advertise a full-capability A-share release.

## First-party references

- [Moomoo OpenAPI permissions, quotas and supported markets](https://open.moomoo.com/api/overview/api-authority)
- [Moomoo OpenAPI fees and market-data subscriptions](https://open.moomoo.com/api/overview/api-fee)
- [Moomoo US Terms of Use, Market Data provisions](https://www.moomoo.com/terms?global_content=%7B%22promote_id%22%3A13643%2C%22sub_promote_id%22%3A4%7D)
- [TuShare Pro API documentation](https://tushare.pro/document/2)
- [TuShare Pro data-service agreement](https://tushare.pro/document/1?doc_id=405)
- [BaoStock API documentation](https://www.baostock.com/mainContent?file=pythonAPI.md)
- [BaoStock official site and API knowledge base](https://www.baostock.com/)
- [BaoStock security basics and delisting fields](https://www.baostock.com/mainContent?file=stockBasic.md)
- [BaoStock API access and IP limits](https://www.baostock.com/blacklist)
- [AKShare documentation](https://akshare.akfamily.xyz/)
- [AKShare source repository and project guidance](https://github.com/akfamily/akshare)
- [Longbridge API overview](https://open.longbridge.com/docs)
- [Longbridge quote API overview](https://open.longbridge.com/docs/quote/overview)
- [Longbridge daily candlestick API and quotas](https://open.longbridge.com/docs/quote/pull/candlestick)
- [Longbridge fundamental API overview](https://open.longbridge.com/docs/fundamental/overview)
- [Longbridge security news API](https://open.longbridge.com/docs/content/news)
- [Longbridge pricing and quote entitlements](https://open.longbridge.com/pricing)
- [Longbridge US Terms of Service](https://longbridge.com/us/en/support/articles/14864594652431)
- [SSE market-data information, authorization, fees and application channel](https://www.sse.com.cn/transparency/services/index.shtml)
- [SZSE market-data authorization entry](https://www.szse.cn/)
- [SZSE technical-services/product documentation](https://www.szse.cn/marketServices/technicalservice/)
- [HKEX Terms of Use](https://www.hkex.com.hk/global/exchange/terms-of-use?sc_lang=en)
- [HKEX Data Licensing](https://www.hkex.com.hk/Services/Market-Data-Services/Real-Time-Data-Services/Data-Licensing?sc_lang=en)
- [User-provided `a-stock-data` source repository](https://github.com/simonlin1212/a-stock-data) — technical endpoint reference only; its README claims are not provider permission or this app's coverage evidence.
