# 回测结果页深度优化美化：可直接执行的 Harness 提示语句

> 对象：回测页结果区（`BacktestPanel.tsx` 结果分支 + 波动率快照 + `pages.css` 相关区块）。
> 范围：桌面端为主，Android 降级同步补齐；不动回测数据结构与交互逻辑，只做视觉、排版、节奏。
> 用法：把下方「提示语句全文」整段粘贴给编码智能体（或新开会话的我）即可执行。参照基线：`artifacts/ui-shots/2026-08-16/desktop-1440-data/backtest-dense.png`。

---

## 提示语句全文

````text
你正在维护「股选优」桌面/Android 同源前端（Tauri 2 + React 19 + TypeScript，源码 desktop/frontend/src/）。
任务：回测结果页（BacktestPanel 运行结果区 + 波动率快照模块）深度优化美化。不改数据结构、不改图表计算逻辑，只做视觉与排版。

## 已核实的现状事实（不要重新猜测）

结果区结构（BacktestPanel.tsx）：
  .backtest-result
  ├── .metric-strip：5 张指标卡（总收益/年化/最大回撤/超额/Precision@N，strong 带 positive/negative 类）
  ├── .backtest-primary-chart → .equity-chart：组合净值曲线（SVG，stroke=currentColor 走 CSS）
  ├── .backtest-comparison：7 张次级统计卡（股票数/基准/成本/换手/调仓次数/样本外折数/Mode）
  ├── .backtest-holdings ×2-3（发布门槛检查 .backtest-fold-list、逐折明细、标的 .symbol-strip）
  ├── .notes（回测说明）
  └── .backtest-volatility（波动率快照：header+标的下拉、.volatility-grid 6 卡、.volatility-interpretation 一句话看懂/为什么这么说/策略可以怎么改/方法 details/免责）

已核实缺陷清单：
1. 【违反自家规范】pages.css L2463 `--equity-portfolio: var(--score)`——组合净值线用金色，违反 DESIGN.md Gold Score Rule（金色只属于综合评分）。改为 var(--accent)；基准线 --chart-benchmark 不变。
2. 【卡片疲劳】结果页 5+7+6=18 张同款数字卡（border+radius+bg 的 metric chrome 重复三遍），视觉单调无层级。
3. 【字阶漂移】回测/波动率区一批 rem 魔法数：0.68/0.7/0.72/0.76/0.78/0.8/0.82/0.86/0.9/0.94rem——映射 token：0.68/0.7/0.72→--fs-caption、0.76/0.78/0.8/0.82→--fs-label、0.86→--fs-body、0.9/0.94→--fs-data。
4. 【色彩语义】波动率卡中 Chaikin -25.06% 等带方向的值需要按 A 股语义着色（正=--rise 红、负=--fall 绿），无方向的值（通道区间、RVI）保持中性；一句话看懂正段整段禁止 tone 着色（用户截图显示为红色段落——无论来自旧构建还是级联，必须用运行时颜色审计锁死，见任务 D-2）。
5. 【顶部拥挤】.backtest-context 一行混合：来源切换（当前条件/自选股）+ 运行回测按钮 + 一长串参数文本（持仓·区间·调仓·成本·基准·Mode）。
6. 【标的长串】.symbol-strip 十个 mono 代码用 " · " 连成一行，无换行节奏。

工程护栏（全部必须通过）：node scripts/check-ui-density.mjs、check-css-architecture.mjs、check-unstyled-classes.mjs（若存在）；cd desktop/frontend && npm test 全绿；字号/颜色/间距/圆角/动效只取 tokens.css 变量；无硬编码 px/rem 字号；无平台 class；无 backdrop-filter；动画遵守 prefers-reduced-motion；DESIGN.md 的 One Action Color / Semantic Market（红绿配文字）/ Gold Score Rule / Flat-at-Rest 生效。

## 任务 A：结果区层级重排（治"卡片疲劳"）

1. Hero 指标带：总收益提为唯一 hero——.metric-strip 改 grid 为 [2fr 1fr 1fr 1fr 1fr]，首格（总收益）去卡片 chrome、数值升 --fs-display(24px→移动20) 800 tabular-nums 并按正负着色；其余 4 格合并为一个无边框 stat 带（label --fs-caption tertiary + value --fs-data 750），格间 1px line-soft 分隔。
2. 次级统计（.backtest-comparison 7 项）：去卡片化，改为单行 stat 条带（同 1 的 stat 规格，横向 flex-wrap），不再 7 张带框卡。
3. 净值曲线卡保持结构，仅精修：header 标题 --fs-label、legend --fs-caption、曲线区背景 surface 90%、网格线 line-soft；组合线色按缺陷 1 修正。
4. .backtest-fold-list 行精修：行高 36px、date 列 tabular-nums、pass/fail 用 6px 圆点 + 文字（不再整行红绿）。
5. .symbol-strip：代码改 chip 流（flex-wrap gap 6px，每枚 mono tabular-nums --fs-caption surface-2 底 radius-xs padding 2px 8px）。

## 任务 B：波动率快照美化

1. 6 张指标卡与结果页 stat 体系同构：label（--fs-caption）/ value（--fs-data 750 tabular-nums）/ detail（--fs-caption secondary，限 2 行）；保留 3 列网格与卡片底，但 padding、字号、行距与任务 A 完全一致——全页数字单元一种语言。
2. 方向值着色：带方向的值（Chaikin 变化率等）按正负 --rise/--fall；区间/位置/RVI 等无方向值中性 --text。
3. 一句话看懂：标题行（--fs-label 700 + "已返回 N/6 项" --fs-caption tertiary 徽章）；正文 --fs-body line-height 1.6，color: var(--text)——禁止任何 tone 类。
4. 为什么这么说 / 策略可以怎么改：两栏各加 surface-2 底卡（radius-md padding var(--space-3)），h4 --fs-caption tertiary 650，ul 行距 1.55，::marker tertiary；≤768px 改单列。
5. 标的下拉：34px→32px（桌面档），label --fs-caption；header 区标题 --fs-label 750。
6. 免责与方法 details：--fs-caption tertiary，details summary 中性（禁 accent 红）。

## 任务 C：Android 降级（媒体查询，禁平台 class）

- .metric-strip：hero 改单列（总收益大字独占一行 + stat 带 2×2）；
- .backtest-comparison stat 条带：2 列 grid；
- .volatility-grid 3→2 列（沿用现有 @media 640 规则，核对其字号也走 token）；
- .volatility-interpretation-grid 2→1 列；
- .backtest-context 参数长串：换行为两行（来源+按钮一行，参数串 caption 一行 wrap）；
- hero 数值 --fs-headline(20)。

## 任务 D：测试与截图（随代码一起交付）

1. 契约测试 src/lib/backtestPage.contract.test.ts：
   - pages.css 不存在 `--equity-portfolio: var(--score)`（Gold Score Rule）；
   - 回测/波动率区 0 处 rem/px 魔法字号（映射后锁定）；
   - .volatility-interpretation-summary 规则块 color 仅为 var(--text)；
   - .backtest-comparison 不再出现 border+radius 卡片 chrome（断言其规则块不含 border-radius）。
2. 【新】运行时颜色审计（Playwright，并入 ui-screenshot.mjs 或独立 e2e）：对 backtest-dense 页面取 computed style——
   - .volatility-interpretation-summary 的 color 必须等于 --text 解析值；
   - .metric strong.positive 必须等于 --rise、.negative 等于 --fall；
   - 波动率方向值正负着色正确。
   目的：截图与 CSS 静态分析不一致（用户安装包疑为旧构建）时，运行时断言是唯一可信源。
3. 截图基线：backtest 数据态 × {desktop-1440, desktop-1920, phone-390} × {dark, light}；波动率快照元素级（.backtest-volatility）同矩阵；已有 backtest-dense.png 作为 before 参照。
4. 守卫三件套 + vitest 全绿。

## 验收 DoD

- [ ] 组合净值线非金色；页面金色只出现在综合评分场景
- [ ] 18 张数字卡收敛为：1 hero + 2 条 stat 带 + 6 张波动率卡（同构语言）
- [ ] 0 rem/px 魔法字号；一句话看懂正文运行时色 == --text
- [ ] 方向值 A 股语义着色（正红负绿），无方向值中性
- [ ] Android 390px 全部降级无横滚；基线与审计全绿

## 执行顺序（每步独立 commit）

1. 缺陷 1/3/4 修正（净值线色 + 字阶映射 + 方向着色）——小步快跑，截图比对；
2. 任务 A 结果区层级（hero + stat 带 + 去卡片化）；
3. 任务 B 波动率精修 + 任务 C Android；
4. 任务 D 契约/审计/基线接入。
````

---

## 备注

- "一句话看懂显示为红色"在用户安装包截图中可见，但当前源码 `.volatility-interpretation-summary` 已是 `color: var(--text)`——疑似安装包滞后或存在未发现的级联。提示语句中的**运行时颜色审计**（任务 D-2）就是为此类"静态分析与真机不一致"准备的最终裁判，建议后续各页 harness 都沿用这一道。
- 与观察摘要/消息页 harness 的同构约定：6px 圆点、stat 结构（label caption + value data 750）、卡片去 chrome 改 stat 带——三份文档语言一致，执行时可互为参照。
