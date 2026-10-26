# HK market provider readiness

**Checked on:** 2026-10-25
**Decision owner:** release engineering
**Scope:** ordinary-share HK observation and financial enrichment in this application.

This record is an engineering release gate, not a legal opinion. A reachable endpoint, a public web page, or a parser fixture does not grant an automated-access, caching, display, or redistribution right. Until the listed evidence is obtained, fixture-based parsing and read-only local-cache views remain available while the corresponding live refresh is blocked.

## Provider decisions

| Provider / domain | Input and useful fields | Freshness / coverage evidence | Attribution and reuse evidence | Release decision |
| --- | --- | --- | --- | --- |
| HKEX security directory | HKEX `ListOfSecurities.xlsx`; code, name, category, subcategory, board lot, trading currency | Workbook is a point-in-time directory. The response time is recorded as fetch time; it does not verify ordinary-share instrument type. | HKEX Terms of Use restrict programmatic, scripted, or mechanical access and systematic retrieval without express written permission. HKEX Data Licensing offers vendor, end-user, and application-service-provider licence paths. | **Unavailable for live refresh.** Obtain written permission or an applicable licensed-feed agreement. Cached rows stay searchable and display their actual currency. Every legacy/HKEX `Equity` row remains `classification_status=unverified`; it cannot enter observation until a source that distinguishes ordinary shares from preference shares and other equity instruments is licensed and mapped. |
| Tencent HK quote and daily history | `qt.gtimg.cn` quote fields and `web.ifzq.gtimg.cn` daily K-line fields | Quote time and last bar date are retained separately from fetch time. Existing fixtures validate field parsing only. The currently implemented quote adapter supports HKD only. | No provider-specific public terms authorizing this application's automated requests, local caching, or display/redistribution were confirmed for these HK endpoints. | **Unavailable for live refresh.** Require a Tencent agreement or replace it with a provider whose HK quote/history licence covers the intended display and cache behavior. A currency mismatch always stays unavailable. |
| Eastmoney HK financial candidate | `datacenter.eastmoney.com/securities/api/data/v1/get`; candidate financial fields and report period | Candidate fetch time and report period are preserved. Candidate values are not official values and remain unverified unless independently matched field-by-field. | The current Eastmoney user agreement says some market data have product/region limits and prohibits copying or providing market data without prior written exchange consent. No agreement for this endpoint and this application's use was confirmed. | **Unavailable for live refresh.** Obtain written provider/exchange permission for automated access, caching, and the intended display. Offline parsing fixtures remain enabled. |
| HKEXnews issuer metadata | HKEXnews issuer-title search; exact issuer-match metadata and official disclosure entry URL | Query/check time is stored separately from an issuer's disclosed-at date. Search metadata does not verify financial values. | HKEX Terms of Use apply to HKEX websites and restrict automated access/systematic retrieval without express written permission. | **Unavailable for live refresh.** Obtain written permission or a licensed service agreement. Do not treat metadata as parsed or verified financial content. |
| Frankfurter, pinned to ECB | `api.frankfurter.dev/v2/providers/ecb/rate/HKD/CNY`; daily HKD/CNY cross-rate calculated from the selected provider's reference-rate series | Daily reference observation; the response's `date` is displayed as the observation date. It is not an intraday/trading rate. | Frankfurter documents direct provider queries, daily rates, commercial use of its own blended output, local caching, and that direct provider data remain subject to that provider's terms. ECB permits free reuse of its published information if it is reproduced accurately and cited; modifications must be disclosed. | **Available with limits.** Pin the endpoint to `ecb`, display “European Central Bank (via Frankfurter)” and the actual observation date, cache the daily value, and disclose that the HKD/CNY cross is derived and is not live-market FX. |

## Release rules

1. `ensure_hk_provider_ready` is the single live-request gate for HKEX, Tencent, Eastmoney HK, and HKEXnews. The gate must cover direct refresh commands and foreground recovery paths, not only the visible UI action.
2. Capability status uses both provider readiness and data evidence. Existing cache records cannot open a closed provider gate. A cache-backed capability is at most `limited`; no usable records means `unavailable`.
3. `classification_status` defaults to `unverified` when loading older cache files. Do not convert HKEX's `Equity` category or the subcategory text into `ordinary_share` evidence.
4. A provider can be enabled only after this record includes the signed/official permission reference, authorized purpose, allowed fields, cache duration, display/redistribution scope, rate limits, required attribution, and review date. Update the integration tests and release gate in the same change.
5. Real-network verification is a separately authorized operational check. Automated tests use fixtures and never contact these endpoints.

## Official references

- [HKEX Terms of Use](https://www.hkex.com.hk/Global/Exchange/Terms-of-Use?sc_lang=en)
- [HKEX Data Licensing](https://www.hkex.com.hk/Services/Market-Data-Services/Real-Time-Data-Services/Data-Licensing?sc_lang=en)
- [HKEX Market Data FAQ](https://www.hkex.com.hk/Global/Exchange/FAQ/Market-Data?sc_lang=en)
- [Eastmoney User Service Agreement](https://about.eastmoney.com/home/protocol)
- [Frankfurter API documentation and FAQ](https://frankfurter.dev/)
- [Frankfurter licence notes](https://frankfurter.dev/license/)
- [ECB disclaimer and copyright](https://www.ecb.europa.eu/services/using-our-site/disclaimer/html/index.en.html)
- [ECB statistical reuse policy](https://www.ecb.europa.eu/stats/ecb_statistics/governance_and_quality_framework/html/usage_policy.en.html)
- Tencent HK endpoint-specific usage terms: **not located/confirmed as of 2026-10-25**. This is an unresolved release gate, not evidence that the endpoint is either permitted or prohibited.
