# UI Density Rules

The frontend uses CSS media queries and shared tokens as its only layout-density
mechanism. Runtime detection may select behavior or IPC paths, but it must not
select typography, spacing, or control sizes.

## Required Tokens

- Body, data, label, and caption text use the `--fs-*` scale in
  `src/styles/tokens.css`.
- Primary controls use `--touch-comfort` (44px).
- Repeated dense controls may use `--touch-dense` (32px).
- Mobile navigation uses `--nav-height` plus `--safe-bottom`.

## Prohibited Patterns

- Platform density selectors such as `.android-phone` or `.mobile-tauri`.
- Setting `font-size` on `html`.
- Using `text-size-adjust` for zooming. Only `100%` is allowed.
- Literal font sizes below 10px.
- Decorative glow shadows on primary actions.

Use `npm run test:density` before committing CSS changes. Use
`npm run ui:screenshots -- --url http://127.0.0.1:4173` after starting
`npm run preview` to capture the 360px, 390px, 430px, 768px, and 1440px
mobile/desktop matrix plus its diagnostics report.
