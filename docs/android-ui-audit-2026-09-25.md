# Android UI audit and remediation log

Date: 2026-09-25
Scope: React/Tauri Android frontend and generated Android window integration.

## Standards

- `PRODUCT.md` and `DESIGN.md`
- Android touch and edge-to-edge window behavior
- WCAG AA contrast and non-color status communication
- Primary mobile target: 48px; secondary target: 44px; dense repeated controls: 32px

## Baseline evidence

- `npm run test:density`: passed before remediation.
- `npm run test:theme-parity`: passed before remediation.
- `npm run test:unit`: 67 files / 420 tests passed before remediation.
- `src/styles/responsive.css`: 982 lines at baseline and no Android platform-density selector overrides.
- Browser build and contrast audit initially hit local `spawn EPERM`; rerun with an approved process-launch environment is required.
- No Android device was connected at baseline; safe-area, system-bar, keyboard, Back, TalkBack, and high-DPR behavior remain device-validation items.

## Implemented in this change

- Added semantic touch tokens for primary, secondary, and dense controls.
- Kept compact density from reducing touch target semantics.
- Added safe-area tokens for all four edges and applied mobile header/content insets through CSS.
- Added Android-oriented font fallbacks: Noto Sans CJK SC, HarmonyOS Sans, and Roboto.
- Added contract tests for the semantic touch and safe-area model.
- Added static touch-target and color-contract audit scripts with focused Node tests.
- Strengthened light-theme action/status colors where token-level contrast was below the normal-text threshold.
- Changed the Android generator to keep the window edge-to-edge without applying a second top padding to the WebView content.
- Added cutout-safe, transparent system-bar theme attributes and generated Android window-controller configuration.

## Remaining verification

- Run all frontend unit, static, build, contrast, and screenshot checks.
- Run the Android generator twice and verify idempotent output.
- Build the Android APK.
- Validate 360/390/430 phone widths, 768 tablet, portrait/landscape, dark/light, 100%/130% font scale, keyboard open, gesture and three-button navigation.
- Complete manual TalkBack and Back-button flows.
- Add screenshots and any P0-P3 findings under `artifacts/android-ui-audit/` without replacing existing baselines.

## Acceptance gates

- No mobile primary control below 48px.
- No mobile secondary control below 44px.
- Dense 32px controls are not the only way to perform a critical action.
- No horizontal overflow in the audited matrix.
- Fixed chrome does not obscure scroll content.
- Normal text and semantic status colors meet 4.5:1 where used as text.
- Chart and non-text indicators meet 3:1 or include redundant labels/icons.
- No test result is marked as device-passed without actual Android evidence.

## Verification update (2026-09-27)

Passed after remediation:

- `npm run audit:touch-targets`
- `npm run audit:color-contract`
- `npm run test:touch-targets-contract` (6/6)
- `npm run test:color-contract` (6/6)
- `npm run test:density`
- `npm run test:theme-parity` (63 color tokens)
- `npm run test:unit` (68 files / 424 tests)
- `npm run build:app`
- `npm run test:contrast:built` (24 route/theme/viewport combinations)
- Android asset generator syntax validation
- Android asset generator idempotence check

The desktop screenshot harness still stops on an existing news empty-state baseline mismatch (`news/empty/desktop-1440-dark/news.png`, approximately 20.8% pixel difference). Existing baselines were intentionally not overwritten.

Android APK compilation is not verified on this machine because `npm run build:android` cannot locate `ANDROID_HOME`/Android SDK. Gradle compilation is therefore a remaining environment-gated check. No physical Android device or emulator was connected for TalkBack, Back, keyboard, safe-area, and system-navigation validation.

Android build verification update (2026-09-27):

- With the repository-local `.android-sdk` and NDK configured, the Rust Android release library compiled successfully and was linked into `jniLibs/arm64-v8a`.
- APK assembly then stopped during Gradle `:buildSrc` configuration with the opaque failure code `25`; no APK was produced. This is an Android/Gradle toolchain blocker, not a frontend TypeScript or Rust compilation failure.

## MuMu emulator verification update (2026-09-27)

Connected emulator:

- MuMu Android 15 guest, serial `127.0.0.1:16384`.
- `wm size`: `1440x2560`.
- `wm density`: `640`.
- System font scale: `1.0`.
- APK installed for this smoke run: `desktop/src-tauri/gen/android/app/build/outputs/apk/universal/release/guxuanyou_0.6.2_android_aarch64_release_signed.apk`.
- Important limitation: this APK is timestamped 2026-09-24 and is an older artifact; it is not evidence that the current unassembled source changes are packaged.

Captured evidence:

- `docs/android-ui-audit-2026-09-25/mumu-1440x2560/observe-nav.png`
- `docs/android-ui-audit-2026-09-25/mumu-1440x2560/observe-keyboard-open.png`

Observed pass evidence on the installed APK:

- App launches on Android 15 and remains in `com.tutict.stockoptimizer/.MainActivity`.
- Portrait app shell renders without a visible horizontal overflow at the 1440x2560 / 640-dpi emulator profile.
- Header, primary content cards, and five-item bottom navigation are visible together.
- Bottom navigation uses large icon/label targets and has a distinct selected state.
- Observe page exposes a clear empty state: stock code input, `开始观察` action, and explanatory text.
- Focused input receives a visible focus treatment and the soft keyboard opens.
- System status bar remains visible and does not cover the app header in the captured state.

Findings / not-yet-closed gates from this emulator run:

- The installed artifact is stale; current source-level Android changes require a fresh APK before packaging claims can be made.
- The MuMu window/UI accessibility tree exposes the emulator chrome rather than WebView semantics, so TalkBack semantics and DOM-level focus order were not independently proven by this channel.
- A first Back-key probe dismissed the IME (`mInputShown=false`), but the subsequent foreground state was not retained reliably during the emulator interaction; Back hierarchy therefore remains an open P1/P2 validation item until repeated against the fresh APK with a controlled sequence.
- Light theme, 130% font scale, landscape, cutout/gesture navigation, three-button navigation, and TalkBack remain unverified.
- The captured keyboard screenshot shows the focused field and primary action still visible in the viewport; the exact IME resize/inset behavior must be repeated on the fresh APK because this evidence comes from the stale artifact.

Current status: MuMu is connected and useful for device smoke validation, but Android Definition of Done remains `not complete` until a fresh APK is assembled and the remaining device matrix is rerun.

Additional MuMu matrix observations:

- 130% system font scale screenshot: `mumu-1440x2560/select-font-130-app.png`. Text remains readable and primary action remains visible, but the horizontal tab row clips the rightmost `自定义选股` label. Treat as P1 for the large-font matrix; do not close until the fresh APK proves wrapping/scrolling behavior.
- Landscape screenshot: `mumu-1440x2560/select-landscape.png`. The left navigation rail, header, refresh card, tab row, and primary run action render without visible horizontal overflow in the captured viewport. Lower content continues below the fold and still requires scroll/keyboard coverage.
- Emulator settings were restored to portrait (`user_rotation=0`) and font scale `1.0` after the matrix probe.

Fresh APK build attempts after MuMu connection:

- Using Java 24 and repository-local Android SDK/NDK, Gradle reached `:app:rustBuildArm64Release` but the Tauri mobile helper failed because its WebSocket endpoint was reset/refused (`10054`/`10061`).
- A temporary Windows `npm.cmd.bat` wrapper was used only to diagnose the generated Gradle BuildTask's inability to launch `npm.cmd`; it was removed after the attempt and is not part of the product changes.
- A clean local Gradle cache attempt was blocked by a network timeout while resolving `kotlin-compiler-embeddable:2.0.21` from GitHub.
- No fresh APK was produced. Existing MuMu screenshots must therefore remain explicitly marked as stale-artifact smoke evidence.

Verification rerun after MuMu evidence capture (2026-09-27):

- `npm.cmd run audit:touch-targets`: passed.
- `npm.cmd run audit:color-contract`: passed.
- `npm.cmd run test:unit`: passed, 68 files / 424 tests.
- `npm.cmd run test:density`: passed, 11 stylesheets.
- `npm.cmd run test:theme-parity`: passed, 63 color tokens.
- `git diff --check`: passed; only Git's normal CRLF conversion warnings were emitted.
- The first sandboxed Vitest attempt reproduced the known Vite `spawn EPERM`; the same test passed when rerun in the approved process-launch environment.

Follow-up remediation from MuMu 130% font-scale evidence:

- Updated mobile `.panel-tabs` to use a full-width, non-wrapping horizontal scroller with hidden scrollbar and `white-space: nowrap` tab labels. This prevents large-font labels from being clipped while preserving the 48px primary touch target.
- Re-ran `npm.cmd run build:app` successfully and `npm.cmd run prepare:android` successfully; the generated `desktop/mobile-dist` now contains the updated CSS bundle.
- Re-ran unit tests after the fix: 68 files / 424 tests passed.

## Final MuMu verification after fresh APK rebuild (2026-09-27)

The current source was rebuilt into a fresh Android APK, signed with the local debug keystore for emulator-only validation, installed on MuMu Android 15, and launched successfully.

Fresh artifact facts:

- Build command: `npm.cmd run build:android`.
- Frontend build, bundle budget, Rust Android release compilation, Gradle APK assembly: passed.
- Fresh unsigned APK output: `desktop/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`.
- Emulator validation APK: `guxuanyou_0.6.2_android_aarch64_release_current_scrollfix_debug_signed.apk` in the same output directory.
- MuMu serial: `127.0.0.1:16384`.
- Android 15, 1440x2560, density 640, font scale restored to 1.0, portrait restored after testing.
- Package update time observed: 2026-09-27 14:00:20.

Fresh evidence:

- `mumu-1440x2560/fresh/observe-scrollfix.png`
- `mumu-1440x2560/fresh/observe-dark-final.png`
- `mumu-1440x2560/fresh/observe-light-theme.png`
- `mumu-1440x2560/fresh/observe-keyboard-fixed.png`
- `mumu-1440x2560/fresh/observe-landscape-final.png`
- `mumu-1440x2560/fresh/select-font-130-fixed.png`
- `mumu-1440x2560/fresh/select-font-130-tabs-swipe2.png`
- `mumu-1440x2560/fresh/select-safearea-fixed.png`

Verified on the fresh APK:

- Status-bar inset no longer overlays the header/logo. The generated Android Insets bridge now uses status/navigation-bar insets ignoring visibility and reapplies them after WebView startup.
- Switching from Select to Observe resets the page scroll position; the Observe disclaimer is fully visible below the header instead of being clipped by sticky chrome.
- Light and dark themes update both Web UI and system-bar icon appearance without a visible status-bar color discontinuity.
- The 130% font-scale matrix keeps text readable; the dense Select tabs can be horizontally swiped to reveal the full tab label set, including `自定义选股` and `趋势选股`.
- Keyboard focus produces a visible focus treatment and `mInputShown=true`; Android Back dismisses the IME and returns `mInputShown=false`.
- Portrait and landscape layouts render the header, navigation rail/bottom navigation, primary inputs, and main action without horizontal overflow in the captured states.
- No fatal app/runtime error was found in the post-install log filter.

Code changes from the final device findings:

- `scripts/prepare-tauri-android-assets.ps1`: robust WebView safe-area injection and delayed inset reapplication.
- `desktop/frontend/src/App.tsx`: reset window/workbench scroll on view changes.
- `desktop/frontend/src/styles/responsive.css`: mobile tabs use a non-wrapping horizontal scroller while retaining the touch target contract.

Remaining unverified gates:

- TalkBack semantic traversal was not proven because MuMu's desktop accessibility tree exposes the emulator shell rather than WebView DOM semantics.
- Physical cutout/true gesture-navigation variants, three-button navigation, 130% font scale on every page, network loss/recovery, and full Agent/News/Backtest journey still require dedicated matrix runs.
- The Android Definition of Done remains conditional on those remaining accessibility and full-journey checks; the fresh APK and MuMu smoke gates are no longer blocked by Android SDK/Gradle assembly.

## Continuation verification update (2026-09-28)

### Accessibility / TalkBack capability check

- MuMu ADB reports no TalkBack/accessibility package installed.
- `settings get secure accessibility_enabled`: `0`.
- `settings get secure enabled_accessibility_services`: `null`.
- `uiautomator dump` exposes only the native WebView container (`android.webkit.WebView`, `NAF=true`), not the internal React DOM nodes. Therefore a TalkBack traversal cannot be honestly marked passed on this emulator. It remains blocked by missing screen-reader runtime/bridge evidence, not by an app crash.

### System navigation check

- MuMu `navigation_mode` reported `0`; the app was tested with the emulator's current navigation configuration and Android Back/IME behavior.
- A separate true gesture-navigation and three-button visual comparison could not be established because the MuMu guest does not expose a resolvable Android Settings activity through ADB in this configuration.
- The app's CSS/Android bridge remains configured for edge-to-edge with safe-area variables; navigation-mode-specific evidence is still pending.

### Full journey smoke evidence on the fresh APK

Fresh APK was installed and the five primary workspaces were visited in sequence:

- Select: `mumu-1440x2560/journey-00-select.png`
- Observe: `mumu-1440x2560/journey-01-observe.png`
- Backtest: `mumu-1440x2560/journey-02-backtest.png`
- News: `mumu-1440x2560/journey-03-news.png`
- Agent: `mumu-1440x2560/journey-04-agent.png`

The workspaces launched and navigation state changed correctly. The screenshots also revealed that the light theme persisted during that smoke pass, so dark-theme journey evidence is kept separately in the fresh dark screenshots.

### 130% font matrix

Initial 130% screenshots exposed clipping in the News header/tool row and Agent header controls. CSS was updated so:

- Research actions wrap/reflow and horizontally scroll instead of clipping.
- Sentiment toolbar can scroll horizontally.
- Agent mobile menu/new/history buttons are at least 44px and the toolbar reserves space for them.

Static audits, 68 unit-test files / 424 tests, and production build passed after these fixes. A new Android build was assembled after the fixes; emulator installation and final 130% screenshots were captured as:

- `font130-news-fixed.png`
- `font130-agent-fixed.png`

These show the main toolbar and Agent controls are no longer clipped at the right edge; remaining lower content still requires page-specific scrolling checks.

### Network interruption capability check

A controlled network disconnect/recovery pass was not completed: MuMu is using a host-managed network path and no safe, guest-local Wi-Fi toggle was available through the exposed ADB/settings surface. This is explicitly `not verified`, not a pass.

### Cutout/notch capability check

No physical notch/cutout profile is available in the current MuMu guest. The Android generator contains `windowLayoutInDisplayCutoutMode=shortEdges` and the WebView safe-area bridge, but a true cutout screenshot remains `not verified`.

Current conclusion: the current MuMu run closes fresh-APK workspace, Back/IME, portrait/landscape, light/dark, and 130% smoke evidence for the available guest. TalkBack, true notch, separate gesture-vs-three-button comparison, and controlled network interruption remain explicitly open due to emulator capability limits.

### Network interruption update (2026-09-28)

- MuMu guest baseline had `airplane_mode_on=0`, `wifi_on=1`, and a validated Wi-Fi network.
- ADB airplane-mode toggle was attempted, but the guest retained a validated `wlan0` network after `airplane_mode_on=1`; therefore this is not trustworthy evidence of an actual offline state.
- Captured `network-off-observe.png` is retained only as a UI smoke screenshot; it must not be interpreted as a successful offline/recovery test.
- Airplane mode was restored to `0`; the guest returned to its normal connected state.
- Offline/recovery remains `not verified` until the emulator's network bridge can be disabled at the host/MuMu level or a real device can be tested.

### Follow-up code verification (2026-09-28)

After the 130% News/Agent findings:

- `desktop/frontend/src/styles/research.css` now reflows the News research topbar and action row and allows horizontal scrolling for the sentiment toolbar.
- `desktop/frontend/src/styles/responsive.css` now reserves space for Agent mobile menu/new/history controls and raises those controls to the secondary touch target.
- `npm.cmd run audit:touch-targets`: passed.
- `npm.cmd run audit:color-contract`: passed.
- `npm.cmd run test:unit`: passed, 68 files / 424 tests.
- `npm.cmd run build:app`: passed.
- `npm.cmd run prepare:android`: passed.
- `npm.cmd run build:android`: passed; a fresh emulator-signed APK was installed on MuMu.
- Fresh 130% evidence: `font130-news-fixed.png`, `font130-agent-fixed.png`.

The 130% screenshots show the primary News and Agent controls reflowing without the earlier right-edge clipping. Page-specific content still requires scrolling through the full matrix; no claim of universal 130% page completion is made.

## Complete 130% scroll-matrix pass (2026-09-28)

A 30-screenshot matrix was captured on MuMu Android 15 at 1440x2560 / 640 dpi / 130% system font:

- `scroll-matrix/select-top.png` + `select-scroll-1..5.png`
- `scroll-matrix/observe-top.png` + `observe-scroll-1..5.png`
- `scroll-matrix/backtest-top.png` + `backtest-scroll-1..5.png`
- `scroll-matrix/news-top.png` + `news-scroll-1..5.png`
- `scroll-matrix/agent-top.png` + `agent-scroll-1..5.png`

The matrix used five repeated upward swipes per workspace and restored system font scale to 100% afterward.

Results:

- Select: no crash; fixed bottom navigation remained present. The visible empty/result card remains scrollable, but the current data-empty state did not expose additional result rows.
- Observe: no crash; input, primary action, empty-state card, and watchlist card remained usable during scrolling.
- Backtest: no crash; parameter controls and lower result area were traversable in the captured sequence.
- News: no crash; research header, toolbar, daily brief, event stream, and composer remained present. At 130%, the research toolbar still presents a horizontally scrollable control row rather than claiming all controls fit simultaneously.
- Agent: no crash; the empty conversation state and composer remained stable. The fixed mobile action cluster remained visible during the captured swipe sequence.

This closes the requested *capture-and-review scroll matrix* for the available MuMu guest. It does not close data-dependent states that require populated watchlists, loaded backtest results, news evidence, sentiment evidence drawers, or a running Agent tool call.
