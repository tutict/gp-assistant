# Agent Run Replay UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an on-demand Agent run history and replay drawer that preserves lightweight chat storage while restoring complete ledger-backed execution details.

**Architecture:** Keep run-list and run-detail I/O in a typed `agentRuns` data module, move the existing result renderer into a reusable component, and isolate replay state and presentation in `AgentRunDrawer`. `AgentPanel` only persists `runId`, provides the two entry points, and reports stream completion so an open running detail can refresh once.

**Tech Stack:** React 19, TypeScript 7, Vitest 4, `react-test-renderer`, Lucide React, existing Tauri `getJson` routing, CSS custom properties, Rust/Tauri ledger commands.

---

## File Structure

- Create `desktop/frontend/src/lib/agentRuns.ts`: typed ledger list/detail client, JSON normalization, bounds, and URL construction.
- Create `desktop/frontend/src/lib/agentRuns.test.ts`: data-contract and request tests.
- Create `desktop/frontend/src/components/panels/AgentResultView.tsx`: shared live/replay structured result renderer extracted from `AgentPanel.tsx`.
- Create `desktop/frontend/src/components/panels/AgentResultView.test.tsx`: extraction regression tests.
- Create `desktop/frontend/src/components/panels/AgentRunDrawer.tsx`: drawer list/detail state, timeline projection, focus, retry, and refresh behavior.
- Create `desktop/frontend/src/components/panels/AgentRunDrawer.test.tsx`: interaction and state coverage using `react-test-renderer`.
- Modify `desktop/frontend/src/components/panels/AgentPanel.tsx`: retain `runId`, add replay controls and drawer orchestration, and close on conversation changes.
- Modify `desktop/frontend/src/components/panels/AgentPanel.test.tsx`: lightweight persistence and static accessibility coverage.
- Create `desktop/frontend/src/components/panels/AgentPanel.interaction.test.tsx`: streaming-to-run integration and drawer entry coverage.
- Modify `desktop/frontend/src/styles/pages.css`: desktop toolbar, message replay action, drawer, list, timeline, and state styles.
- Modify `desktop/frontend/src/styles/responsive.css`: full-screen mobile sheet and toolbar integration.
- Create `scripts/check-agent-replay-css.test.mjs`: geometry contract for the desktop drawer and mobile sheet.

The existing Rust ledger API and Tauri routes require no feature changes. Their tests remain regression gates.

### Task 1: Add The Typed Agent Run Client

**Files:**
- Create: `desktop/frontend/src/lib/agentRuns.ts`
- Create: `desktop/frontend/src/lib/agentRuns.test.ts`

- [ ] **Step 1: Write failing normalization and request tests**

Create `desktop/frontend/src/lib/agentRuns.test.ts` with these cases:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getJson } = vi.hoisted(() => ({ getJson: vi.fn() }));
vi.mock("./tauri", () => ({ getJson }));

import { getAgentRun, listAgentRuns, normalizeAgentRunDetail, normalizeAgentRunSummary } from "./agentRuns";

describe("agent run ledger client", () => {
  beforeEach(() => getJson.mockReset());

  it("normalizes bounded summaries and preserves defensive unknown status", () => {
    expect(normalizeAgentRunSummary({
      run_id: "run-1",
      conversation_id: "conversation-1",
      question: "Review the market",
      mode: "expert",
      status: "corrupt-status",
      started_at_epoch_ms: 100,
      duration_ms: 25,
      error: "x".repeat(2_100),
    })).toMatchObject({
      runId: "run-1",
      conversationId: "conversation-1",
      question: "Review the market",
      mode: "expert",
      status: "unknown",
      startedAtEpochMs: 100,
      durationMs: 25,
      error: "x".repeat(2_000),
    });
    expect(normalizeAgentRunSummary({ question: "missing id" })).toBeNull();
  });

  it("normalizes detail events and skips malformed entries", () => {
    const detail = normalizeAgentRunDetail({
      run_id: "run-2",
      question: "Trace evidence",
      mode: "research",
      status: "completed",
      started_at_epoch_ms: 200,
      events: [null, "bad", { type: "status", label: "Validated", percent: 94 }],
      result: { reply: "Complete", warnings: ["Verify inputs"] },
    });
    expect(detail?.events).toEqual([{ type: "status", label: "Validated", percent: 94 }]);
    expect(detail?.result?.reply).toBe("Complete");
  });

  it("requests current-conversation and all-run summaries", async () => {
    getJson.mockResolvedValue({ runs: [] });
    await listAgentRuns({ conversationId: "conversation/1" });
    await listAgentRuns({});
    expect(getJson).toHaveBeenNthCalledWith(
      1,
      "/api/agent/runs?limit=50&conversation_id=conversation%2F1",
      { signal: undefined },
    );
    expect(getJson).toHaveBeenNthCalledWith(2, "/api/agent/runs?limit=50", { signal: undefined });
  });

  it("encodes a run id and returns null for a missing ledger record", async () => {
    getJson.mockResolvedValue({ run: null });
    await expect(getAgentRun("run/1")).resolves.toBeNull();
    expect(getJson).toHaveBeenCalledWith("/api/agent/runs/run%2F1", { signal: undefined });
  });
});
```

- [ ] **Step 2: Run the focused test and verify the missing-module failure**

Run:

```powershell
npx.cmd vitest run src/lib/agentRuns.test.ts
```

Workdir: `desktop/frontend`

Expected: FAIL because `./agentRuns` does not exist.

- [ ] **Step 3: Implement the typed client and normalizers**

Create `desktop/frontend/src/lib/agentRuns.ts` with this public contract and behavior:

```ts
import type { AgentResult, AgentStreamEvent } from "../types";
import { normalizeAgentResult, normalizeAgentStreamEvent } from "./contracts";
import { getJson } from "./tauri";

export type AgentRunStatus = "running" | "completed" | "failed" | "unknown";

export interface AgentRunSummary {
  runId: string;
  conversationId?: string;
  question: string;
  mode: string;
  status: AgentRunStatus;
  startedAtEpochMs: number;
  completedAtEpochMs?: number;
  durationMs?: number;
  error?: string;
}

export interface AgentRunDetail extends AgentRunSummary {
  events: AgentStreamEvent[];
  result?: AgentResult;
}

export interface ListAgentRunsOptions {
  conversationId?: string;
  limit?: number;
  signal?: AbortSignal;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function boundedText(value: unknown, max: number): string {
  return typeof value === "string" ? value.trim().slice(0, max) : "";
}

function optionalNumber(value: unknown): number | undefined {
  if (value === null || value === undefined || value === "") return undefined;
  const number = Number(value);
  return Number.isFinite(number) ? number : undefined;
}

function normalizeStatus(value: unknown): AgentRunStatus {
  return value === "running" || value === "completed" || value === "failed" ? value : "unknown";
}

export function normalizeAgentRunSummary(value: unknown): AgentRunSummary | null {
  const record = asRecord(value);
  const runId = boundedText(record.run_id, 256);
  if (!runId) return null;
  const startedAtEpochMs = optionalNumber(record.started_at_epoch_ms) ?? 0;
  const completedAtEpochMs = optionalNumber(record.completed_at_epoch_ms);
  const durationMs = optionalNumber(record.duration_ms);
  const conversationId = boundedText(record.conversation_id, 256) || undefined;
  const error = boundedText(record.error, 2_000) || undefined;
  return {
    runId,
    conversationId,
    question: boundedText(record.question, 8_000),
    mode: boundedText(record.mode, 64) || "quick",
    status: normalizeStatus(record.status),
    startedAtEpochMs,
    completedAtEpochMs,
    durationMs,
    error,
  };
}

export function normalizeAgentRunDetail(value: unknown): AgentRunDetail | null {
  const record = asRecord(value);
  const summary = normalizeAgentRunSummary(record);
  if (!summary) return null;
  const events = Array.isArray(record.events)
    ? record.events.map(normalizeAgentStreamEvent).filter((event): event is AgentStreamEvent => Boolean(event && typeof event.type === "string"))
    : [];
  const rawResult = record.result;
  const result = rawResult && typeof rawResult === "object" && !Array.isArray(rawResult)
    ? normalizeAgentResult(rawResult)
    : undefined;
  return { ...summary, events, result };
}

export async function listAgentRuns(options: ListAgentRunsOptions = {}): Promise<AgentRunSummary[]> {
  const params = new URLSearchParams({ limit: String(Math.min(200, Math.max(1, options.limit ?? 50))) });
  if (options.conversationId) params.set("conversation_id", options.conversationId);
  const response = asRecord(await getJson(`/api/agent/runs?${params}`, { signal: options.signal }));
  return Array.isArray(response.runs)
    ? response.runs.map(normalizeAgentRunSummary).filter((run): run is AgentRunSummary => Boolean(run))
    : [];
}

export async function getAgentRun(runId: string, signal?: AbortSignal): Promise<AgentRunDetail | null> {
  const response = asRecord(await getJson(`/api/agent/runs/${encodeURIComponent(runId)}`, { signal }));
  return response.run == null ? null : normalizeAgentRunDetail(response.run);
}
```

- [ ] **Step 4: Run the focused test and type build**

Run:

```powershell
npx.cmd vitest run src/lib/agentRuns.test.ts
npm.cmd run build:app
```

Expected: the new tests pass and TypeScript reports no errors.

- [ ] **Step 5: Commit the data boundary**

```powershell
git add desktop/frontend/src/lib/agentRuns.ts desktop/frontend/src/lib/agentRuns.test.ts
git commit -m "feat(agent): add run replay client"
```

### Task 2: Extract The Shared Agent Result View

**Files:**
- Create: `desktop/frontend/src/components/panels/AgentResultView.tsx`
- Create: `desktop/frontend/src/components/panels/AgentResultView.test.tsx`
- Modify: `desktop/frontend/src/components/panels/AgentPanel.tsx` in the result-view imports and the block from `AgentResultView` through `GenericAgentResult`

- [ ] **Step 1: Write a failing extraction regression test**

Create `desktop/frontend/src/components/panels/AgentResultView.test.tsx`:

```tsx
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AgentResultView } from "./AgentResultView";

describe("AgentResultView", () => {
  it("renders the same tool, evidence, warning, and reply structure for replay", () => {
    const html = renderToStaticMarkup(
      <AgentResultView
        result={{
          reply: "Research complete",
          tool_calls: [{ id: "tool-1", tool: "adaptive_screen", label: "Market screen", status: "ok", output_summary: "3 candidates" }],
          evidence_summary: [{ title: "Market regime", summary: "Trend confirmed", source: "local quote", level: "tool_fact" }],
          answer_sections: [{ title: "Conclusion", bullets: ["Keep risk bounded"] }],
          warnings: ["Validate price freshness"],
        }}
        watchlist={[]}
        onToggleWatchlist={vi.fn()}
      />,
    );
    expect(html).toContain("adaptive_screen");
    expect(html).toContain("Market regime");
    expect(html).toContain("Keep risk bounded");
    expect(html).toContain("Validate price freshness");
  });
});
```

- [ ] **Step 2: Run the test and verify the missing-module failure**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentResultView.test.tsx
```

Workdir: `desktop/frontend`

Expected: FAIL because `./AgentResultView` does not exist.

- [ ] **Step 3: Move the existing renderer without semantic changes**

Create `AgentResultView.tsx` with the imports currently required by the renderer:

```tsx
import type { AgentResult, BacktestResult, NewsRagResult, ObserveResult, StockRowView, WatchlistItem } from "../../types";
import { actionResultKind, normalizeScreenRows } from "../../lib/contracts";
import { agentHarnessExecutionLabel, agentHarnessLabel, MAX_AGENT_EVIDENCE_ITEMS } from "../../lib/agent";
import { StockList } from "../StockList";
import { RawJson } from "../RawJson";
import { BacktestResultView } from "./BacktestPanel";
import { NewsRagView } from "./NewsRagPanel";
import { ObserveResultView } from "./ObservePanel";

export function AgentResultView({ result, watchlist, onToggleWatchlist }: {
  result: AgentResult;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
}) {
  const kind = actionResultKind(result);
  const nested = agentNestedResult(result, kind);
  const legacyView = (() => {
    if (kind === "backtest") return <BacktestResultView result={nested as unknown as BacktestResult} />;
    if (["screen", "sector", "graph", "trend"].includes(kind)) {
      const rows = normalizeScreenRows(nested) as StockRowView[];
      return rows.length
        ? <StockList items={rows} watchlist={watchlist} onToggleWatchlist={onToggleWatchlist} />
        : <GenericAgentResult result={nested || result} />;
    }
    if (kind === "news") return <NewsRagView result={nested as unknown as NewsRagResult} />;
    if (kind === "observe") return <ObserveResultView result={nested as unknown as ObserveResult} />;
    return <GenericAgentResult result={result} />;
  })();

  return <div className="agent-result-stack"><AgentStructuredResult result={result} />{legacyView}</div>;
}
```

Move these existing private helpers from `AgentPanel.tsx` into the same file unchanged: `AgentStructuredResult`, `agentNestedResult`, `GenericAgentResult`, and `asRecord`. Remove their old definitions and obsolete imports from `AgentPanel.tsx`, then import `AgentResultView` from the new module.

- [ ] **Step 4: Run focused and existing Agent panel tests**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentResultView.test.tsx src/components/panels/AgentPanel.test.tsx
```

Expected: PASS with no snapshot or markup regressions.

- [ ] **Step 5: Commit the extraction**

```powershell
git add desktop/frontend/src/components/panels/AgentResultView.tsx desktop/frontend/src/components/panels/AgentResultView.test.tsx desktop/frontend/src/components/panels/AgentPanel.tsx
git commit -m "refactor(agent): share result view with replay"
```

### Task 3: Preserve Run IDs In Lightweight Conversations

**Files:**
- Modify: `desktop/frontend/src/components/panels/AgentPanel.tsx` in `ChatMessage`, assistant placeholder creation, and `sanitizeConversations`
- Modify: `desktop/frontend/src/components/panels/AgentPanel.test.tsx`

- [ ] **Step 1: Add a failing sanitizer test**

Export the sanitizer as `sanitizeAgentConversations` and add this test to `AgentPanel.test.tsx`:

```tsx
it("persists runId while dropping heavy result and transient steps", async () => {
  const { sanitizeAgentConversations } = await import("./AgentPanel");
  const conversations = sanitizeAgentConversations([{
    id: "conversation-1",
    title: "Replay",
    mode: "expert",
    createdAt: 100,
    updatedAt: 200,
    messages: [{
      role: "assistant",
      content: "Complete",
      timestamp: 200,
      runId: "run-1",
      result: { reply: "Heavy" },
      steps: [{ stage: "tool", label: "Tool", percent: 50 }],
    }],
  }] as never);
  expect(conversations[0].messages[0]).toEqual({
    role: "assistant",
    content: "Complete",
    timestamp: 200,
    runId: "run-1",
    error: false,
  });
});
```

- [ ] **Step 2: Run the focused test and verify the failure**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentPanel.test.tsx
```

Expected: FAIL because `runId` is not part of `ChatMessage` and the sanitizer drops it.

- [ ] **Step 3: Add the lightweight run reference**

Apply these exact changes in `AgentPanel.tsx`:

```ts
interface ChatMessage {
  role: "user" | "assistant";
  content: string;
  timestamp: number;
  runId?: string;
  result?: AgentResult;
  steps?: AgentStep[];
  error?: boolean;
}
```

Set the placeholder reference at request creation:

```ts
const assistantMessage: ChatMessage = {
  role: "assistant",
  content: "准备中...",
  timestamp: now,
  runId,
  steps: [],
};
```

Rename and export the module-level sanitizer:

```ts
export function sanitizeAgentConversations(value: AgentConversation[]): AgentConversation[] {
```

Keep the existing call site synchronized:

```ts
const [conversations, setConversations, quotaError] = useLocalStorage<AgentConversation[]>(
  AGENT_HISTORY_KEY,
  [createConversation()],
  sanitizeAgentConversations,
);
```

Add the optional field to the persisted message map:

```ts
runId: typeof message.runId === "string" && message.runId.trim()
  ? message.runId.trim().slice(0, 256)
  : undefined,
```

- [ ] **Step 4: Run the Agent tests and type build**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentPanel.test.tsx src/lib/agent.test.ts
npm.cmd run build:app
```

Expected: PASS; the sanitizer retains only text metadata and `runId`.

- [ ] **Step 5: Commit lightweight persistence**

```powershell
git add desktop/frontend/src/components/panels/AgentPanel.tsx desktop/frontend/src/components/panels/AgentPanel.test.tsx
git commit -m "feat(agent): retain run ids in chat history"
```

### Task 4: Build The Replay Drawer

**Files:**
- Create: `desktop/frontend/src/components/panels/AgentRunDrawer.tsx`
- Create: `desktop/frontend/src/components/panels/AgentRunDrawer.test.tsx`

- [ ] **Step 1: Write failing list, detail, retry, and stale-response tests**

Create `AgentRunDrawer.test.tsx` using `react-test-renderer`. Mock `../../lib/agentRuns` and assert these behaviors with separate tests:

```tsx
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ listAgentRuns: vi.fn(), getAgentRun: vi.fn() }));
vi.mock("../../lib/agentRuns", () => api);

import { AgentRunDrawer, buildAgentRunTimeline } from "./AgentRunDrawer";

const summary = {
  runId: "run-1",
  conversationId: "conversation-1",
  question: "Review market regime",
  mode: "expert",
  status: "completed" as const,
  startedAtEpochMs: 100,
  completedAtEpochMs: 200,
  durationMs: 100,
};

function textOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) return value.map(textOf).join("");
  if (value && typeof value === "object" && "children" in value) {
    return textOf((value as { children?: unknown }).children);
  }
  return "";
}
```

Required assertions:

- opening without `initialRunId` calls `listAgentRuns({ conversationId: "conversation-1" })` and renders the summary;
- switching the segmented control to all runs calls `listAgentRuns({})`;
- opening with `initialRunId="run-1"` calls `getAgentRun("run-1")` and renders the result;
- a null detail renders `本次运行未成功留痕`;
- a rejected request renders `重试`, and invoking it repeats the exact request;
- resolving an older request after a scope change does not replace the new list;
- changing `finishedRunId` to the selected running run refetches detail exactly once;
- `Escape` invokes `onClose`;
- `Tab` and `Shift+Tab` wrap within the drawer's focusable controls;
- `buildAgentRunTimeline` skips `result`, bounds labels to 500 characters, and returns neutral entries for unknown event types.

- [ ] **Step 2: Run the focused test and verify the missing-module failure**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentRunDrawer.test.tsx
```

Workdir: `desktop/frontend`

Expected: FAIL because `AgentRunDrawer` does not exist.

- [ ] **Step 3: Implement the drawer API and timeline projection**

Use this component contract:

```tsx
export interface AgentRunDrawerProps {
  open: boolean;
  activeConversationId?: string;
  initialRunId?: string;
  finishedRunId?: string;
  returnFocusElement?: HTMLElement | null;
  watchlist: WatchlistItem[];
  onToggleWatchlist: (item: StockRowView) => void;
  onClose: () => void;
}
```

Export a pure `buildAgentRunTimeline(events)` helper. It must:

```ts
export interface AgentRunTimelineItem {
  key: string;
  type: string;
  label: string;
  detail?: string;
  tone: "neutral" | "active" | "success" | "error";
}
```

- skip events whose type is `result`;
- use `event.label` or `event.stage` for status events;
- use `payload.label`, `payload.tool`, or `event.action` for tool starts;
- use `payload.output_summary`, `payload.status`, or a completed fallback for tool results;
- use bounded `event.message` for errors;
- map evidence and final events to stable visible labels;
- map unknown types to a neutral label based only on their bounded type string;
- cap every label and detail at 500 characters.

Implement request isolation with a monotonically increasing token:

```ts
const requestTokenRef = useRef(0);

const beginRequest = useCallback(() => {
  requestTokenRef.current += 1;
  return requestTokenRef.current;
}, []);

const isCurrentRequest = useCallback((token: number) => requestTokenRef.current === token, []);
```

When `open` becomes true, synchronize to `detail` for a non-empty `initialRunId`; otherwise synchronize to `list` and load the current-conversation scope. List state must retain separate loading, error, and summaries. Detail state must retain separate loading, error, missing, and run values.

If the current scope has no `activeConversationId`, render the current-conversation empty state without issuing an unfiltered request. Only the explicit `all runs` scope may omit `conversation_id`.

Guard the one-time completion refresh separately from ordinary detail loads:

```ts
const refreshedCompletionRef = useRef<string>();

useEffect(() => {
  if (!open || !selectedRunId || selectedRunId !== finishedRunId || detail?.status !== "running") return;
  if (refreshedCompletionRef.current === finishedRunId) return;
  refreshedCompletionRef.current = finishedRunId;
  void loadDetail(selectedRunId);
}, [detail?.status, finishedRunId, loadDetail, open, selectedRunId]);
```

Render this stable structure:

```tsx
<aside
  className={`agent-run-drawer ${open ? "open" : ""}`}
  role="dialog"
  aria-modal="true"
  aria-label="Agent 运行复盘"
  aria-hidden={!open}
  onKeyDown={(event) => { if (event.key === "Escape") onClose(); }}
>
  <header className="agent-run-drawer-head" />
  <div className="agent-run-drawer-body" />
</aside>
```

The header contains `ArrowLeft` in detail view, the current title, and an `X` close `IconButton`. List view contains a two-option segmented control and semantic buttons for summary rows. Detail view renders overview, timeline, persisted error, and `AgentResultView` only when a normalized result exists. All loading, empty, missing, and request failures use explicit text and a real retry button.

On open, focus the close button. On close after a previously open render, call `returnFocusElement?.focus()`. Keep Escape and Tab handling within the dialog surface rather than installing a document-wide key handler. For Tab, query enabled buttons, links, inputs, selects, textareas, and `[tabindex]:not([tabindex="-1"])` inside the drawer; wrap forward from the last item to the first and backward from the first item to the last.

- [ ] **Step 4: Run drawer tests and type build**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentRunDrawer.test.tsx src/lib/agentRuns.test.ts
npm.cmd run build:app
```

Expected: PASS with no stale-state or TypeScript errors.

- [ ] **Step 5: Commit the isolated drawer**

```powershell
git add desktop/frontend/src/components/panels/AgentRunDrawer.tsx desktop/frontend/src/components/panels/AgentRunDrawer.test.tsx
git commit -m "feat(agent): add run replay drawer"
```

### Task 5: Integrate Replay Entry Points With Agent Streaming

**Files:**
- Modify: `desktop/frontend/src/components/panels/AgentPanel.tsx`
- Create: `desktop/frontend/src/components/panels/AgentPanel.interaction.test.tsx`
- Modify: `desktop/frontend/src/components/panels/AgentPanel.test.tsx`

- [ ] **Step 1: Write failing entry-point and stream-completion tests**

In `AgentPanel.test.tsx`, extend the static markup assertions to require a top-level button with the accessible label `运行历史` when a conversation is active.

Create `AgentPanel.interaction.test.tsx` with a minimal `localStorage` stub and mocked Tauri dependencies. `isTauriRuntime` returns true, `getTauriListen` captures the `agent-stream-event` handler, `getTauriInvoke` returns an async `api_agent_stream` mock, and `buildTauriAgentPayload` returns its input. Mock `AgentRunDrawer` with a component that captures its props. Capture drawer props and assert:

```tsx
expect(capturedDrawerProps).toMatchObject({
  open: true,
  activeConversationId: "conversation-1",
  initialRunId: "run-message-1",
});
```

Required cases:

- clicking the header history control opens the drawer with no `initialRunId`;
- clicking an assistant message replay control opens its exact `runId`;
- historical assistant messages without `runId` render no replay control;
- switching conversations invokes drawer close state;
- receiving a terminal `result` or `error` event sets `finishedRunId` to the active stream run;
- a ledger replay failure exposed by the mocked drawer does not disable the composer or mutate message content.

- [ ] **Step 2: Run the focused tests and verify missing controls**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentPanel.test.tsx src/components/panels/AgentPanel.interaction.test.tsx
```

Expected: FAIL because the toolbar, message action, drawer state, and completion signal are absent.

- [ ] **Step 3: Add drawer orchestration state**

Import `History`, `FileSearch`, and `AgentRunDrawer`, then add:

```ts
const [replayOpen, setReplayOpen] = useState(false);
const [replayRunId, setReplayRunId] = useState<string>();
const [finishedRunId, setFinishedRunId] = useState<string>();
const replayTriggerRef = useRef<HTMLElement | null>(null);

const openRunHistory = useCallback((trigger: HTMLElement) => {
  replayTriggerRef.current = trigger;
  setReplayRunId(undefined);
  setReplayOpen(true);
}, []);

const openRunReplay = useCallback((runId: string, trigger: HTMLElement) => {
  replayTriggerRef.current = trigger;
  setReplayRunId(runId);
  setReplayOpen(true);
}, []);

const closeRunReplay = useCallback(() => setReplayOpen(false), []);
```

Close and clear direct selection when the active conversation changes:

```ts
useEffect(() => {
  setReplayOpen(false);
  setReplayRunId(undefined);
}, [activeConversationId]);
```

- [ ] **Step 4: Add the toolbar and per-message controls**

Add a compact toolbar before `agent-thread`. The desktop row displays the conversation title and an `IconButton` using `History`; the mobile action cluster includes the same history action. Give the desktop control class `agent-thread-history` and the mobile control class `agent-mobile-history` so responsive CSS can show exactly one entry per viewport.

Inside each assistant `agent-message-meta`, render this only when `msg.runId` exists:

```tsx
<IconButton
  className="agent-message-replay"
  onClick={(event) => openRunReplay(msg.runId!, event.currentTarget)}
  label="查看本次运行复盘"
  title="查看本次运行复盘"
  icon={<FileSearch size={15} aria-hidden="true" />}
/>
```

Mount one drawer as the final child of `agent-chat-stage`:

```tsx
<AgentRunDrawer
  open={replayOpen}
  activeConversationId={activeConversation?.id}
  initialRunId={replayRunId}
  finishedRunId={finishedRunId}
  returnFocusElement={replayTriggerRef.current}
  watchlist={watchlist}
  onToggleWatchlist={toggleWatchlist}
  onClose={closeRunReplay}
/>
```

- [ ] **Step 5: Signal terminal stream completion once**

In `applyEvent`, set `finishedRunId` in both terminal branches after updating the message:

```ts
} else if (event.type === "result") {
  const result = normalizeAgentResult(event.response || {});
  patchAssistant({ content: String(result.reply || "已完成。"), result, steps: undefined });
  setFinishedRunId(runId);
} else if (event.type === "error") {
  patchAssistant({ content: event.message || "智能体执行失败。", error: true, steps: undefined });
  setFinishedRunId(runId);
}
```

Also set it in the `catch` branch after patching the local failure so a best-effort ledger failure can be discovered by retry. Do not tie composer `loading` state to any drawer request.

- [ ] **Step 6: Run integration tests and the Agent subset**

Run:

```powershell
npx.cmd vitest run src/components/panels/AgentPanel.test.tsx src/components/panels/AgentPanel.interaction.test.tsx src/components/panels/AgentRunDrawer.test.tsx src/components/panels/AgentResultView.test.tsx src/lib/agentRuns.test.ts src/lib/agent.test.ts src/lib/tauri.agent.test.ts
```

Expected: PASS for entry points, terminal refresh, existing stream payloads, and existing Tauri routes.

- [ ] **Step 7: Commit integration**

```powershell
git add desktop/frontend/src/components/panels/AgentPanel.tsx desktop/frontend/src/components/panels/AgentPanel.test.tsx desktop/frontend/src/components/panels/AgentPanel.interaction.test.tsx
git commit -m "feat(agent): connect chat to run replay"
```

### Task 6: Add Desktop And Mobile Replay Styling

**Files:**
- Modify: `desktop/frontend/src/styles/pages.css` after the final Agent workspace block
- Modify: `desktop/frontend/src/styles/responsive.css` inside the existing Agent `max-width: 768px` block

- [ ] **Step 1: Add a failing CSS contract test**

Add a focused Node test at `scripts/check-agent-replay-css.test.mjs` that reads both CSS files and asserts the presence of these selectors:

```js
import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";

const pages = fs.readFileSync("desktop/frontend/src/styles/pages.css", "utf8");
const responsive = fs.readFileSync("desktop/frontend/src/styles/responsive.css", "utf8");

test("agent replay drawer has bounded desktop and full-screen mobile layouts", () => {
  assert.match(pages, /\.agent-run-drawer\s*\{/);
  assert.match(pages, /width:\s*clamp\(440px,\s*44vw,\s*640px\)/);
  assert.match(pages, /\.agent-run-status\.failed/);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*inset:\s*0/s);
  assert.match(responsive, /\.agent-run-drawer\s*\{[^}]*width:\s*100%/s);
});
```

- [ ] **Step 2: Run the CSS contract and verify it fails**

Run from repository root:

```powershell
node --test scripts/check-agent-replay-css.test.mjs
```

Expected: FAIL because the replay selectors do not exist.

- [ ] **Step 3: Add restrained desktop styles**

Add concrete styles using existing tokens. The top-level geometry must include:

```css
.agent-thread-toolbar {
  display: flex;
  min-height: 42px;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 6px 14px;
  border-bottom: 1px solid var(--line);
  background: var(--surface);
}

.agent-run-drawer {
  position: absolute;
  z-index: var(--z-drawer);
  inset: 0 0 0 auto;
  display: grid;
  width: clamp(440px, 44vw, 640px);
  min-width: 0;
  grid-template-rows: auto minmax(0, 1fr);
  visibility: hidden;
  transform: translateX(100%);
  border-left: 1px solid var(--line);
  background: var(--surface);
  box-shadow: -12px 0 28px rgba(0, 0, 0, 0.24);
  transition: transform var(--motion-standard) var(--ease-out), visibility 0s linear var(--motion-standard);
}

.agent-run-drawer.open {
  visibility: visible;
  transform: translateX(0);
  transition-delay: 0s;
}

.agent-run-drawer-head {
  display: flex;
  min-height: 48px;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-bottom: 1px solid var(--line);
}

.agent-run-drawer-body {
  min-height: 0;
  padding: 12px;
  overflow-y: auto;
}
```

Add focused selectors for the segmented scope control, fixed-height icon buttons, summary rows, overview definition list, timeline rows, status labels, explicit loading/empty/error panels, retry action, and message metadata replay control. Use `var(--success)`, `var(--error)`, `var(--warning)`, and neutral text tokens so status is not a one-hue treatment. Keep radii at `var(--radius-md)` or below and do not nest decorative cards.

Update `.agent-chat-stage` to use `grid-template-rows: auto minmax(0, 1fr) auto` for the new toolbar.

- [ ] **Step 4: Add mobile full-screen behavior**

Inside the existing Agent mobile media block in `responsive.css`, add:

```css
.agent-thread-toolbar {
  min-height: 50px;
  padding: 6px 7px 6px 94px;
}

.agent-run-drawer {
  position: fixed;
  inset: 0;
  width: 100%;
  max-width: none;
  border-left: 0;
}

.agent-run-drawer-head {
  padding-top: max(7px, env(safe-area-inset-top));
}
```

Ensure the drawer z-index is above the conversation rail and scrim, and its body bottom padding includes `env(safe-area-inset-bottom)`. Keep the toolbar title single-line with ellipsis so it cannot collide with mobile actions.

Hide `.agent-thread-history` in the mobile block and keep `.agent-mobile-history` hidden in desktop styles. The mobile actions row is the only mobile history entry.

- [ ] **Step 5: Run CSS, density, unit, and build checks**

Run:

```powershell
node --test scripts/check-agent-replay-css.test.mjs
npm.cmd run test:density
npm.cmd run test:architecture
npm.cmd run test:unit
npm.cmd run build:app
```

Workdir for npm commands: `desktop/frontend`

Expected: every command passes and the CSS architecture checker reports no ownership violation.

- [ ] **Step 6: Commit styles and their contract**

```powershell
git add scripts/check-agent-replay-css.test.mjs desktop/frontend/src/styles/pages.css desktop/frontend/src/styles/responsive.css
git commit -m "style(agent): add responsive replay drawer"
```

### Task 7: Verify The Workflow And Perform Four-Axis Review

**Files:**
- Modify only files implicated by review findings.

- [ ] **Step 1: Run all automated verification**

From `desktop/frontend`:

```powershell
npm.cmd run test:unit
npm.cmd run test:density
npm.cmd run test:architecture
npm.cmd run build:app
```

From repository root:

```powershell
cargo test --manifest-path desktop/src-tauri/Cargo.toml
cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check
git diff --check
```

Expected: 100% pass. Existing unrelated warnings may be reported but no new warning may originate from replay files.

- [ ] **Step 2: Verify desktop and mobile layouts with Playwright**

Start the frontend on an unused port:

```powershell
npm.cmd run dev -- --host 127.0.0.1 --port 4174
```

Use Playwright request interception for `/api/agent/runs` and `/api/agent/runs/run-1` with completed, failed, and running fixtures. Seed a lightweight conversation containing assistant `runId: "run-1"`, open the Agent view, and capture:

- 1440 x 900: history list and direct detail while chat remains visible;
- 768 x 1024: full-screen detail with no toolbar collision;
- 390 x 844: full-screen list, scope control, detail back action, and composer unaffected after close.

Inspect each screenshot for clipped text, overlapping controls, unexpected layout shifts, inaccessible close/back actions, and blank result content. Exercise Escape close and verify focus returns to the invoking button.

- [ ] **Step 3: Review on four axes**

Review the complete implementation range from commit `055dab9` to `HEAD`:

1. Functional/spec: every acceptance criterion in `docs/superpowers/specs/2026-08-08-agent-run-replay-ui-design.md` has an implementation and test.
2. Correctness/boundaries: stale requests, missing records, invalid events, stream completion refresh, conversation switches, and localStorage quotas cannot corrupt chat state.
3. Security/privacy: no raw request/config object is rendered; all ledger text remains escaped and bounded; run IDs are encoded.
4. Maintainability/tests: `AgentPanel` does not absorb drawer internals, the result renderer has one implementation, and tests cover public behavior rather than CSS implementation details except the explicit geometry contract.

Record actionable findings with file and line references. Fix all severity 1-2 findings before completion. Add regression tests before fixes when the finding is behavioral.

- [ ] **Step 4: Re-run affected checks after review fixes**

Run the focused failing test first, then repeat the full commands from Step 1. Recheck the affected desktop/mobile screenshot when a style or interaction changes.

Expected: all checks pass and no unresolved high- or medium-severity review finding remains.

- [ ] **Step 5: Commit review corrections if needed**

If review required changes:

```powershell
git add -- desktop/frontend/src/lib/agentRuns.ts desktop/frontend/src/lib/agentRuns.test.ts desktop/frontend/src/components/panels/AgentResultView.tsx desktop/frontend/src/components/panels/AgentResultView.test.tsx desktop/frontend/src/components/panels/AgentRunDrawer.tsx desktop/frontend/src/components/panels/AgentRunDrawer.test.tsx desktop/frontend/src/components/panels/AgentPanel.tsx desktop/frontend/src/components/panels/AgentPanel.test.tsx desktop/frontend/src/components/panels/AgentPanel.interaction.test.tsx desktop/frontend/src/styles/pages.css desktop/frontend/src/styles/responsive.css scripts/check-agent-replay-css.test.mjs
git commit -m "fix(agent): harden run replay boundaries"
```

If review found no issues, do not create an empty commit. Report the reviewed range and residual test risk in the completion summary.
