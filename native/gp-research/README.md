# gp-research

Independent Rust 2021 library (`stock-research`, import name `gp_research`) for offline mainland A-share point-in-time research. Rust 1.99 is required. It uses the public `gp_core` feature/scoring APIs; it has no Python, provider API, or ML runtime. Default features are empty; `cli` enables the `stock-research` binary and is unnecessary for Android/app library consumers.

The checked-in [tiny bundle](tests/fixtures/tiny/manifest.json) contains **synthetic data**, 90 weekday sessions, ten equities, and two indices. Its Gregorian weekday calendar is synthetic, not an authoritative China exchange calendar. `real_data=false` is explicit. It cannot support calibration promotion or real investment conclusions.

## API

All models are serde public exports from `lib.rs`; dates are `chrono::NaiveDate`, serialized `YYYY-MM-DD`. Errors are `ResearchError { code, message, details }`, returned through `ResearchResult<T>`.

```rust
ResearchStore::open(app_data_dir: &Path) -> ResearchResult<ResearchStore>
ResearchStore::open_readonly(app_data_dir: &Path) -> ResearchResult<ResearchStore>
store.import_bundle(bundle_dir: &Path) -> ResearchResult<DatasetRecord>
store.dataset(dataset_id: &str) -> ResearchResult<DatasetReader>
store.datasets() -> ResearchResult<Vec<DatasetRecord>>
store.save_report(kind: &str, report: &serde_json::Value) -> ResearchResult<String>
store.reports() -> ResearchResult<Vec<RunSummary>>
store.report(run_id: &str) -> ResearchResult<serde_json::Value>
store.root() -> &Path
reader.record() -> &DatasetRecord
reader.calendar() -> &[NaiveDate]
reader.validate(config: &EvaluationConfig) -> ResearchResult<QualityReport>
reader.features_on(signal_date: NaiveDate, config: &EvaluationConfig)
    -> ResearchResult<gp_core::AdaptiveFeatureSet>
reader.screen_on(signal_date: NaiveDate, request: &gp_core::AdaptiveScreenRequest,
    parameters: &gp_core::ScoringParameters) -> ResearchResult<gp_core::AdaptiveScreenResult>
reader.forward_return(code: &str, signal_date: NaiveDate, horizon: usize)
    -> ResearchResult<Option<f64>>
evaluate(reader: &DatasetReader, parameters: &gp_core::ScoringParameters,
    config: &EvaluationConfig) -> ResearchResult<EvaluationReport>
research_implementation_fingerprint() -> &'static str
store.resolve_parameters(requested: Option<&str>) -> ResearchResult<ResolvedParameters>
store.research_parameters(id: &str) -> ResearchResult<ResolvedParameters>
store.register_parameters(parameters: &gp_core::ScoringParameters, dataset_id: &str)
    -> ResearchResult<()>
store.qualification_verified(run_id: &str) -> ResearchResult<bool>
store.promote(run_id: &str) -> ResearchResult<ResolvedParameters>
store.rollback(parameter_set_id: &str) -> ResearchResult<ResolvedParameters>
calibrate(store: &ResearchStore, dataset_id: &str, options: &CalibrationOptions)
    -> ResearchResult<CalibrationReport>
compute_split(calendar: &[NaiveDate], end_date: Option<NaiveDate>)
    -> ResearchResult<CalibrationSplit>
spearman(x: &[f64], y: &[f64]) -> Option<f64>
bootstrap_lower_bound(monthly_excess: &[f64]) -> Option<f64>
release_gate(evidence: &GateEvidence) -> GateReport
feasible_neighbors(parameters: &gp_core::ScoringParameters, modes: &[String])
    -> ResearchResult<Vec<gp_core::ScoringParameters>>
```

`screen_on` preserves criteria, requested primary/exploration limits and mode, and uses the authoritative PIT financial/identity/price basis. An explicit request as-of date must match the signal session (canonical or compact date). The returned regime records the canonical signal date. Core criteria are applied through `adaptive_candidate_codes` before requesting the bounded stage-one histories (80 codes for this scoring revision). Market breadth still uses the whole supplied historical equity universe. Parameters are validated against the actual core implementation.

`EvaluationConfig` defaults to 2021-10-01 through 2026-09-30, initial cash 1,000,000 CNY, 10 bps costs, top 10, monthly rebalancing, `auto`, `adaptive`. Modes are the core modes `auto`, `range`, `trend`, `defensive`, `transition`; explicit modes support focused synthetic tests. Strategies are `adaptive` and `legacy_balanced`. Monthly is the only supported frequency. Short/manual-mode evaluations never provide release qualification.

`criteria` is serde-defaulted `gp_core::ScreenCriteria`; actual research filters are passed through stage-one features and selection. Global quality/action/history auditing remains unfiltered. `signal_cutoff` defaults to `None`; calibration validation sets it to the last unpurged signal. Subsequent months retain portfolios and accounting but place no new orders/query no new forward labels. Canonical test configurations require default criteria and no signal cutoff. Old v1 report JSON without these fields still reads.

`DatasetRecord` carries immutable identity, source and flags, date bounds and row counts. `QualityReport` carries per-session coverage, exact missing observations, and reasons. Missing slow data at or below the allowed 5% is still recorded without inventing values. `EvaluationReport` includes core/research/parameter/data fingerprints, parameters/config, quality, metrics, monthly folds, curves, actual/failed trades, and locked-position exposures. Curves are `{date,equity}`. `PortfolioMetrics` includes total/annualized return, nonnegative drawdown, annualized traded-notional/NAV turnover and actual costs. Precision is the fraction of selected symbols with positive matured 20-session total returns; any unavailable label makes the fold precision null. `RunSummary` holds report identity, dataset, kind/status, and optional parameter-set id.

## Exact gp-pit/v1 bundle

Place `manifest.json` and six header CSV files in a single directory. Every declared path must be a plain relative path resolving inside that directory, including symlink resolution. Every file must declare the lowercase SHA-256 of its exact bytes. The dataset fingerprint hashes the exact manifest bytes and all verified role/file hashes; changed files or manifest bytes create a different dataset. Identical import is idempotent. No content is obtained from the network.

```json
{
  "format": "gp-pit/v1",
  "name": "My source bundle",
  "market": "cn",
  "timezone": "Asia/Shanghai",
  "source": "Nonempty source/version/provenance description",
  "real_data": false,
  "full_universe": true,
  "coverage": {
    "start": "2025-11-03",
    "end": "2026-03-06",
    "corporate_actions_complete": true
  },
  "rate_units": {
    "roe": "percentage_points",
    "dividend_yield": "percentage_points",
    "deducted_net_profit_growth_rate": "percentage_points",
    "deducted_net_profit_margin": "percentage_points",
    "turnover_rate": "percentage_points"
  },
  "money_unit": "CNY_100m",
  "files": [
    {"role":"securities", "path":"securities.csv", "sha256":"<64 lowercase hex characters>"},
    {"role":"daily_bars", "path":"daily_bars.csv", "sha256":"<64 lowercase hex characters>"},
    {"role":"financial_events", "path":"financial_events.csv", "sha256":"<64 lowercase hex characters>"},
    {"role":"daily_status", "path":"daily_status.csv", "sha256":"<64 lowercase hex characters>"},
    {"role":"corporate_actions", "path":"corporate_actions.csv", "sha256":"<64 lowercase hex characters>"},
    {"role":"trading_calendar", "path":"trading_calendar.csv", "sha256":"<64 lowercase hex characters>"}
  ]
}
```

`real_data` alone defaults to false. All other manifest fields above are required. Coverage bounds must exactly match the first/last calendar session. Calendar dates are sorted and duplicate dates rejected. Supply real exchange sessions for real data. `full_universe=true` is a provenance declaration, not proof: structural listing/status/quote coverage is independently checked. Auto rejects an explicitly partial universe.

Rates require an explicit `ratio` or `percentage_points` unit for each nonempty rate column. No magnitude inference is used. Internally ROE, dividend yield and turnover use ratios; profit margin/growth retain the core wire percentage-point convention. `money_unit` scales `deducted_net_profit_billion`: CNY is divided by 1e8; CNY_100m is already in the core wire unit. EPS, BPS, OHLC and cash-per-share are CNY/share; bar `amount` is CNY, volume/shares are share counts. Those quote/share units do not change with the financial profit unit.

All listed headers below are required even when a file has zero rows. Optional values are empty CSV cells. Required identity/time/boolean/revision fields cannot be empty or inferred. OHLC must be finite, positive and consistent; supplied volume/amount/share/liquidity values must be finite and nonnegative. Conflicting duplicates are rejected; exact duplicate observations are harmless except calendar dates, which must be unique.

```csv
# securities.csv
code,name,security_type,listed_date,delisted_date
# daily_bars.csv
code,date,open,high,low,close,volume,amount,total_shares,float_shares,turnover_rate,volume_ratio
# financial_events.csv
code,report_date,available_at,revision,source,eps_ttm,bps,roe,deducted_net_profit_billion,deducted_net_profit_growth_rate,deducted_net_profit_margin,dividend_yield
# daily_status.csv
code,date,name,industry,is_listed,is_st,can_buy,can_sell
# corporate_actions.csv
code,event_id,kind,known_at,ex_date,pay_date,cash_per_share,share_multiplier
# trading_calendar.csv
date
```

The comment lines above identify files and are not part of CSV contents. `security_type` is `equity` or `index`; codes must be six digits plus `.SH`, `.SZ`, or `.BJ`. Index rows are excluded from equity candidates. `delisted_date` is the **first unlisted date**. Bars before listing or on/after delisting are rejected; statuses are required for every session in the listed interval, including suspended names. Supply historical name, industry, ST and buy/sell permissions for each date. There is no current-status or stale execution-price backfill.

Financial event identity is `(code, report_date, revision)`. A revision has its own `available_at` and only applies once visible. The most recent visible report period, then disclosure session/revision, determines the current financial snapshot. `available_at` and `known_at` accept a date or RFC3339 timestamp with explicit offset. Date-only enters on the strictly next calendar session. Timed observations are converted to UTC+08:00: at/before 15:00 enters that session; later or a non-session enters the next one. No next calendar session means it remains invisible inside this dataset. A report date cannot follow its disclosure date.

Actions have identity `(code,event_id)` and kinds `cash_dividend`, `share_split`, `delisting_settlement`, or `unsupported`. Cash/settlement needs explicit nonnegative `cash_per_share`; **zero settlement is valid evidence of total loss**. Split needs a positive `share_multiplier`. Ex-date must be a calendar session, and pay-date cannot precede it. Delisting requires settlement on the first unlisted date. Screening checks all known/effective action and required settlement evidence across the dataset before any industry filter; future disclosures do not enter that as-of check. Evaluation additionally checks realized outcomes. Unsupported/missing actions fail strict quality; a header-only action file requires declared complete source coverage. On a shared ex-date, dividend/settlement cash is per shares held before that day's transformations; split and cash adjustment is grouped by date, independent of event-id order. Total-return labels and portfolios use realized action outcomes; feature adjustment uses only then-known and effective actions.

## Strict evaluation and storage

Only the calendar is retained in full. Price histories are indexed per-code queries bounded to the last 120 calendar sessions at or before the signal, with at most 120 actual bars; each core factor candidate needs at least 60 valid bars. Suspended sessions may legitimately have no bar, and IPO histories begin at listing. Financial/status queries use signal-date cutoffs. Stock PE/PB use positive disclosed EPS/BPS and the signal close; cap uses that day's total shares divided by 1e8. Financial gaps remain unavailable.

Quality checks the configured period plus required preceding status/price warmup. Each then-tradable equity needs its exact bar and every listed equity its exact historical status. Each signal session needs at least 95% usable slow-factor data and sufficient core technical coverage. Auto additionally needs two of 000001.SH/399001.SZ/399006.SZ with exact session bars and valid histories, plus core breadth coverage. `evaluate` errors before producing a result if strict validation fails.

Ordinary `screen_on` audits the same structural session evidence across its bounded as-of warmup before factors or ranking, globally even when criteria exclude a security. Only the signal day's slow-factor coverage is required; future disclosures, session gaps and realized actions do not participate in that screen. Auto requires exact bars for every declared supported index in this window.

Implementation identity uses source-text canonicalization: core/research domain `v2-lf`, native release contract `v4-lf`. CRLF pairs become LF before named-file length framing and SHA-256; all other bytes, file order and substantive edits remain significant. The shared core helper is itself covered, as are manifests and crate lockfiles. Dataset imports, parameter signatures and exact persisted report/receipt bytes retain their existing exact-byte protocols. Prior implementation fingerprints remain stale and fail closed; this change does not migrate, requalify or unlock existing evidence. Rebuilding the same implementation with another checkout newline convention preserves the new identity.

The ordinary native backtest boundary validates explicit identity metadata and rejects raw scoring weights before legacy dispatch. Adaptive strategy names use the core's trim/lowercase convention; exact version/mode and frozen nested screen metadata must agree. Qualified v2 resolution remains registry/receipt-owned and bound to its frozen PIT dataset. Unannotated candidate snapshots and walk-forward requests keep the legacy path.

The first execution uses the final signal session before the configured start. Subsequent executions use the first eligible session of each month and the preceding session's factors. Orders use the actual execution close, after company actions and payment settlement. Previously held shares earn ex-date cash receivables; receivables enter NAV immediately but become spendable only at payment. Splits transform share count. Buy restrictions leave intended cash; sell restrictions retain holdings and exposure. A missing execution bar never reuses the last price. Holding valuation may retain a prior price while recording the position as locked. When an ex-date cash dividend has no exact bar, that retained mark is reduced in pre-action share units before split transforms and the receivable is added; a later exact raw quote replaces the mark without another discount. Costs apply to actual notional; post-cost equal weights are solved against sell-locked exposure floors and permitted trade flows. Buy-blocked slots reserve intended cash. Unfunded or partly funded orders record the unfilled intent as execution failures. If a buy has no positive exact execution quote or no representable share quantity, its failed record uses shares=0 as explicitly unavailable, price=None when no exact quote exists, notional as the intended cash amount, cost=0, and an explicit reason; cash remains reserved. For executed records notional is actual traded notional. Retained valuation marks never become execution quotes. Shares/cash/receivables and NAV carry continuously across folds.

The candidate equal-weight benchmark is rebuilt from the identical PIT core feature candidate pool at each signal and uses the same action, raw-price, permission and cost simulation. The legacy balanced auxiliary ranks the complete eligible historical core candidate pool before enforcing the industry cap and selecting top N. Fold completeness requires the complete calendar month and source coverage evidence; partial months remain incomplete. A fully lost portfolio has subsequent zero returns rather than NaNs. Successful evaluation reports check finite trade, exposure, curve, fold and metric values before serialization; computed overflow returns a precise numeric error instead of silently serializing null. The simulator uses fractional shares and proportional close orders; it does not model exchange lots, market impact or slippage beyond configured notional costs.

Forward horizons are 10/20/30 sessions: entry on the next session close; exit at entry index + horizon. Realized split/dividend/delisting outcomes are included. A suspended endpoint with an actual bar counts; absent or unmatured endpoint returns `None`, without using a stale quote. Delisting settlement can supply the terminal evidence even when no post-delisting bar exists.

Storage is `<app-data>/stock-research/research.sqlite` for registry/reports and `<app-data>/stock-research/datasets/<fingerprint>/dataset.sqlite` for bulk datasets. Immutable bulk schema remains v1. Writable registry opens migrate legacy v1 to registry v2 (`application_id=1196446258`), with datasets/reports, parameter_sets, calibration_receipts, test_locks, qualifications and active_parameters. Newer/foreign/inconsistent registry identities are rejected and preserved; bulk and registry have separate version checks. Import streams CSV inside a transaction, verifies the parse's bytes again, integrity-checks and checkpoints a temporary WAL database, then exposes the committed immutable dataset. The registry references committed files only. Reports are immutable per run id. Evaluation ids/content derive from input/core/research/parameter fingerprints and config, without wall time. Research fingerprint covers all production modules including calibration, diagnostics, release gates, registry, CLI/main, package manifest and lockfile.

The app must use `open_readonly`; missing stores are errors and no directories/databases/migrations are created. Checkpointed databases use SQLite immutable read snapshots to avoid creating WAL sidecars. Mutable registry query methods open fresh read snapshots so later committed publications are visible even through an existing store handle. Existing active WALs require their existing WAL/SHM pair; missing shared memory or orphan shared memory fails read-only and requires explicit writable CLI recovery/checkpoint. Dataset readers do not write bulk files. All mutation methods reject a read-only store. A readonly legacy-v1 registry with no parameter tables resolves the fresh builtin without migration.

## CLI

Build with `cargo build --manifest-path native/gp-research/Cargo.toml --features cli`. On Windows the executable is `native/gp-research/target/debug/stock-research.exe`. Every operation requires an explicit app data directory. Example commands (replace dataset/run ids with the values returned by preceding commands):

```powershell
stock-research --help
stock-research import --app-data-dir C:/research/app --bundle C:/research/my-pit-bundle
stock-research validate --app-data-dir C:/research/app --dataset DATASET_ID
stock-research evaluate --app-data-dir C:/research/app --dataset DATASET_ID --parameters fixed-v1 --start 2021-10-01 --end 2026-09-30 --cost 10 --top 10 --mode auto
stock-research calibrate --app-data-dir C:/research/app --dataset DATASET_ID --end-date 2026-09-30
stock-research promote --app-data-dir C:/research/app --run CALIBRATION_RUN_ID
stock-research rollback --app-data-dir C:/research/app --parameters fixed-v1
```

`evaluate` defaults to corrected `fixed-v1`; optional parameters must be registered/current and belong to that dataset. Start defaults to session index60 (or the first available prior-session interval for a short dataset, which still must pass strict quality), end to source coverage end, and other fields to library defaults. `validate` supports optional `--start`, `--end`, `--mode`; `calibrate` supports only optional `--end-date`. Unknown, duplicate, missing or conflicting arguments fail explicitly. No elapsed-time override exists.

Stdout is exactly one JSON envelope `{ok:true,command,data}` or `{ok:false,command,error:{code,message,details}}`; progress goes to stderr. Successful evaluation/calibration data is the full report and can be redirected. Exit0 means success;1 I/O/internal failure;2 invalid/incomplete configuration/data (including a persisted `insufficient_data` calibration and a failed quality report);3 failed eligibility/performance. A short synthetic evaluation can succeed as offline research. Synthetic calibration reports remain `insufficient_data` even when a long replay is structurally complete; real failing calibration is `rejected`. Neither qualifies promotion.

## Deterministic calibration and promotion

Supply at least36 complete training months,12 validation months and60 test months, plus120 earlier warmup bars. The default test end is the latest complete supported month capped at2026-09-30; an explicit end must be a supported calendar month end. Trailing Saturday/Sunday closure can be inferred from the last supplied session; absent weekday holiday evidence cannot be invented. Train starts at a complete month after warmup; validation/test never shrink for short data. The last31 train/validation signal sessions are purged. Labels enter at the actual next close and exit10/20/30 trading sessions later, with realized actions and no labels beyond the relevant split cutoff.

Each range/trend/defensive mode requires120 effective signal dates with at least30 mature20-session candidate labels each. Sparse modes remain builtin with explicit reasons. Transition averages range and defensive, without a separate fit. SQLite feature/label caches live beside the immutable bulk database (`cache-*.sqlite`, short hash/process names for Windows paths) rather than millions of rows in reports/RAM. Histories remain bounded per day. The full required window is audited once; subsequent candidate portfolios reuse that internal audit.

Search starts at builtin vectors. Nonzero baseline weights stay within±0.05, zero support stays exactly zero, nonnegative weights sum1, and overheat penalty stays within±0.02. Every feasible0.01 weight transfer/penalty move is evaluated, for at most20 rounds. The objective is equal-date mean RankIC20 minus0.1 squared weight deviation; improvement must exceed1e-12. Ties use minimum baseline distance then canonical vectors. At most four distinct improving train candidates plus baseline are retained. Validation chooses greatest net annualized gain over corrected fixed, requiring drawdown degradation≤0.005 and turnover≤fixed×1.10; ties use lower drawdown, smaller baseline distance, canonical weights. Without a distinct feasible gain, fixed remains.

Candidate IDs hash canonical modes plus immutable origin dataset, split and current core/research context; the entire final parameter object has a separate evidence digest. Registration is immutable/idempotent and grants offline evaluation only. A dataset/test-window lock stores the weight signature and split **before the first test label or metric query**. An existing lock reuses frozen weights without retraining or validation reselection, including after implementation changes. A different signature cannot reopen the window.

Test portfolios use identical dataset/dates, monthly top10, CNY1e6, auto mode and10bps for candidate, corrected fixed and legacy balanced; candidate plus its same eligible-pool benchmark repeat at25/50bps. Diagnostics summarize tied-rank Spearman IC, sample/missing counts, equal-date quintiles, fractional tie-aware Top10 excess, pairwise factor correlations for train/validation/test, and monthly selection Jaccard. No label rows are embedded in reports. Constants return no IC, never NaN.

The typed pure release gate checks real/full-universe/quality/identity/frozen config;≥60 complete months; candidate annualized return≥fixed+0.01; absolute MDD≤fixed+0.005; turnover≤fixed×1.10 (zero fixed permits zero only); positive benchmark excess at10/25/50bps;≥60% positive complete nonoverlapping12-month windows; and a positive central95% lower mean paired-monthly excess bound from2000 circular six-month block draws (portable xorshift64* seed20261029). Industry target cap3 and actual latency evidence are required. The first fresh SQLite-reader PIT screen and three repeats measure the same read/scorer path; maximum repeat time is cached evidence. Labels say **local PIT replay**. Fresh connection does not control the OS filesystem cache and is not an online/provider latency claim.

Reports and calibration-owned digest receipts persist atomically. Receipts hash exact persisted JSON bytes; `serde_json/float_roundtrip` preserves the full f64 evidence when read. `save_report` cannot create a receipt. Promotion re-derives gates, verifies the report digest, exact parameter bounds/objects, all current fingerprints, dataset origin and the frozen window; a JSON `passed=true` has no authority. Only explicit promotion atomically changes active parameters and records qualification. No rows are deleted. Existing writers recheck schema identity before every mutation. `resolve_parameters(None)` uses the active pointer or fresh builtin if absent; corrupt/stale/unknown active pointers fail. Explicit fixed always means the current builtin. Explicit v2 requires a genuine previously qualified set. `research_parameters` permits an unqualified candidate only for offline evaluation. Rollback accepts fixed or a matching previously qualified set.

## Verification

```powershell
cargo test --manifest-path native/gp-research/Cargo.toml --all-features
cargo clippy --manifest-path native/gp-research/Cargo.toml --all-targets --all-features -- -D warnings
cargo fmt --manifest-path native/gp-research/Cargo.toml -- --check
```

Tests create real local CSV bundles and SQLite databases and run the public core. They cover disclosure boundaries/revisions, future mutation invariance, units, IPO/delist/ST/industry history, import/hash/path/duplicate/header/schema errors, strict coverage, orders/cash/fees/locks/receivables/actions, dynamic benchmarks, total-return maturities, deterministic search/ranks/bootstrap/gates, registry rejection/rollback/live readonly queries and CLI subprocess envelopes/exits. No real market dataset is included. Literal positive gates test the numerical contract; they are not actual release evidence. Full real-data promotion, market performance and provider/online latency remain unverified until an authoritative dataset is supplied.
