---
name: 股选优
description: A professional A-share research workbench for screening, evidence, observation, and backtesting.
colors:
  primary: "#48BFD2"
  primary-deep: "#0F7B8A"
  accent: "#D97706"
  background-dark: "#0D1116"
  surface-dark: "#151B22"
  surface-raised-dark: "#1D252D"
  text-dark: "#F2F7FA"
  muted-dark: "#9BAAB7"
  border-dark: "#2B3844"
  background-light: "#F6F8FB"
  surface-light: "#FFFFFF"
  text-light: "#16212B"
  muted-light: "#5F6F7D"
  rise: "#F25D52"
  fall: "#35B779"
  warning: "#D97706"
typography:
  display:
    fontFamily: "Fira Sans, Inter, system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 700
    lineHeight: 1.15
    letterSpacing: "0"
  title:
    fontFamily: "Fira Sans, Inter, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 700
    lineHeight: 1.25
    letterSpacing: "0"
  body:
    fontFamily: "Fira Sans, Inter, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.55
    letterSpacing: "0"
  label:
    fontFamily: "Fira Sans, Inter, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 700
    lineHeight: 1.3
    letterSpacing: "0"
  data:
    fontFamily: "Fira Code, ui-monospace, SFMono-Regular, Consolas, monospace"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: 1.35
    letterSpacing: "0"
rounded:
  sm: "4px"
  md: "8px"
  lg: "10px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.primary-deep}"
    textColor: "#FFFFFF"
    rounded: "{rounded.sm}"
    padding: "10px 14px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.primary}"
    rounded: "{rounded.sm}"
    padding: "9px 12px"
  input:
    backgroundColor: "{colors.surface-dark}"
    textColor: "{colors.text-dark}"
    rounded: "{rounded.sm}"
    padding: "10px 12px"
---

# Design System: 股选优

## 1. Overview

**Creative North Star: "Research Desk Terminal"**

The interface should feel like a focused research desk: quiet, structured, and built around evidence. It uses a restrained dark-first workbench, strong text contrast, compact controls, and a single cyan action color so data and status remain louder than decoration.

The system rejects marketing-page dramatics, generic finance neon, and noisy trading-software clutter. The design earns trust through consistent controls, visible states, and disciplined grouping.

**Key Characteristics:**
- Dense product UI with stable navigation, filters, result panels, and evidence blocks.
- Dark-first palette with a light mode equivalent, semantic rise/fall/warning colors, and no decorative gradients.
- 4px to 10px radii, thin borders, tonal layering, and shadows only for sticky or elevated overlays.
- Single sans family for UI, monospace only for codes, values, timestamps, and tabular data.

## 2. Colors

The palette is graphite surfaces, exchange cyan actions, amber caution, and semantic red/green market states.

### Primary
- **Exchange Cyan**: used for primary actions, selected navigation, focus rings, and active stream/status markers.
- **Deep Exchange Cyan**: used for primary button fills and high-emphasis selected states.

### Secondary
- **Audit Amber**: used for warnings, incomplete data, stale cache, and settings that require review.

### Neutral
- **Graphite Background**: the default dark canvas for long research sessions.
- **Panel Graphite**: main panels, drawers, cards, result blocks, and form surfaces.
- **Raised Graphite**: sticky headers, selected panels, popovers, and focused results.
- **Ink Text**: primary reading text on dark surfaces.
- **Muted Steel**: secondary labels, timestamps, helper text, and descriptions.
- **Workbench Line**: separators and component borders.

### Named Rules

**The One Action Color Rule.** Cyan is reserved for action, selection, focus, and progress. It is not decoration.

**The Semantic Market Rule.** Red and green only describe market direction or positive/negative evidence, and must be paired with text.

## 3. Typography

**Display Font:** Fira Sans with Inter and system fallbacks  
**Body Font:** Fira Sans with Inter and system fallbacks  
**Label/Mono Font:** Fira Code for stock codes, values, timestamps, and tabular data

**Character:** The type system is technical without becoming terminal cosplay. Labels are compact and firm; prose is readable; data uses tabular figures and monospace rhythm.

### Hierarchy
- **Display** (700, 24px, 1.15): app title and major page headings only.
- **Headline** (700, 20px, 1.2): primary module headings and drawer titles.
- **Title** (700, 18px, 1.25): panel headings, result groups, and card titles.
- **Body** (400, 14px, 1.55): explanations, evidence summaries, and helper copy.
- **Label** (700, 12px, 1.3): form labels, navigation captions, badges, and metadata.
- **Data** (500, 13px, 1.35): stock codes, prices, timestamps, percentages, and compact metrics.

### Named Rules

**The No Display Labels Rule.** UI labels, buttons, and data never use display styling. Large type is for orientation, not routine controls.

## 4. Elevation

Depth is conveyed mostly by tonal layering and thin borders. Shadows are structural: sticky headers, floating drawers, mobile sheets, and open menus may use a defined shadow; ordinary panels stay flat.

### Shadow Vocabulary
- **Sticky Header** (`0 14px 34px rgba(3, 8, 13, 0.28)`): top app header and fixed mobile chrome.
- **Floating Sheet** (`0 24px 60px rgba(3, 8, 13, 0.38)`): mobile criteria drawer, menus, and modal-like overlays.
- **Raised Result** (`0 12px 28px rgba(3, 8, 13, 0.22)`): selected assistant/result details only.

### Named Rules

**The Flat-at-Rest Rule.** Routine modules do not use decorative shadow. Use border, tone, and spacing first.

## 5. Components

### Buttons
- **Shape:** compact rectangle with precise corners (4px).
- **Primary:** Deep Exchange Cyan fill, white text, 10px 14px padding, 44px minimum touch height on mobile.
- **Hover / Focus:** hover deepens the fill; focus uses a visible cyan ring; active state darkens without shifting layout.
- **Ghost:** transparent surface with Workbench Line border and cyan text for secondary commands.

### Chips
- **Style:** muted tonal background, thin border, label-weight text.
- **State:** selected chips use cyan text and a stronger border; unselected chips stay neutral.

### Cards / Containers
- **Corner Style:** restrained rounded corners (8px to 10px).
- **Background:** Graphite or Raised Graphite depending on hierarchy.
- **Shadow Strategy:** no shadow at rest; use shadow only for selected, floating, or sticky contexts.
- **Border:** 1px Workbench Line, never a thick side stripe.
- **Internal Padding:** 12px to 16px for dense panels, 20px for large result modules.

### Inputs / Fields
- **Style:** dark tonal fill, thin border, 4px radius, visible label.
- **Focus:** border shifts to cyan and receives a soft focus ring.
- **Error / Disabled:** error uses text plus semantic color; disabled states reduce opacity and remove pointer affordance.

### Navigation
- **Style:** persistent sidebar on desktop, bottom navigation on mobile, both with label and icon. Active state uses cyan text and a tonal background. Navigation never hides primary work surfaces.

### Result Panels
- **Style:** dense evidence-first content, consistent metric rows, explicit empty/loading/error states.
- **State:** loading shows skeleton or progress text; errors include source and recovery action where available.

## 6. Do's and Don'ts

### Do:
- **Do** keep the current conditions and data source visible near the working surface.
- **Do** use cyan only for action, selection, focus, and progress.
- **Do** pair red/green market colors with text labels, arrows, or context.
- **Do** keep controls at 44px minimum touch target on mobile.
- **Do** preserve reduced-motion support and use motion only for state changes.

### Don't:
- **Don't** build marketing-style hero sections, decorative metric strips, or sales copy.
- **Don't** use purple-blue gradients, glassmorphism, glowing blobs, or ornamental background effects.
- **Don't** use thick side-stripe borders on cards or alerts.
- **Don't** rely on red/green alone to convey meaning.
- **Don't** write investment-advice language such as “必涨”, “稳赢”, “立即买入”, “目标价”, or “满仓”.
