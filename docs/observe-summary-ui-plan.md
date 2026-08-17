# 观察摘要模块 UI 治理方案（Android + 桌面端）

> 对象：观察页「观察摘要」大模块（`ObservePanel.tsx` L329–390 + `pages.css` L3647–4470 + `CapitalQuantPanel`）。
> 用户反馈：文字、图标（装饰符号）过于杂乱。本文档给出量化诊断、双端优化方案与防回归 harness。

---

## 1. 诊断：杂乱不是观感问题，是可量化的失控

### 1.1 字阶失控（核心病因）

模块范围（pages.css L3647–4470）内实测 **44 处字号声明、12 种取值**：

| 取值 | 次数 | 是否 token 阶梯 |
| --- | --- | --- |
| var(--fs-caption) | 13 | ✅ |
| 12px | 6 | ✅（=--fs-label 但未用变量） |
| **10px** | 6 | ❌ 阶梯外 |
| **10.5px** | 5 | ❌ 阶梯外 |
| 11px | 3 | ✅（=--fs-caption 但未用变量） |
| **11.5px** | 3 | ❌ 阶梯外 |
| 13px | 2 | ✅（未用变量） |
| **12.5 / 15 / 16 / 17 / 18px** | 各 1 | ❌ 阶梯外或与阶梯冲突 |

**10–12.5px 这 2.5px 的带内挤了 7 种字号**——单独看每种都"差不多"，组合起来就是没有层级的毛边感。再叠加 5 种字重（650/700/720/750/760）、2 种 letter-spacing（0.04em/0.09em）、1 处 uppercase，一个模块内出现了超过 20 种文字变体。

> 连带更正：全库实测 **196 处硬编码 px 字号、14 种取值，约半数在 token 阶梯外**（已更正 desktop-frontend-optimization-plan.md 中"无硬编码 px 字号"的错误结论）。观察摘要模块是重灾区，先行治理。

### 1.2 三套装饰系统并存（"图标乱"的来源）

| 位置 | 装饰 | 问题 |
| --- | --- | --- |
| `.observe-verdict` | 8px 圆点（桌面）/ 渐变底 + inset 4px 色条（base） | 两种形态随断点切换 |
| `.observe-decision-item` / `.capital-main-flow` / `.capital-quant-lane` | 28px / 44px 短横 `::before` | 三处短横宽度不一 |
| `.observe-special-quant-item` | 6px 圆点 + `box-shadow 0 0 0 3px` 发光环 | 发光违反 DESIGN.md Flat-at-Rest；与 verdict 圆点是"相似但不同"的第三套 |

同一模块三种装饰语义，且都不承载交互——纯噪声。

### 1.3 Android 端：网格零降级

`responsive.css` 中**没有任何** observe/capital 相关覆写。以下桌面网格在 390px 手机上原样渲染：

| 网格 | 桌面列数 | 手机现状 |
| --- | --- | --- |
| `.observe-decision-grid`（趋势位置/价格表现/财务概览） | 3 | 3 列长文挤压 |
| `.observe-special-quant`（资金背离/趋势效率/波动状态/流动性风险） | 4 | 4 列×三行文字挤压 |
| `.observe-key-metrics`（最新价/涨跌幅/支撑/压力） | 4 | 勉强可用 |
| `.capital-main-flow .capital-quant-metrics` | 3 | 3 个"暂缺"占位 |
| `.capital-quant-lanes`（龙虎榜/量价代理） | 2 | 2 列各 4 指标 |

### 1.4 文案层泄漏与噪声（截图实证）

1. **枚举原文泄漏**：`signalTypeLabel`（ObservePanel.tsx L842）只映射 7 个枚举，`labels[...] || String(value)` 回退原文 → 截图中"类型为trend_continuation"。`riskFlagLabel`/`patternSignalLabel` 同款回退。
2. **原始时间戳**：`<time>{String(stock.quote_time)}</time>` → 右上角"20260807161409"。
3. **术语未翻译**：L712 "SWL 位于 SWS 上方"直接面向用户。
4. **缺失态不降权**：接口不可用时仍渲染 3 个等大的"暂缺"指标位 + 两段常驻说明文（"怎么看…""介入度按净占比分档…"）——缺失信息占据了与有效信息相同的视觉权重。
5. **语义色竞争**：同屏出现 rise 红（圆点、涨跌幅、机构净买）、fall 绿（偏流出）、warning 琥珀（震荡噪声、估算标记）、accent 红（"下一步看什么"标签）——accent 红与市场红撞车，违反 DESIGN.md「One Action Color」与「Semantic Market」规则。

---

## 2. 治理目标（验收硬指标）

| 指标 | 目标 |
| --- | --- |
| 模块字号 | 收敛到 4 档且全部取 token：标题 17(--fs-title) / 数值 15–17 等宽 / 正文 12(--fs-label) / 辅助 11(--fs-caption)；10/10.5/11.5/12.5px 清零 |
| 装饰 | 全模块只剩**一种**语义标记：6px 圆点、无发光、仅 rise/fall/warning/neutral 四态；`::before` 短横清零 |
| 色彩 | 单屏语义色 ≤ 3 种；accent 红只用于交互，标签不再使用 |
| 缺失态 | "暂缺"统一 12px tertiary 常规字重；接口不可用时折叠为单行提示 + details 口径说明 |
| Android | 5 个网格全部有 ≤768px 降级（媒体查询，禁平台 class） |
| 文案 | 枚举 100% 中文映射且回退不再输出原文；时间戳格式化；snake_case 泄漏有测试拦截 |

---

## 3. 方案 A：信息架构与文案（双端共用，先做）

### 3.1 模块节奏重排

```
┌─────────────────────────────────────────────┐
│ 观察摘要                          08-07 16:14 │  ← 标题带（时间格式化）
│ ● 趋势偏强，进入确认阶段                        │  ← 结论带（verdict，仅圆点装饰）
│   积极结构已经出现，下一步验证压力位突破…         │
├─────────────────────────────────────────────┤
│  最新价      涨跌幅      支撑位      压力位      │  ← 关键数带（上移紧贴结论，
│  4.86      +0.83%      4.78       4.89      │    决策最先要的就是这四个数）
├─────────────────────────────────────────────┤
│  趋势位置    价格表现    财务概览               │  ← 证据区一：三项判断
│  ● 资金背离  ● 趋势效率  ● 波动状态  ● 流动性风险 │  ← 证据区二：四项状态（合并为同一
├─────────────────────────────────────────────┤    个 .observe-fact 基元渲染）
│  资金量化证据                    真实资金流·本地估算 │
│  最新交易日主力资金 ── 接口不可用（单行折叠态）      │  ← 缺失折叠，不再摆三个"暂缺"
│  ▸ 口径说明                                    │
│  龙虎榜机构席位        │  量价资金代理（估算）       │  ← lanes 保留，移动端堆叠
├─────────────────────────────────────────────┤
│ ▸ 专业指标明细（disclosure 保持现状）             │
└─────────────────────────────────────────────┘
```

### 3.2 文案规范化（TS 层改动）

```ts
// ObservePanel.tsx —— label 映射补全 + 安全回退
function signalTypeLabel(value: unknown): string {
  const labels: Record<string, string> = {
    bullish: "看多", bearish: "看空", neutral: "中性", watch: "观察",
    buy_setup: "买点预备", exit: "离场", uptrend: "上升趋势",
    trend_continuation: "趋势延续", trend_reversal: "趋势反转",
    range_bound: "区间震荡", breakout_attempt: "突破尝试", // …按后端枚举全集补齐
  };
  const key = String(value || "");
  if (!key) return "--";
  if (!labels[key]) console.warn("[observe] 未映射枚举:", key); // harness 测试会拦截
  return labels[key] || "未识别类型";          // ← 不再回退英文原文
}

// 时间格式化（复用或新增 lib/format.ts）
function formatObserveTime(raw: unknown): string {
  const s = String(raw || "");
  const m = s.match(/^(\d{4})(\d{2})(\d{2})(\d{2})?(\d{2})?/);
  if (!m) return s || "数据时间未知";
  return m[4] ? `${m[1]}-${m[2]}-${m[3]} ${m[4]}:${m[5] || "00"}` : `${m[1]}-${m[2]}-${m[3]}`;
}

// 术语翻译：L712 改为
const trendPosition = signal?.swl_above_sws === true
  ? "短期均线位于长期均线上方，趋势结构偏积极"
  : signal?.swl_above_sws === false
    ? "短期均线位于长期均线下方，趋势结构仍需观察"
    : "均线关系暂不完整";
```

### 3.3 缺失态折叠（CapitalQuantPanel）

```tsx
{!mainFlow.available ? (
  <p className="capital-flow-unavailable">
    主力资金接口暂不可用，以下为本地量价代理估算。
    <details className="capital-quant-note-details">
      <summary>口径说明</summary>
      {/* 原"怎么看"与"介入度分档"两段文字移入此处 */}
    </details>
  </p>
) : (
  <CapitalQuantMetrics metrics={[...]} />
)}
```

有效数据时："暂缺"单项用 `.is-missing`（12px / tertiary / 字重 400）渲染，与有效值拉开层级。

---

## 4. 方案 B：桌面端样式收敛

### 4.1 字号映射表（逐项替换，消灭阶梯外取值）

| 元素 | 现状 | 目标 |
| --- | --- | --- |
| 模块标题 h3 | 17px / 桌面 15px | `var(--fs-title)`（17px），桌面不再降 |
| 时间戳 | 10.5px | `var(--fs-caption)` |
| verdict 标题 | 18px / 桌面 16px | `var(--fs-title)` |
| verdict 摘要 | 12px | `var(--fs-label)` |
| decision 标题 / 正文 | 12px@760 / 11px | `var(--fs-label)` 700 / `var(--fs-caption)` |
| special 状态值 | 12.5px | `var(--fs-data)`（13px） |
| special 描述 | 10px | `var(--fs-caption)` |
| 下一步 标签 / 正文 | 10px uppercase 红 / 12px | `var(--fs-caption)` tertiary 非大写 / `var(--fs-label)` |
| key-metrics 标签 / 数值 | 10px / 15px | `var(--fs-caption)` / `var(--fs-title)` 750 + tabular-nums |
| capital h4/h5/small | 13/12/10px | `var(--fs-label)` / `var(--fs-label)` / `var(--fs-caption)` |
| 结论与口径说明 | 10.5px | `var(--fs-caption)` |
| 免责声明 | 10px | `var(--fs-caption)` |

### 4.2 装饰统一（删两套，留一套）

```css
/* 删除：.observe-decision-item::before、.capital-main-flow::before、
   .capital-quant-lane::before（三处短横）、.observe-verdict 的渐变底与 inset 色条 */
/* 保留并统一为唯一标记（special-quant 圆点去发光）： */
.observe-special-quant-item > span::before,
.observe-verdict::before {
  width: 6px; height: 6px; border-radius: 50%;
  background: var(--observe-tone, var(--text-tertiary));
  box-shadow: none;            /* 去掉 0 0 0 3px 发光环 */
  content: "";
}
/* "下一步看什么"标签去 accent 红，改 tertiary；红色只出现在涨跌数值上 */
.observe-next-signal > span { color: var(--text-tertiary); text-transform: none; letter-spacing: 0; }
```

### 4.3 桌面"连续证据表"保留

`@media (min-width: 769px)` 的扁平行分隔设计是对的，只收敛分隔节奏：每区恰好一条 `1px var(--line-soft)` 下边框，删除各区自带的 margin-top 参差（统一 `var(--space-2)` 节奏）。

---

## 5. 方案 C：Android 端网格降级

在 `responsive.css` 的 `@media (max-width: 768px)` 块内**新增**（这是补齐媒体查询，不是恢复已删除的平台 class 补丁）：

```css
@media (max-width: 768px) {
  /* 三项判断：3列 → 纵向列表 */
  .observe-decision-grid { grid-template-columns: 1fr; }
  .observe-decision-item { border-left: 0; border-top: 1px solid var(--line-soft); }
  .observe-decision-item:first-child { border-top: 0; }

  /* 四项状态：4列 → 2×2 */
  .observe-special-quant { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .observe-special-quant-item:nth-child(odd) { border-left: 0; }
  .observe-special-quant-item:nth-child(n+3) { border-top: 1px solid var(--line-soft); }

  /* 关键数带：保持 4 列（数值短），数值缩一档 */
  .observe-key-metrics dd { font-size: var(--fs-data); }

  /* 资金 lanes：2列 → 堆叠 */
  .capital-quant-lanes { grid-template-columns: 1fr; }
  .capital-quant-lane { border-left: 0; border-top: 1px solid var(--line-soft); }
  .capital-quant-lane:first-child { border-top: 0; }

  /* 长文本行宽与行距 */
  .observe-verdict p, .observe-decision-item p { line-height: 1.55; }
}
```

约束（写入契约测试）：以上规则**只允许**出现在 `@media (max-width: 768px)` 块内，禁止新增任何 `html.android-*` 选择器。

---

## 6. 方案 D：防回归 harness（四层）

### 6.1 模块契约测试 `src/lib/observeSummaryContract.test.ts`

```ts
const pagesCss = readFileSync("../styles/pages.css", "utf8");
const observeBlock = pagesCss.slice(pagesCss.indexOf(".observe-decision-summary"));

it("观察摘要模块字号必须全部取自 token 阶梯", () => {
  const hardcoded = [...observeBlock.matchAll(/font-size:\s*([\d.]+)px/g)].map(m => m[1]);
  expect(hardcoded).toEqual([]);   // 替换完成后锁定为 0
});

it("模块装饰 ::before 只剩统一圆点白名单", () => {
  const decorators = [...observeBlock.matchAll(/([^{}]+)::before/g)].map(m => m[1].trim());
  expect(decorators.sort()).toEqual([
    ".observe-special-quant-item > span", ".observe-verdict",
  ].sort());
});

it("移动端五个网格必须存在降级", () => {
  for (const sel of [".observe-decision-grid", ".observe-special-quant",
                     ".observe-key-metrics", ".capital-quant-lanes"]) {
    expect(responsiveCss).toContain(sel);
  }
});
```

### 6.2 文案守卫（双保险）

- **枚举完整性测试**：从后端契约（`lib/contracts.ts` / Rust 输出样例）收集信号枚举全集，断言 `signalTypeLabel`/`riskFlagLabel`/`patternSignalLabel` 每个枚举都有中文映射；
- **泄漏渲染测试**：用含未映射枚举的 fixture 渲染 ObservePanel，断言输出文本不匹配 `/\b[a-z]+_[a-z_]+\b/`（snake_case 即失败）；
- **时间格式测试**：`formatObserveTime("20260807161409") === "2026-08-07 16:14"`。

### 6.3 截图基线扩展（ui-screenshot.mjs）

观察页增加**两种数据 fixture**：①完整资金数据态 ②接口不可用折叠态；基线矩阵 `desktop-1440 / desktop-1920 / phone-390 / desktop-1440-light` × 2 态 = 8 张，模块级截图（element screenshot `.observe-decision-summary`）而非整页，比对精度更高。

### 6.4 密度守卫扩展（check-ui-density.mjs）

新增规则：`pages.css` 观察摘要段内出现 `font-size: <px>` 直接 fail；全库 off-token px 字号（10/10.5/11.5/12.5/13.5/15/16/18/19）先 warn 输出清单，本模块清零后将该段转 fail，后续按文件逐步收编全库 196 处。

---

## 7. 执行顺序与验收

| 步骤 | 内容 | 验证 |
| --- | --- | --- |
| 1 | 枚举映射补全 + 时间格式化 + 术语翻译（3.2） | 文案守卫测试通过，截图无 snake_case |
| 2 | 缺失态折叠（3.3） | 接口不可用 fixture 截图 |
| 3 | 桌面字号映射 + 装饰统一 + 色彩收敛（4.x） | 模块契约测试 + desktop 基线比对 |
| 4 | Android 网格降级（5） | phone-390 基线 + 真机 chrome://inspect |
| 5 | harness 全部接入 `npm test` / release-check | 守卫全绿 |

**验收 DoD**：
- [ ] 模块内 0 处硬编码 px 字号、0 处 `::before` 短横、0 处发光
- [ ] 截图对比：完整态/缺失态 × 桌面/手机 各 8 基线入库
- [ ] 单屏语义色 ≤ 3；accent 红不出现在静态标签
- [ ] 未映射枚举渲染为"未识别类型"且测试告警；时间戳 `YYYY-MM-DD HH:mm`
- [ ] 全部新 CSS 仅在媒体查询块内，无平台 class

**风险**：字號整体上調（10→11、10.5→11 等）會讓模塊總高度增加約 8–12%，屬於預期內的可讀性收益；若個別表格行溢出，優先刪減文案而不是縮字號——這是本次治理的基本取捨。
