# Sentiment Workbench Contract

Implementation contract for the approved sentiment workbench. UI labels are Chinese. No trading actions or automatic paid analysis.

## Transport

All endpoints are POST using existing postJson/Tauri routing. Prefix `/api/sentiment/`.

- `snapshot`: `{stock_code, window_days:30, industry?}` -> SentimentSnapshot. No LLM call. Freezes known documents and cached market data at server time.
- `start`: `{stock_code, window_days:30, industry?, llm}` -> `{run_id}`. Background task; identical active requests coalesce.
- `status`: `{run_id}` -> SentimentRun (result embeds immutable snapshot).
- `cancel`: `{run_id}` -> `{cancelled}`.
- `latest`: `{stock_code}` -> `{analysis:SentimentAnalysis|null}`.
- `history`: `{stock_code?}` -> `{items:SentimentAnalysis[]}` (last 30). Enables watchlist phase labels.
- `followup`: `{analysis_id, question, llm}` -> SentimentFollowup, references original snapshot only.

Frontend types are in `desktop/frontend/src/types/sentiment.ts`. Rust uses the matching JSON keys. No keys, prompts with secrets, or LLM connection config in persisted analyses.

## Module Ownership

- `sentiment_data.rs`: source registry, provenance, deduplication, snapshot deterministic metrics. `build_snapshot(stock_code:&str, window_days:u32, cutoff:i64, generation:&str, documents:Vec<Value>, data:Value, industry:Option<&str>)->Result<Value,String>`.
- `sentiment_agent.rs`: Rig analysis with snapshot-only tools. `analyze(payload:Value,snapshot:Value,cancellation:Arc<RunCancellation>,sink:impl FnMut(Value)+Send)->Result<Value,String>`. Returns structured analysis fields without storage IDs. `followup(payload:Value,snapshot:Value,cancellation:Arc<RunCancellation>)->Result<Value,String>` returns answer/evidence_ids.
- `sentiment.rs`: independent SQLite persistence, snapshot acquisition from ResearchStore, background jobs, latest/history/cancellation and Tauri commands. Owns IDs, model metadata, stale detection and sanitization.
- Frontend: `SentimentPanel` is the new news workspace, reuses existing NewsRagPanel as events/history/management subview; no duplication of existing research ingestion APIs.

## Snapshot Evidence

Document inputs: document_id,title,content,source_tier,source_name,url,published_at,first_seen_at,metadata. Query data at cutoff using first_seen_at plus published_at. Unknown first-seen dates cannot establish historical availability. Price input uses current cached CoreDataSet shape.

Evidence IDs E1... are document facts/discussion. Metric IDs M1 (messages), M2 (price), M3 (industry) can be cited; each links to concrete snapshot metrics. Data quality gates constrain model output. No fabricated zeroes for missing feeds. Industry uses fixed primary industry, not opportunistic concepts. No history percentile until sufficient real historical coverage.

## Verification

Source-spoofing, duplicate stories/posts, relevant/time filtering, no lookahead, missing dimensions, immutable followups, explicit model failure, cancellation races, store replacement, UI route switching, desktop/mobile light/dark, real configured API only when accessible without exposing secrets.

## Layout revision — 2026-09-23

- One news route hosts two peer tabs, 消息 and 情绪. The first visit and a watchlist news action open 消息. Choosing 情绪 and then using the sidebar again does not force the tab back. Both tabs share one stock code and stay mounted after they have been opened.
- Desktop keeps the watchlist in the first grid column and analysis in the flexible second column. At 1181px and above, selecting a reference opens the third-column inspector; smaller viewports use the existing accessible Sheet.
- Mobile renders a single watchlist trigger and mounts the watchlist inside a Sheet. The 30-day chart is a summary line with previous and next day controls. Follow-up input is fixed at the bottom while an analysis is open.
- Reading order: stock context and compact coverage, stage/verdicts, follow-up, timeline, three dimensions, factual events/discussion, history. Conclusion and evidence use the same snapshot unless the user explicitly asks to see newer evidence.
- Timeline controls expose messages, price/volume and industry without reloading snapshot data. Viewport changes preserve the selected analysis and do not issue requests.
- Follow-up stays with its analysis for the page session and is not written to storage. Composition Enter cannot submit; mobile sending uses the visible button.
- Browser regression now asserts actual analysis-column width, rather than accepting absence of overflow as proof of a correct desktop layout. No DOM-forced disclosure fallback.

Verification: frontend 393 unit tests passed; production build and bundle budget passed; CSS architecture, density, theme parity and class coverage passed; contrast audit passed 24 combinations; fixed-data sentiment interaction matrix passed 8 combinations. The full npm test chain remains stopped at the previous news screenshot baseline (news/empty/desktop-1440-dark/news.png); no threshold was relaxed or screenshot baseline overwritten. Real Android keyboard/safe-area validation and an actual configured external model call are not covered by these UI fixtures.
