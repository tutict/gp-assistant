# 选股筛选表单 UI 治理方案（智能选股·高级过滤 + 自定义选股·基础范围）

> 对象：`CriteriaFields.tsx`（两处使用：智能选股的 `.adaptive-advanced` 高级过滤折叠区、自定义选股的 `.custom-screen-criteria` 常驻区）及其容器样式。
> 本次以**桌面端**为主，移动端约束同步补齐。沿用既有 harness 体系（density 守卫 / css-architecture 守卫 / 截图基线 / 契约测试）。

---

## 1. 诊断：这次的根因是"类写了，样式没写"

### 1.1 零样式类（截图 1 高瘦单列的直接原因）

以下 className 出现在组件中，但在**全部 8 个样式文件里 0 条规则**：

| className | 出现位置 | 应有职责 |
| --- | --- | --- |
| `.criteria-field-group`（含 `-primary` / `-secondary` 变体） | CriteriaFields L72/95/124 | 三个分组的卡片/分区容器 |
| `.criteria-field-grid` | L78/101/130 | 组内字段网格 |
| `.criteria-toggle-row` | L157 | 复选开关行 |
| `.custom-screen-criteria` | ScreenPanel L155 | 自定义选股表单容器 |
| `.custom-screen-controls` | ScreenPanel L143 | 自定义选股控制条 |

表单实际只靠通用 `.form-row` 兜底：label `display:block` + input `width:100%; min-height:44px` → 每个字段独占一行、通栏宽度，10 个控件堆成 900px 高的细条。

### 1.2 组头塌陷

```tsx
<header><strong>基础范围</strong><span>限定行业和结果规模</span></header>
```

`.criteria-field-group` 无样式 → `<strong>` 与 `<span>` 行内挤压，视觉上合并成一句粗体长句（截图中的"基础范围限定行业和结果规模"）——标题层级和辅助说明完全没有分开，三组之间也没有任何分隔。

### 1.3 高级过滤 disclosure 裸奔（截图 2）

`.adaptive-advanced` 仅有一条 `summary { cursor:pointer; color; font-weight }`：无边框、无内边距、无 hover、无展开态、无 chevron 动画，渲染为一条通栏裸条，可点击性（affordance）极弱。展开后内部表单同样无样式（1.1）。

### 1.4 桌面密度错配

- 输入框 `min-height: var(--touch-comfort)` = **44px**（移动端触控标准）——桌面密集筛选表单应为 `--control-height` 40px，紧凑档 36px；
- 数值输入（返回数量/ROE/PE/PB/市值）通栏全宽，实际内容仅需 ~110–130px；
- "排序方向"（升序/降序两个选项）使用全宽 `<select>`，应为分段开关；
- 复选框为浏览器默认样式，与整体控件语言不一致。

### 1.5 移动端同样裸奔

`responsive.css` 中 criteria 相关规则为 0——移动端虽靠 `.form-row` 单列兜底可用，但组头塌陷、开关行、触控一致性同样未处理。本次一并补齐（媒体查询，不引入平台 class）。

---

## 2. 设计目标（验收硬指标）

| 指标 | 目标 |
| --- | --- |
| 组件 class 样式覆盖率 | 100%——新增"未样式化类"守卫（见 4.1），本次根因不得复发 |
| 自定义选股表单首屏 | 1280×860 窗口下三个分组全部可见（不滚动） |
| 桌面输入高度 | 36–40px（`--control-height`），移动端保持 44px |
| 组头 | 标题/描述两行分层，组间 1px 分隔或卡片化 |
| 高级过滤 | 有边框容器 + hover + 展开态 + chevron 旋转；展开内容 2–3 列网格 |
| 控件选型 | 数值输入固定宽度；升/降序分段开关；复选改 chip 开关 |

---

## 3. 方案

### 3.1 组件微调（CriteriaFields.tsx / ScreenPanel.tsx）

结构基本不动，只补语义类和控件替换：

```tsx
// 1) 组头分层（class 已有，样式补齐即可，TSX 无需改）
// 2) 数值字段加窄宽修饰类
<div className="form-row criteria-num-field">…返回数量 / ROE / PE / PB / 市值…</div>

// 3) 排序方向 select → 分段开关
<div className="segmented" role="group" aria-label="排序方向">
  <button type="button" className={criteria.sortDir === "desc" ? "active" : ""}
    onClick={() => update({ sortDir: "desc" })}>降序</button>
  <button type="button" className={criteria.sortDir === "asc" ? "active" : ""}
    onClick={() => update({ sortDir: "asc" })}>升序</button>
</div>

// 4) 复选 → chip 开关（保留原生 checkbox 保证可访问性，样式化 label）
<div className="criteria-toggle-row">
  <label className={`toggle-chip ${criteria.includeSt ? "active" : ""}`}>
    <input type="checkbox" … /><span>包含 ST</span>
  </label>
  …
</div>

// 5) 高级过滤容器补充状态类（ScreenPanel）
<details className="adaptive-advanced">
  <summary><span>高级过滤</span><small>ROE / PE / PB / 市值 / 排序偏好</small></summary>
  <CriteriaFields … />
</details>
```

### 3.2 桌面端 CSS（pages.css 新增区块，components.css 放基元）

```css
/* ===== 筛选表单分组（components.css 基元）===== */
.criteria-field-group { min-width: 0; }
.criteria-field-group > header {
  display: flex; align-items: baseline; gap: var(--space-2);
  padding-bottom: var(--space-2);
}
.criteria-field-group > header > strong { font-size: var(--fs-label); font-weight: 700; color: var(--text); }
.criteria-field-group > header > span { font-size: var(--fs-caption); color: var(--text-tertiary); }

.criteria-field-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: var(--space-2) var(--space-3);
}
.criteria-field-grid .form-row label { font-size: var(--fs-caption); margin-bottom: 4px; }
.criteria-field-grid .form-row input,
.criteria-field-grid .form-row select {
  min-height: var(--control-height);   /* 桌面 40px，不再 44px */
  font-size: var(--fs-data);
}
.criteria-num-field input { max-width: 132px; font-variant-numeric: tabular-nums; }

/* 分段开关 */
.segmented { display: inline-flex; gap: 2px; padding: 2px; border: 1px solid var(--line);
  border-radius: var(--radius-sm); background: var(--surface-2); }
.segmented > button { min-height: 32px; padding: 0 14px; border: 0; border-radius: 4px;
  background: transparent; color: var(--text-secondary); font-size: var(--fs-label); font-weight: 650; cursor: pointer; }
.segmented > button.active { background: var(--accent); color: var(--button-text); }

/* chip 开关 */
.criteria-toggle-row { display: flex; gap: var(--space-2); align-items: end; padding-bottom: 2px; }
.toggle-chip { display: inline-flex; align-items: center; min-height: var(--control-height);
  padding: 0 12px; border: 1px solid var(--line); border-radius: var(--radius-sm);
  background: var(--surface-2); color: var(--text-secondary); font-size: var(--fs-label);
  font-weight: 650; cursor: pointer; user-select: none; }
.toggle-chip input { position: absolute; opacity: 0; pointer-events: none; }  /* 保留可访问性 */
.toggle-chip.active { border-color: color-mix(in srgb, var(--accent) 48%, var(--line));
  background: var(--accent-soft); color: var(--text); }

/* ===== 自定义选股常驻区（pages.css）===== */
.custom-screen-criteria {
  display: grid; gap: 0; width: 100%;
}
.custom-screen-criteria .criteria-field-group {
  padding: var(--space-3) 0;
  border-top: 1px solid var(--line-soft);
}
.custom-screen-criteria .criteria-field-group:first-child { border-top: 0; padding-top: 0; }
/* ≥1100px 时三组横向铺开：基础范围 / 估值质量 / 排序偏好 */
@media (min-width: 1100px) {
  .custom-screen-criteria { grid-template-columns: 1.1fr 1.6fr 1.4fr; gap: var(--space-4); }
  .custom-screen-criteria .criteria-field-group { border-top: 0; border-left: 1px solid var(--line-soft); padding: 0 0 0 var(--space-4); }
  .custom-screen-criteria .criteria-field-group:first-child { border-left: 0; padding-left: 0; }
}

/* ===== 智能选股·高级过滤（pages.css）===== */
.adaptive-advanced {
  width: 100%; border: 1px solid var(--line); border-radius: var(--radius-md);
  background: var(--surface);
}
.adaptive-advanced > summary {
  display: flex; align-items: center; gap: var(--space-2);
  min-height: var(--control-height); padding: 0 var(--space-3);
  cursor: pointer; list-style: none; color: var(--text-secondary);
  font-size: var(--fs-label); font-weight: 650;
}
.adaptive-advanced > summary::-webkit-details-marker { display: none; }
.adaptive-advanced > summary::before { content: "›"; color: var(--text-tertiary);
  transition: transform var(--motion-fast) var(--ease-out); }
.adaptive-advanced[open] > summary::before { transform: rotate(90deg); }
.adaptive-advanced > summary:hover { color: var(--text); background: var(--surface-2); }
.adaptive-advanced > summary > small { margin-left: auto; color: var(--text-tertiary);
  font-size: var(--fs-caption); font-weight: 400; }
.adaptive-advanced[open] > summary { border-bottom: 1px solid var(--line-soft); }
.adaptive-advanced > .criteria-field-group,
.adaptive-advanced > section { padding: var(--space-3); }
/* 展开区：组间分隔 + 三组纵向（容器较窄时自动回单列由 auto-fit 保证） */
.adaptive-advanced .criteria-field-group + .criteria-field-group {
  margin-top: var(--space-3); padding-top: var(--space-3);
  border-top: 1px solid var(--line-soft);
}
```

### 3.3 移动端补齐（responsive.css，≤768px 媒体查询）

```css
@media (max-width: 768px) {
  .criteria-field-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .criteria-field-grid .form-row input,
  .criteria-field-grid .form-row select { min-height: var(--touch-comfort); } /* 触控 44px 保持 */
  .criteria-num-field input { max-width: none; }
  .custom-screen-criteria { grid-template-columns: 1fr !important; } /* 三列回单列（!important 换可读性，见风险节） */
  .custom-screen-criteria .criteria-field-group { border-left: 0; padding-left: 0;
    border-top: 1px solid var(--line-soft); padding: var(--space-3) 0; }
  .adaptive-advanced > summary { min-height: var(--touch-comfort); }
  .adaptive-advanced > summary > small { display: none; }  /* 手机上不显示提示小字 */
}
```

> 说明：`!important` 仅用于覆盖 ≥1100px 三列规则，如不希望使用，可将桌面三列规则改为 `@media (min-width: 1100px)` 独立块（本方案即如此写，移动端实际无需 !important，落地时删去）。

---

## 4. 防回归 harness（本次重点新增）

### 4.1 未样式化类守卫（根因防线）—— `scripts/check-unstyled-classes.mjs`

本次问题的本质是"TSX 写了 className，CSS 没有对应规则"且**没有任何环节报警**。新增守卫：

```js
#!/usr/bin/env node
// 扫描 src/**/*.tsx 的 className 字符串，与 styles/*.css 的选择器取交集，
// 报告"组件引用但零样式定义"的类（白名单豁免纯状态类/测试钩子）。
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const ROOT = new URL("../desktop/frontend/src/", import.meta.url).pathname;
const WHITELIST = new Set(["active", "open", "positive", "negative", "warning",
  "neutral", "unavailable", "empty", "spin", "rail-collapsed"]); // 纯状态类
const DYNAMIC = /\$\{|classList|clsx\(/; // 动态拼接的跳过精确校验

// 1. 收集 CSS 类名
const cssClasses = new Set();
for (const f of readdirSync(join(ROOT, "styles")).filter(f => f.endsWith(".css"))) {
  const css = readFileSync(join(ROOT, "styles", f), "utf8");
  for (const m of css.matchAll(/\.(-?[_a-zA-Z]+[_a-zA-Z0-9-]*)/g)) cssClasses.add(m[1]);
}
// 2. 收集 TSX className
function* walk(dir) { for (const e of readdirSync(dir)) {
  const p = join(dir, e);
  if (statSync(p).isDirectory()) yield* walk(p);
  else if (p.endsWith(".tsx") && !p.endsWith(".test.tsx")) yield p;
} }
const unstyled = new Map();
for (const file of walk(ROOT)) {
  const src = readFileSync(file, "utf8");
  for (const m of src.matchAll(/className=\{?"`([^}"`]+)"`\}?/g)) {
    if (DYNAMIC.test(m[1])) continue;
    for (const cls of m[1].split(/\s+/)) {
      if (!cls || WHITELIST.has(cls) || cssClasses.has(cls)) continue;
      if (!unstyled.has(cls)) unstyled.set(cls, file);
    }
  }
}
if (unstyled.size) {
  console.error("未样式化类（组件引用但 CSS 零规则）:");
  for (const [cls, file] of unstyled) console.error(`  .${cls}  ← ${file}`);
  process.exit(1);
}
```

接入：`frontend/package.json` 的 `test` 链（`test:density && test:unstyled && vitest run`）与 `release-check.ps1`。

> 注意误报面：状态类（active/open/positive…）走白名单；模板字符串动态拼接的类名跳过。先以 warn 模式跑一轮收敛存量，再转 fail。

### 4.2 契约测试 `src/lib/screenCriteriaLayout.test.ts`

```ts
it("自定义选股三组容器必须有样式", () => {
  for (const sel of [".custom-screen-criteria", ".criteria-field-group",
                     ".criteria-field-grid", ".criteria-toggle-row"]) {
    expect(pagesCss + componentsCss).toContain(sel);
  }
});
it("桌面三列与移动单列并存且都在媒体查询内", () => {
  expect(pagesCss).toMatch(/@media \(min-width: 1100px\)[\s\S]*?\.custom-screen-criteria\s*\{[^}]*grid-template-columns:\s*1\.1fr/);
  expect(responsiveCss).toMatch(/@media \(max-width: 768px\)[\s\S]*?\.criteria-field-grid\s*\{[^}]*repeat\(2/);
});
it("高级过滤 disclosure 具备完整交互样式", () => {
  expect(pagesCss).toMatch(/\.adaptive-advanced\[open\]/);
  expect(pagesCss).toMatch(/\.adaptive-advanced > summary::before/);
});
it("桌面筛选输入不得使用 44px 触控高度", () => {
  expect(componentsCss).toMatch(/\.criteria-field-grid \.form-row input[\s\S]*?min-height:\s*var\(--control-height\)/);
});
```

### 4.3 截图基线扩展（ui-screenshot.mjs）

- 新增路由态：`screen`（自定义选股 tab 激活）与 `screen-advanced-open`（Playwright 先 `page.click(".adaptive-advanced > summary")` 再截图——**折叠内容的截图必须显式展开**，这是此前基线的盲区）；
- 矩阵：desktop-1440 / desktop-1920 / phone-390 三档；
- 结果区有 mock 数据 + 表单区展开，一张图同时验证表单与结果布局。

### 4.4 可访问性快检

- `summary` 可键盘触发（原生 details 自带，契约测试锁定不换成 div）；
- `.toggle-chip` 内原生 checkbox 保留（焦点环 `:focus-visible` 移到 chip 上：`label:has(input:focus-visible)`）；
- 分段开关 `role="group"` + 按钮 `aria-pressed`。

---

## 5. 执行顺序与验收

| 步骤 | 内容 | 验证 |
| --- | --- | --- |
| 1 | check-unstyled-classes.mjs 上线（warn 模式） | 拿到全量未样式化类清单 |
| 2 | 3.2 桌面 CSS + 3.1 控件替换（分段开关/chip） | desktop-1440/1920 截图 |
| 3 | 3.3 移动端补齐 | phone-390 截图 + 真机 |
| 4 | 存量未样式化类清零或白名单，守卫转 fail | CI 通过 |
| 5 | 契约测试 + 截图基线（含展开态）接入 | `npm test` 全绿 |

**DoD**：
- [ ] 五个零样式类全部有规则；未样式化类守卫 fail 模式运行
- [ ] 自定义选股 1280×860 首屏三组全可见；≥1100px 三列、≤768px 单列
- [ ] 高级过滤有边框/hover/展开态/chevron 动画；展开截图入基线
- [ ] 桌面输入 40px、数值列定宽 tabular-nums；移动端 44px 不变
- [ ] 键盘可达：summary / chip / 分段开关均可 Tab + Enter 操作

**风险**：① 未样式化类守卫首轮可能报出十几个历史类（按"补样式 / 删死类 / 白名单"三选一处理）；② `.form-row` 通用规则被多处共享，所有改动限定在 `.criteria-field-grid` 作用域内，不影响趋势选股日期行等其他表单；③ 分段开关/chip 替换后需跑 ScreenPanel.interaction.test.tsx 确认交互测试仍通过（选择器变化需同步）。
