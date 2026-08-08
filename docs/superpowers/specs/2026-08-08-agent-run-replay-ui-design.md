# Agent Run Replay UI Design

**Status:** Approved

**Date:** 2026-08-08

## Context

The Agent Run Ledger persists each Agent execution independently from the lightweight chat transcript. The desktop UI still discards `result` and `steps` before writing conversations to `localStorage`, so a reload preserves the conversation text but removes the tool trace, evidence catalog, structured result, and failure context.

This feature exposes the existing ledger through an on-demand replay drawer. It must restore inspectability without increasing chat storage size or making ledger availability a prerequisite for normal Agent execution.

## Goals

- Open the exact run associated with a persisted assistant message.
- Browse runs for the active conversation and, when requested, all recorded runs.
- Load full run details only when the user selects a run.
- Present run metadata, execution events, evidence, risk output, and the final structured result in one inspectable surface.
- Keep replay failures isolated from chat and Agent execution.
- Establish a UI boundary that can later host diagnosis, decision-path, and case-library features.

## Non-Goals

- Changing Agent routing, stock selection, evidence gates, or model synthesis.
- Editing, rerunning, deleting, exporting, or comparing ledger records.
- Adding search, pagination, retention policy controls, or case-library classification.
- Persisting full Agent results in `localStorage`.
- Rendering the ledger's sanitized raw request JSON as a user-facing debug view.

## Chosen Approach

Use a right-side replay drawer with two entry points:

1. A replay icon beside an assistant response opens that response's run directly.
2. A history icon in the conversation header opens a run list, defaulting to the active conversation.

The drawer keeps the original chat visible on desktop. On narrow screens it becomes a full-screen sheet. This approach preserves conversation density while allowing the run detail surface to grow into a decision-path inspector later.

## Architecture

### `AgentPanel`

`AgentPanel` remains responsible for chat orchestration and the two replay entry points. It adds only lightweight drawer state and a stable run reference on messages.

`ChatMessage` gains:

```ts
runId?: string;
```

The assistant placeholder receives the generated `runId` before streaming starts. `sanitizeConversations` keeps `runId` while continuing to remove `result` and `steps`. Existing messages without a run ID remain valid and do not show the per-response replay action.

When the active conversation changes, `AgentPanel` closes the drawer and clears its selected run. This prevents a detail opened from one conversation from appearing beside another.

### `AgentRunDrawer`

A new focused component owns replay presentation and request state. Its public inputs are:

- whether the drawer is open;
- the active conversation ID;
- an optional directly selected run ID;
- a close callback.

It supports two internal views:

- `list`: summaries for the current scope;
- `detail`: one fully loaded run, with a back action to the list.

It also owns the scope control, request cancellation/staleness guards, loading states, retry actions, and the one-time refresh after a currently open run finishes.

### `lib/agentRuns.ts`

A new data module defines the ledger contracts, calls the existing Tauri-routed endpoints, and normalizes untrusted JSON into stable frontend values:

```ts
interface AgentRunSummary {
  runId: string;
  conversationId?: string;
  question: string;
  mode: string;
  status: "running" | "completed" | "failed" | "unknown";
  startedAtEpochMs: number;
  completedAtEpochMs?: number;
  durationMs?: number;
  error?: string;
}

interface AgentRunDetail extends AgentRunSummary {
  events: AgentStreamEvent[];
  result?: AgentResult;
}
```

The module exposes list and detail functions over:

- `GET /api/agent/runs?conversation_id=<id>&limit=50`
- `GET /api/agent/runs?limit=50`
- `GET /api/agent/runs/:runId`

The data module does not expose the stored request object to the drawer.

### Reusable Result View

Move `AgentResultView` and its private rendering helpers from `AgentPanel.tsx` into a dedicated component module. Live answers and replay details use the same normalized rendering path, preventing replay-specific interpretations of tool facts or evidence.

The extraction is limited to code required for reuse. It does not change result semantics or visual behavior in the live chat.

## Data Flow

### New Agent Run

1. The frontend generates `runId` and writes it to the assistant placeholder.
2. The stream request sends the same `run_id` and `conversation_id` to the Tauri command.
3. The ledger records the run independently from the transcript.
4. Stream events continue updating temporary `steps` and the live result.
5. Conversation persistence keeps `runId` and text, but removes `steps` and `result`.

### Direct Replay

1. The user activates the replay icon on an assistant message.
2. The drawer opens in `detail` view for that message's `runId`.
3. The drawer requests the detail endpoint.
4. The response is normalized, then rendered as metadata, event timeline, errors or warnings, and the shared structured result.

### History Replay

1. The user activates the conversation header's history icon.
2. The drawer opens in `list` view with the `current conversation` scope.
3. The drawer requests lightweight summaries only.
4. The user may switch to `all runs`, which repeats the request without `conversation_id`.
5. Selecting a summary loads only that run's detail.

The list is capped at 50 records in this iteration. Pagination and search remain outside scope.

## Interaction Design

### Desktop

- The drawer overlays the right edge of the Agent stage at a width constrained between 440 px and 640 px.
- The underlying chat does not resize and remains visible for comparison.
- Opening and closing the drawer does not reset the chat scroll position.
- Add a compact toolbar above the thread containing the active conversation title and the history entry. On mobile, the history entry joins the existing menu and new-conversation actions instead of creating a second toolbar row.
- The drawer header contains its title, list/detail navigation, and a close icon.
- The history entry uses the Lucide `History` icon.
- The per-response entry sits in the assistant message metadata row, uses the Lucide `FileSearch` icon, and has an accessible tooltip.

### Mobile

- At the existing 768 px mobile breakpoint, the drawer becomes a full-screen sheet.
- It has a stable header and independently scrollable body.
- Opening it does not reopen or overlap the conversation rail.
- All actions remain keyboard accessible and have text alternatives.

### Run List

- A segmented control switches between `current conversation` and `all runs`.
- Records are ordered newest first.
- Each row displays the question, mode, status, start time, and duration when available.
- `running`, `completed`, `failed`, and defensive `unknown` states use both an icon and visible text; color is supplementary.
- Empty current-conversation and all-runs states use scope-specific text.

### Run Detail

The detail body is an unframed vertical layout in this order:

1. Run overview: mode, status, start time, completion time, and duration.
2. Execution timeline: normalized status and tool events in recorded order.
3. Failure or warning summary when present.
4. Final structured result rendered by the shared result view.

Timeline rows show stable labels derived from known event types. Unknown event types receive a neutral fallback label and are not rendered as raw JSON. Duplicate terminal `result` events are not repeated in the timeline because the result has its own section.

## State And Refresh Rules

- Opening from a response selects that run immediately.
- Opening from the header starts in the list view and clears a previous direct selection.
- Returning from detail keeps the current scope and previously loaded summaries.
- Closing the drawer clears transient request errors but may retain cached summaries for the current component lifetime.
- Responses are ignored when their request token no longer matches the active scope or selected run.
- When the active Agent stream finishes and the drawer is showing that same `running` run, the detail is fetched once more. There is no background polling in this iteration.

## Error Handling

- List and detail failures render inside the drawer with a retry action.
- A missing detail record displays `This run was not successfully recorded` and does not substitute the lightweight chat text as a full replay.
- A failed run displays all recorded events plus the persisted error summary; it does not render a fabricated success result.
- A running run displays its current status. Since events are committed on terminal completion today, the live message steps remain the source for in-progress detail until the final refresh.
- Malformed event entries are skipped individually. A malformed result falls back to an unavailable-result message without breaking the rest of the detail.
- Ledger errors never disable the composer, alter an existing response, or stop a new Agent request.

## Security And Privacy

- All questions, errors, event labels, and summaries render as React text, never injected HTML.
- The drawer does not render the stored request object, model credentials, headers, proxy values, or raw configuration.
- The UI consumes only the ledger's already-sanitized summaries, events, and result.
- Run IDs are URL-encoded before detail requests.
- User-visible error strings are truncated to 2,000 characters and event labels to 500 characters; both are treated as text.

## Accessibility

- The drawer uses dialog/sheet semantics with an accessible name.
- Focus moves into the drawer on open and returns to the invoking control on close.
- Escape closes the drawer unless another modal owns the event.
- Icon-only controls use existing `IconButton` labels and tooltips.
- Loading, empty, running, failed, and completed states are conveyed in text and announced where appropriate.
- Focus order follows header controls, scope control, list or detail body, then close/navigation actions according to visual order.

## Testing

### Data Module Tests

- Normalize complete, failed, and running summaries.
- Normalize detail records and skip malformed event entries.
- Encode run IDs and build current-conversation versus all-runs requests correctly.
- Reject or safely default missing required identifiers and invalid status values.

### Conversation Persistence Tests

- Preserve `runId` through `sanitizeConversations`.
- Continue removing `result` and `steps`.
- Accept historical messages without `runId`.

### Component Tests

- Header history entry opens the current-conversation list.
- Scope switching issues the correct list request and ignores stale responses.
- A response replay entry opens and loads its exact run.
- List, detail, loading, empty, running, completed, failed, missing, malformed, and retry states render correctly.
- Returning to the list preserves scope and summaries.
- Changing conversations closes the drawer.
- Completing the currently selected stream triggers one detail refresh.
- Drawer controls have accessible names and keyboard close behavior.

### Regression Verification

- Existing Agent stream, result rendering, and lightweight transcript tests remain green.
- Frontend unit tests and production build pass.
- Rust tests and formatting checks pass because the UI relies on the existing ledger API contract.
- Desktop and mobile screenshots verify that the drawer does not overlap the conversation rail, composer, or its own header/content.

## Acceptance Criteria

1. Every newly created assistant message retains its `runId` after reload without retaining `result` or `steps`.
2. A response with a recorded run opens the matching full detail on demand.
3. The header entry lists current-conversation runs and can switch to all runs.
4. Selecting a summary loads detail lazily and renders the shared structured result.
5. Failed, running, missing, empty, loading, and request-error states are explicit and recoverable where applicable.
6. Replay failures do not change chat state or block Agent execution.
7. Desktop and mobile drawer layouts are usable without incoherent overlap or clipped controls.
8. No raw request configuration or unsafe HTML is exposed.

## Future Extensions

The drawer intentionally leaves room for later, separately designed sections:

- diagnosis and decision/research-plan phases;
- visual decision paths and evidence gates;
- linking a run to later market performance or backtest outcomes;
- promotion of selected runs into a searchable case library.

These extensions must not be introduced as part of this implementation.
