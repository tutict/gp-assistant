# 桌面端前端优化方案（视觉 / 交互 / 性能 / 防回归 harness）

> 适用范围：`desktop/frontend/` 在 Windows（WebView2）下的桌面体验。
> 前置状态：Android 密度治理已完成（responsive.css 5921→1027 行，token 层、密度守卫、截图 harness 已就位）。本文档只覆盖**桌面端特有**问题，不重复移动端内容。

---

## 1. 现状盘点

### 1.1 已经做对的部分（不要动）

- Token 层完整：`--fs-*` 字号阶梯、`--touch-*`、间距/圆角/动效/z-index 全部变量化。
- **更正（2026-08-07）**：此前"全样式表无硬编码 px 字号"的结论有误（当时 grep 模式问题）。实测存在 196 处硬编码 px 字号、14 种取值，约半数不在 token 阶梯（10/10.5/11.5/12.5/13.5/15/16/18/19px），重灾区为观察摘要模块。治理方案见 `docs/observe-summary-ui-plan.md`。
- Cascade layers 已引入（tokens → base → components → shell → pages → responsive）。
- 工程 harness 在运转：`check-ui-density.mjs`、`ui:screenshots`（desktop-1440 基线已有）、32 个 vitest 测试文件。
- 窗口配置合理：1280×860 启动、960×680 下限、`visible(false)` + 页面加载完成后再显示（无白屏闪）。
- K 线为单 `<svg>` viewBox 渲染（非每根 K 线一个 DOM），组件层有 80 处 memo/useMemo/useCallback。

### 1.2 桌面端特有问题清单（按严重度）

| # | 问题 | 证据 | 影响 |
| --- | --- | --- | --- |
| P0-1 | **全局隐藏所有滚动条** | `global.css` L10–35：`* { scrollbar-width: none }` + `::-webkit-scrollbar { display:none }` | 鼠标用户失去滚动位置感知、无法拖拽滚动条；桌面可用性最直接的伤 |
| P0-2 | **research.css 未声明 layer** | `global.css` L8：`@import "./research.css";`（其余 6 个文件均有 layer） | 未分层样式优先级**高于全部 layered 样式**，1635 行 research.css 可以覆盖任何 token/base/components 规则——cascade layers 架构形同虚设 |
| P0-3 | **base.css 与 shell.css 双层定义壳层** | 两者都定义 `.app`（`220px 1fr` vs `var(--sidebar-width)=208px`）、`.app-header`（padding 8px 16px + color-mix 背景 vs 0 var(--space-4) + surface 背景）等 | base 层的壳层定义是被 shell 层压住的死代码，取值已漂移，是"双层覆盖"反模式 |
| P1-4 | **跨文件选择器重复 17.6%** | 脚本统计：1397 个唯一选择器中 246 个在多个文件定义；最大对 pages↔responsive（72）、components↔pages（24） | 改一处漏一处，Android 时代的补丁温床尚未根除 |
| P1-5 | **字体全部依赖系统回退** | 无 woff/ttf 资源；栈首 Inter（Windows 默认无）→ 实际渲染落到微软雅黑；DESIGN.md 指定 Fira Sans/Fira Code 与 tokens.css 的 Inter 互相矛盾；数据等宽体 Fira Code 未装 → Consolas | 拉丁字符/数字字形欠佳；规格与实现不一致；跨机器观感不可控 |
| P1-6 | **宽屏无约束** | `.workbench { padding: 18px 22px }`，无 max-width；1440px 截图中表单/控制条已拉满全宽 | 1920/2560 下表格、输入框拉成超长行，阅读视线跳跃大 |
| P1-7 | **图表指示线颜色漂移** | 74 个硬编码 hex 中：3 种蓝（#4e9dff/#54b7ff/#4a90e2）、2 种琥珀（#f59d38/#f5a623）、3 种紫（#c36cff/#d95cff/#b06ae0） | MACD/KDJ/均线在不同图表里颜色不一致；且绕过明暗主题 token |
| P1-8 | **加载态只有 spinner + 文本** | `.panel-feedback-loading` 单一形态 | 数据密集页面（观察/K 线/回测）白屏感长 |
| P2-9 | **单 bundle 434KB 无分包** | `mobile-dist/assets/index-*.js` 434KB；6 个面板全部首屏 eager 加载；`index.html` 用 `<script src="/vendor/jsQR.js">` 全局加载 jsQR（仅扫码场景使用） | 首屏解析成本；移动端也白付这个成本 |
| P2-10 | **无 bundle 预算与分析** | vite.config 无 manualChunks/visualizer | 体积回退无警报 |
| P3-11 | **键盘生产力缺失** | 全应用仅 ESC、K 线方向键、对话框 ESC | 无 Ctrl+K 全局搜股、无视图切换键、无 `?` 帮助——桌面投研工具的标配 |
| P3-12 | **无信息密度档位** | 只有一套密度 | 桌面投研用户密集阅读需求强，应提供舒适/紧凑两档 |
| P3-13 | **Header 大量留白** | 仅 logo + 主题开关 | 可承载全局搜索与数据状态（股票池/更新日期已有数据，散在页面内） |

---

## 2. 优化目标（验收硬指标）

| 指标 | 目标 |
| --- | --- |
| 桌面滚动条 | 恢复可见（细美化样式），移动端保持隐藏 |
| Cascade layers | 全部样式文件入 layer；`scripts/check-css-architecture.mjs` 守卫通过 |
| 跨文件选择器重复率 | ≤ 5%（排除 @media 块内的合法覆写） |
| 字体 | 拉丁/数字渲染不依赖"用户机器恰好装了 Inter"；DESIGN.md 与实现一致 |
| 宽屏 | 1920px 下内容行宽 ≤ 可读阈值（表单区 ≤ 1200px，长文本区 ≤ 76ch） |
| Bundle | 入口 chunk gzip 后预算 ≤ 180KB；jsQR 按需加载 |
| 键盘 | Ctrl+K 搜索、1–5 切视图、? 帮助面板可用 |
| 密度 | 舒适/紧凑两档，localStorage 持久化，默认舒适 |

---

## 3. 分阶段方案

### Phase 1：架构止损（0.5–1 天，风险低，先做）

**1.1 恢复桌面滚动条（global.css 改写）**

```css
/* global.css —— 移动端隐藏，桌面恢复细滚动条 */
* { scrollbar-width: thin; scrollbar-color: var(--surface-3) transparent; }
*::-webkit-scrollbar { width: 8px; height: 8px; }
*::-webkit-scrollbar-thumb { background: var(--surface-3); border-radius: 4px; }
*::-webkit-scrollbar-thumb:hover { background: var(--line); }
*::-webkit-scrollbar-track { background: transparent; }

@media (max-width: 768px) {
  * { scrollbar-width: none; }
  *::-webkit-scrollbar { display: none; }
}
/* 例外：panel-tabs 等横向滚条组件保持全端隐藏（现有规则保留） */
```

**1.2 research.css 入 layer**

```css
/* global.css L8 改为： */
@import "./research.css" layer(pages);   /* research 属于页面层语义 */
```

同时清理 research.css 内依赖"未分层高优先级"才能赢的规则（入层后被压掉的样式要显式提升 specificity 或修正层内顺序）——用 desktop-1440 截图基线逐页比对。

**1.3 合并 base.css 与 shell.css 的壳层双定义**

- base.css 只保留 reset（box-sizing、body、focus-visible、disabled、a、icon-button）；
- 删除 base.css 中 `/* --- Layout --- */` 起的 `.app`/`.app-header`/`.sidebar`/`.workbench` 等全部壳层定义（shell.css 已是权威版本，且值更新）；
- 验证：删除前后 desktop-1440 六页截图像素级比对应无变化（base 层本来就被 shell 层压住）。

**1.4 跨文件去重（渐进）**

先跑 3.5 的架构守卫拿到全量重复清单，按"定义归属单一文件"原则迁移：组件类归 components.css、页面类归 pages.css、壳层归 shell.css；responsive.css 只保留 @media 块内的合法覆写。本轮把 17.6% 降到 ≤5%，不必追求 0。

### Phase 2：视觉与排版规范（1 天）

**2.1 字体落地（三选一，推荐 A）**

- **A. 子集化自托管（推荐）**：打包 Inter var 拉丁子集（~90KB woff2）+ JetBrains Mono 子集（~60KB）放 `public/fonts/`，`@font-face` 声明，CJK 继续走系统栈（Noto Sans SC/雅黑）。跨机器一致，CSP `font-src 'self'` 已允许。
- B. 纯系统栈：栈首改 `"Segoe UI"`（Windows 原生），放弃 Inter/Fira；同时把 DESIGN.md 字体节改为与实现一致。
- C. 维持现状但修文档：不可控，不推荐。

同步动作：`.metric strong`、`quote-number`、`stock-code` 等数据位指定 mono 栈；更新 DESIGN.md typography 节与实现对齐（规格跟着实现走，或实现跟着规格走，二选一，不能两张皮）。

**2.2 宽屏约束**

```css
/* shell.css */
.workbench { padding: 18px 22px 44px; }
@media (min-width: 1600px) {
  .workbench { padding-inline: max(22px, calc((100vw - 1560px) / 2)); }
}
/* 表单/控制条不随视口无限拉伸 */
.panel-controls, .backtest-controls, .rag-controls { max-width: 1560px; }
/* 长文本证据区限制行宽 */
.evidence-list p, .agent-final-reply, .notes p { max-width: 76ch; }
```

观察页桌面双栏（可选增强）：摘要/基本面左栏（minmax 380px）+ K 线右栏弹性，`@media (min-width: 1280px)` 启用。

**2.3 图表 palette 收编**

在 tokens.css 增加图表语义色并替换全部硬编码：

```css
:root {
  --chart-line-1: #4e9dff;  /* 统一蓝：替换 #54b7ff/#4a90e2 */
  --chart-line-2: #f59d38;  /* 统一琥珀：替换 #f5a623 */
  --chart-line-3: #c36cff;  /* 统一紫：替换 #d95cff/#b06ae0 */
  --chart-up: var(--rise); --chart-down: var(--fall);
}
[data-theme="light"] { --chart-line-1: #1f6fd6; /* ...浅色系映射 */ }
```

`chartPalette.ts` 改为从 CSS 变量读取（或保持 TS 常量但由守卫脚本比对一致性）。

**2.4 骨架屏**

为观察页摘要区、K 线卡、回测结果区加 `.skeleton` 基元（`linear-gradient` 扫光 + `prefers-reduced-motion` 降级），替换数据密集区的纯 spinner：

```css
.skeleton { border-radius: var(--radius-sm); background: var(--surface-2);
  background-image: linear-gradient(100deg, transparent 30%, var(--surface-3) 50%, transparent 70%);
  background-size: 200% 100%; animation: skeleton-sweep 1.4s infinite; }
```

### Phase 3：性能与工程（0.5 天）

**3.1 分包与懒加载**

```ts
// vite.config.ts
build: {
  rollupOptions: { output: { manualChunks: {
    react: ["react", "react-dom"],
    icons: ["lucide-react"],
  } } },
}
```

```tsx
// App.tsx —— 面板级懒加载（默认只加载选股页）
const ObservePanel = lazy(() => import("./components/panels/ObservePanel"));
const BacktestPanel = lazy(() => import("./components/panels/BacktestPanel"));
const AgentPanel = lazy(() => import("./components/panels/AgentPanel"));
const NewsRagPanel = lazy(() => import("./components/panels/NewsRagPanel"));
// 包 <Suspense fallback={<PanelFeedback loading />}>；hash 切换时预取相邻面板
```

**3.2 jsQR 动态化**：删除 `index.html` 的 `<script src="/vendor/jsQR.js">`，在扫码功能入口 `const jsQR = (await import("../vendor/jsQR")).default`（或改为动态 `<script>` 注入 helper）。

**3.3 bundle 预算**：`npm run build` 输出接入 3.5 的守卫——入口 chunk gzip > 180KB 告警、> 220KB 失败。

### Phase 4：桌面生产力（1 天，可分两次迭代）

**4.1 全局搜索（Ctrl+K）**

- Header 中央放搜索框（复用 `StockCodeInput` 的联想逻辑），`Ctrl+K` / `/` 聚焦；
- 选中结果直接跳转观察页并载入该股票；
- 这是把"股票代码输入"从各面板重复控件提升为全局能力。

**4.2 快捷键体系**

| 键位 | 动作 |
| --- | --- |
| `Ctrl+K` / `/` | 聚焦全局搜索 |
| `1`–`5` | 切换 选股/观察/回测/消息/Agent（输入框聚焦时不触发） |
| `?` | 快捷键帮助浮层 |
| `Esc` | 关闭浮层/失焦（已有，保持） |

实现：单个 `useGlobalShortcuts` hook，集中在 App.tsx 注册，含 `e.target`  editable 判断。

**4.3 密度档位**

```css
:root[data-density="compact"] {
  --fs-body: 13px; --fs-data: 12px; --fs-label: 11px;
  --control-height: 34px; --touch-comfort: 36px;
  --space-3: 10px; --space-4: 12px;
}
```

Header 加密度切换（舒适/紧凑），`localStorage` 持久化。复用 token 的好处：只需覆盖变量，零新选择器。

**4.4 Header 数据状态条**：股票池数量 / 数据日期 / 缓存新鲜度（数据已存在 `screen-toolbar` 里）提炼到 Header 右侧，页面内重复显示可删。

### Phase 5：防回归 harness 扩展（0.5 天，持续运转）

在现有 density 守卫与截图 harness 基础上，新增/扩展三个检查，全部接入 `npm test` 与 `release-check.ps1`：

**5.1 `scripts/check-css-architecture.mjs`（新增）**

```js
// 三类规则：
// 1) global.css 中每个 @import 必须声明 layer()（防 P0-2 复发）
// 2) 跨文件选择器重复率 > 5% 失败（排除 @media 块内覆写与 tokens.css）
// 3) hex 颜色白名单：tokens.css 之外只允许 var()/chart 语义色，
//    出现 #4a90e2 等已收编色值直接失败（防 P1-7 复发）
// 4) 桌面 @media (min-width: 769px) 作用域外禁止 scrollbar-width: none（防 P0-1 复发）
```

**5.2 截图矩阵扩展**：`ui-screenshot.mjs` 增加 `desktop-1920`、`desktop-2560`、`desktop-1440-light`（`?theme=light` 已支持）、`desktop-1440-compact`（密度档），共 10 基线；空态之外增加"注入 mock 数据后的数据密集态"截图（K 线/榜单/回测结果），空态截图发现不了排版问题。

**5.3 快捷键 e2e（Playwright）**：Ctrl+K 聚焦、1–5 切换 hash、`?` 开帮助、输入框内按 1 不切换——四条用例进 `ui-screenshot.mjs` 同套预览服务。

**5.4 现有密度守卫扩展**：`check-ui-density.mjs` 增加"desktop 规则"段：禁止 `min-width: 1600px` 媒体块内出现 `max-width` 表单约束的删除（即 2.2 的约束不得被后续 PR 移除）——用契约测试锁定宽屏约束。

---

## 4. 执行顺序与检查点

| 步骤 | 内容 | 验证 |
| --- | --- | --- |
| 1 | global.css 滚动条 + research.css 入 layer | desktop-1440 六页截图比对 |
| 2 | base.css 壳层删除 | 截图应零差异（本就死代码） |
| 3 | check-css-architecture.mjs 上线（先 warn 模式） | 拿到全量重复清单 |
| 4 | 跨文件去重至 ≤5% | 守卫转 fail 模式 |
| 5 | 字体子集化 + mono 数据栈 + DESIGN.md 对齐 | 数字列字形截图 |
| 6 | 宽屏约束 + palette 收编 + 骨架屏 | 1920/2560/浅色系新基线 |
| 7 | manualChunks + 懒加载 + jsQR 动态化 + bundle 预算 | 构建报告 + 预算通过 |
| 8 | Ctrl+K / 快捷键 / 密度档 / Header 状态条 | Playwright e2e 四条 |
| 9 | 截图矩阵扩展 + 数据密集态基线 | 10+ 基线入库 |

每步独立 commit，分支 `feat/desktop-frontend-polish`；截图比对贯穿全程。

---

## 5. 风险与注意

- **research.css 入 layer 是唯一可能"改崩"的步骤**：1635 行里若有依赖未分层优先级的规则，入层后会被压掉。务必先截图基线、逐页比对、必要时显式提 specificity。
- 懒加载面板后，Tauri IPC 调用集中在面板内部，注意 `Suspense` fallback 期间不要触发 invoke。
- WebView2 已知坑（base.css 注释已记录）：不要用 `backdrop-filter: blur()`，本次所有新增样式同样避开。
- 密度档位只覆盖 token 变量即可，**不要**为 compact 档新增成套选择器——那是 Android 时代的覆辙。
