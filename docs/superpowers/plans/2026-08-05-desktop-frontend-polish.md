# Desktop Frontend Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Complete the Windows desktop frontend productivity controls and regression harness while preserving the already-landed CSS architecture and bundle improvements.

**Architecture:** Keep routing and shared state in `App.tsx`, keep the header presentational, and isolate keyboard/density behavior in hooks with pure helpers that can be unit tested. Extend the existing Playwright screenshot runner into a deterministic visual/e2e harness using route interception and localStorage seeding instead of production-only mock switches.

**Tech Stack:** React 19, TypeScript 7, Vite 8, Vitest, react-test-renderer, Playwright, PostCSS-based guard scripts.

---

### Task 1: Desktop header productivity controls

**Files:**
- Modify: `desktop/frontend/src/App.tsx`
- Modify: `desktop/frontend/src/components/Header.tsx`
- Create: `desktop/frontend/src/components/Header.test.tsx`
- Modify: `desktop/frontend/src/components/StockCodeInput.tsx`
- Modify: `desktop/frontend/src/components/StockCodeInput.test.tsx`
- Create: `desktop/frontend/src/hooks/useDensity.test.tsx`
- Modify: `desktop/frontend/src/hooks/useDensity.ts`
- Modify: `desktop/frontend/src/hooks/useGlobalShortcuts.test.ts`
- Modify: `desktop/frontend/src/hooks/useGlobalShortcuts.ts`
- Modify: `desktop/frontend/src/styles/shell.css`
- Modify: `desktop/frontend/src/styles/responsive.css`
- Modify: `desktop/frontend/src/styles/tokens.css`

- [x] **Step 1: Add failing unit tests for header search commit, density controls, help overlay, and shortcut resolution**

  Tests must assert that `Header` exposes the `搜索股票` combobox and calls `onSearchCommit`, that density defaults to comfortable and persists compact, that number shortcuts ignore editable targets, and that `?`/Escape open and close help.

- [x] **Step 2: Run focused tests and verify RED**

  Run: `npm.cmd run test:unit -- src/components/Header.test.tsx src/hooks/useDensity.test.ts src/hooks/useGlobalShortcuts.test.ts src/components/StockCodeInput.test.tsx`

  Expected: FAIL because the new header props/rendered controls and density helper contract are missing.

- [x] **Step 3: Wire behavior through `App.tsx`**

  `App` owns `searchCode`, `searchInputRef`, `shortcutHelpOpen`, and `density`; a committed stock code calls existing `observeStock(code)`. `useGlobalShortcuts` focuses the ref, routes 1-5 through `navigate`, toggles the help dialog, and handles Escape without duplicating document listeners.

- [x] **Step 4: Render compact desktop controls without changing mobile chrome**

  `Header` renders `StockCodeInput`, watchlist count, density toggle, theme toggle, and a keyboard-help icon button. Mobile CSS hides desktop search/status/density/help controls and preserves the existing 48px header.

- [x] **Step 5: Run focused tests and verify GREEN**

  Run: `npm.cmd run test:unit -- src/components/Header.test.tsx src/hooks/useDensity.test.ts src/hooks/useGlobalShortcuts.test.ts src/components/StockCodeInput.test.tsx`

  Expected: all focused tests pass.

### Task 2: Desktop architecture contracts and Playwright matrix

**Files:**
- Modify: `scripts/check-ui-density.mjs`
- Modify: `scripts/ui-screenshot.mjs`
- Modify: `desktop/frontend/package.json`
- Modify: `scripts/release-check.ps1`

- [x] **Step 1: Make the density guard fail when wide-screen constraints disappear**

  Add explicit source contracts for `.workbench` wide-screen centering, form/control max width, and 76ch prose width. Run `npm.cmd run test:density` after temporarily targeting a missing contract and confirm RED, then point the guard at the real required declarations.

- [x] **Step 2: Extend the screenshot device/configuration matrix**

  Add `desktop-1920`, `desktop-2560`, `desktop-1440-light`, and `desktop-1440-compact`. Use query parameters already supported by the preload script, and keep mobile configurations unchanged.

- [x] **Step 3: Add deterministic dense-state captures**

  Seed a watchlist via `page.addInitScript`, intercept the screen/observe/backtest API calls with fixture-shaped JSON, trigger each panel's existing run action, and capture `screen-dense.png`, `observe-dense.png`, and `backtest-dense.png` at desktop size.

- [x] **Step 4: Add shortcut e2e assertions**

  Assert Ctrl+K focuses `[aria-label="搜索股票"]`, `2` changes the hash to `#sectionObserve`, `?` shows the shortcut dialog, Escape closes it, and `1` typed while the search input is focused does not navigate.

- [x] **Step 5: Wire harnesses into package and release checks**

  Add a non-screenshot `test:desktop` mode to the Playwright script so `npm test` can run shortcut checks without requiring baseline artifact churn. Add architecture and desktop harness invocations to `release-check.ps1`.

### Task 3: Final verification and visual QA

**Files:**
- Verify: all modified frontend and script files

- [x] **Step 1: Run all frontend tests**

  Run: `npm.cmd test`

  Expected: density guard, architecture guard, desktop e2e, and all Vitest files pass.

- [x] **Step 2: Run production build and bundle budget**

  Run: `npm.cmd run build`

  Expected: TypeScript/Vite build succeeds and entry gzip remains below 180KB.

- [x] **Step 3: Run the preview server and screenshot matrix**

  Run: `npm.cmd run preview -- --host 127.0.0.1`

  Run separately: `npm.cmd run ui:screenshots -- --url http://127.0.0.1:4173 --output C:\\tmp\\gp-assistant-desktop-polish`

  Expected: all routes render without framework overlays or horizontal overflow, desktop search/shortcuts work, mobile header remains uncluttered, and dense-state captures contain real rows/charts.

- [x] **Step 4: Inspect representative desktop and mobile screenshots**

  Inspect 1440 dark, 1440 light, 1440 compact, 2560 dark, and 390 mobile screenshots for overlap, clipping, unreadable text, scroll traps, and excessive line lengths.

- [x] **Step 5: Re-run clean verification after any visual fixes**

  Run: `npm.cmd test` and `npm.cmd run build`.
