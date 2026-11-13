# 发布前页面优化验收记录

更新日期：2026-11-13。**本次计划范围内的页面验收完成**。Windows／MuMu Android 15 原生验收、完整前端检查与视觉回归均通过，已确认的阻塞和重大问题全部关闭。

工作分支：`codex/release-ui-polish`。版本保留 `0.6.3`，没有提交、发布或修改签名配置。原有 A 股公司资料与消息透明度改动保留；领域算法、接口、能力门槛、存储键及深色／舒适密度／标准字号默认值沿用现有实现。

## 逐页记录

| 页面 | 实施内容 | 验证与状态 |
| --- | --- | --- |
| 全局框架 | 统一紧凑加载、成功、部分失败与失败反馈；恢复入口参与正常布局；保留语义色及 48px 手机主要操作 | 密度、CSS 架构、对比度与最终整套检查见下方 |
| 选股 | 返回数、总数、最高分合并为一行；手机结果优先；调整条件沿用筛选 Sheet；模式标签自动进入可见范围；保留原结果及成功条件快照 | 34 个聚焦用例通过，独立审查通过；公司资料来源与抓取时间保留 |
| 观察 | 身份、日期、结论和关键数值优先，图表先于长解释；完整财务指标展开查看，缺项仍显式列出；输入旁验证及独立重试；同股票结果保留、跨股票响应丢弃 | 58 个观察／财务／港股用例通过，独立审查通过；港股明确要求默认展开的调用仍保留 |
| 回测 | 桌面分组保留；手机成功后收起参数，显示完成请求的范围、日期、方式与成本；调整参数重新展开；完成后定位结果标题 | 36 个用例通过，复审通过；港股范围摘要按实际请求修正，参数改动与原结果区分 |
| 消息 | 标题、计数、更新时间收紧；展开后摘要只出现一次；原文、来源、发布时间、消息／摘要抓取时间保留；部分来源失败可重试并继续阅读缓存 | 75 个消息用例通过；新增部分失败视觉样例；恢复按钮增加完整显示及 48px 检查 |
| 情绪 | 概览与主图先于关键证据；详细来源、分析维度、逐日表按需展开 | 原冻结证据顺序与归属测试保留；完整前端检查覆盖 |
| 研究助手 | 桌面约 760px 阅读列；手机消息行占满可用宽度，用户气泡仅一层 88% 上限并靠右，助手正文靠左；长内容在自身范围内换行或滚动；真实软键盘出现时使用原生可见高度 | 桌面／手机几何与复盘交互通过；MuMu 竖屏及 844×390 横屏实际键盘下输入框和 48px 发送按钮完整可见，返回保留草稿 |
| 设置与弹层 | 设置说明与控件关联；右侧抽屉／手机底部 Sheet 保留；标题和操作入口固定、正文独立滚动；关闭明确设置 false；短横屏模型编辑区独立滚动 | Windows 焦点循环、嵌套 Escape 与焦点恢复通过；Android 系统返回、键盘优先与嵌套恢复通过；模型字段、测试／保存入口、48×48px 关闭入口可达 |

## 前端证据

- 修改前：114 个测试文件、896 个用例通过；原消息来源／时间功能与旧消息截图基线存在 0.00328 差异，原限值为 0.001。
- 浏览器采用项目已有 Playwright；Browser 插件未提供。检查覆盖 1280、1440、1920、2560、360、390、430、768、横屏，以及浅色、大字号、紧凑密度组合。
- 已完成 72 个页面／视口状态的交互、页面身份、框架错误、控制台与几何检查。此采样不替代原生验收。
- 独立页面审查及最终复审已通过；最终搜索弹层 P2 已关闭。加载中、市场确认、选项焦点与事件不穿透父层均有回归用例，过期搜索响应不会重新打开弹层。
- 包体预算仍为 180KB gzip 警告、220KB 失败；截图阈值仍为像素阈值 8、最大差异比例 0.001；没有放宽限值。
- 最终 `npm test`：退出码 0，116 个 Vitest 文件、939 个用例通过；颜色、触控、版本、密度、CSS 架构、主题一致性、构建、24 个对比度组合、桌面快捷键／搜索层级、复盘及消息交互全部通过。
- 像素回归实际比较了 104 张图片：消息 82 张、研究助手 4 张、回测 18 张，全部为 `matched`；按像素阈值 8 测得的最大差异比例为 0。观察摘要另有 16 个已确认样例。部分失败恢复按钮的卡片边界与 48px 检查通过。
- 原回测 6 组视口／主题、18 张图只采样而未比较。已在完整检查入口加入 `--compare-all-baselines`，固定回测样例日期与动效，并按独立视觉审查更新参数／结果／波动率图片。研究助手原 4 个采样也已建立可比较基线，完整检查已验证这两组图片实际进入比较。
- 原生键盘修复将短输入框统一到 48px。消息删除确认页的 10 张基线只更新该已确认高度变化；1440、1920、390 三种视口恢复旧高度后，差异像素均为 0，原阈值未改变。
- 主入口为 294.4KB raw、94.5KB gzip，包体预算通过；CSS 为 13 个文件、2.4% 跨文件重复。原 5% 重复比例和单文件维护限值保留。`git diff --check` 通过。
- 发布检查的测试 fixture 改为结构化参数与预期错误捕获，避免旧版 PowerShell 重定向错误时挂起；真实发布 gate、30 秒超时与测量规则保留。回测未来日期用例使用下一年日期，避免样例随实际日期过期。

最终日志：[完整前端检查](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-accepted-frontend.log)、[包体检查](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-final-bundle.log)及[机器可读验证摘要](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-final-verification.json)。

## 原生验收

MuMu Android 15 实例 0，ADB `127.0.0.1:16384`，物理 1440×2560、density 640，实际 WebView 为 360×640。使用隔离包 `com.tutict.stockoptimizer.codextest`，原正式包数据未替换。已确认原生版本 0.6.3、顶部安全区 24px、该配置底部安全区 0px，以及系统字号 1.3 倍真实生效（标签 12px → 15.6px）。字体变化会重建 WebView，验收必须选择 attached 且可见页面；旧调试页面不能当作当前页面。

Android 最新隔离构建通过了完整原生脚本，留下 16 个截图状态。系统返回按键先关闭软键盘，再关闭内层和外层弹层；草稿保留、嵌套焦点恢复、无效代码旁提示均通过。原生覆盖 360×640、768×1024、640×360 和 844×390，以及真实 1.3 倍系统字体和非零安全区。

MuMu 的内置搜狗键盘出现时，其实际窗口高度为 0。验收采用仓库外构建的本地诊断 InputMethodService，形成真实 240px Android IME 窗口，验证系统 Insets 与返回事件。它无需网络或存储权限。该状态下 WebView 的 `innerHeight` 和 `visualViewport.height` 都保持不变，曾导致输入区被覆盖；修复后使用原生活动窗口与 IME 高度，竖屏和宽横屏的输入／发送均完整可见。模型配置的独立 portal 同样使用原生边界，短横屏可滚动到字段和测试／保存入口。

非零安全区使用 Android 系统双缺口 overlay 实测：顶部、底部均为 32px，底部导航位于安全区之上。宽横屏设置按钮为 44×44px、y=25.5，避开 24px 系统状态栏；短横屏模型关闭入口为 48×48px。

Windows 最新隔离 Debug 构建成功。实际测试窗口验证了 Ctrl+K 搜股、输入 `600519` 自动规范为 `600519.SH`、Ctrl+= 缩放、Ctrl+0 复位、设置焦点循环、固定标题与正文滚动、放大字体、嵌套 Escape、内外层焦点恢复和快捷键帮助关闭。最大化／恢复有实际 1283×891 与 1659×1067 窗口证据；曾尝试的角落拖动未改变尺寸，不作为最小窗口验收证据。最终使用最新前端嵌入重新验证搜股、缩放复位与设置关闭。

验收后已关闭 Windows 测试窗口，释放 exe 占用；MuMu 已读回确认物理尺寸 1440×2560、density 640、系统字体 1.0、原搜狗输入法、硬件键盘显示 IME=0、自动旋转=1／用户旋转=0。临时 cutout overlay 已关闭，ADB 转发已移除。诊断 IME 保留安装但未启用。默认深色、舒适密度和标准字号已恢复。

本次原生验收范围为这台 Windows 运行环境与已授权的 MuMu Android 15；版本仍为 0.6.3。原生截图与浏览器采样分别记录，签名和安装包发布交由后续发布流程处理。

修复与回归入口：[Windows 窗口](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/src-tauri/src/lib.rs)、[Android 窗口与返回适配](C:/Users/tutic/IdeaProjects/gp-assistant/scripts/prepare-tauri-android-assets.ps1)、[原生视口读取](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/lib/mobileViewport.ts)、[键盘与短横屏样式](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/styles/viewport.css)、[设置关闭与焦点](C:/Users/tutic/IdeaProjects/gp-assistant/desktop/frontend/src/components/Header.tsx)、[截图比较](C:/Users/tutic/IdeaProjects/gp-assistant/scripts/ui-screenshot.mjs)。

## 截图与复现材料

材料目录：`C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/`。

- `release-ui-planning/`：修改前截图；观察使用 `observe-data`，设置使用 `settings-settled` 版本。
- `release-ui-before.patch`、`release-ui-before-baselines/`：原工作区差异及原视觉基线。
- `release-ui-after/`：页面对比、72 状态 JSON 记录。
- `release-ui-goldens/`、`release-ui-news-latest/`、`release-ui-observe-goldens/`：已确认视觉变化及状态样例。
- `release-ui-backtest-stable/`：固定日期与动效的回测 6 状态、18 张最终采样；`release-ui-backtest-before-baselines/` 保存更新前图片。
- `release-ui-composer-diff*.json`、`release-ui-composer-before-baselines/`：48px 输入高度差异诊断与更新前的删除确认图片。
- `native-android-initial.png`：真实 MuMu 运行截图，属于阶段证据。
- `native-windows-input-stage.png`：已捕获 Windows 原生输入窗口，属于阶段证据。
- `release-ui-native-android-qa.mjs`、`release-ui-native/android-report.json`：已执行通过的原生回归脚本与 16 个状态。
- `release-ui-native/windows-report.json`：Windows 原生输入、缩放、焦点、设置与窗口尺寸证据；最终源代码复测使用 `windows-latest-source-*` 截图。
- `release-ui-native/android-landscape-wide-keyboard.png`、`android-landscape-model-field.png`、`android-landscape-model-actions.png`、`android-nonzero-safe-area.png`：最终键盘、字段、操作与安全区证据。
- `release-ui-browser-qa.mjs`：隔离浏览器页面矩阵脚本。
- 各 `release-ui-*.log`：基准、测试和原生构建日志。

已更新的视觉基线位于 `desktop/frontend/test-baselines/`；新增 `news/partial/`，公司资料与消息时间已补入截图样例。正式发布仍交由后续版本、签名与安装包流程。

### 主要页面前后对比

| 页面 | 修改前 | 优化后 |
| --- | --- | --- |
| 选股 | [手机选股（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-screen.png) | [手机选股（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-screen.png) |
| 观察 | [手机观察（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-observe-data.png) | [手机观察（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-observe.png) |
| 回测 | [手机回测（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-backtest.png) | [手机回测（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-backtest.png) |
| 消息 | [手机消息（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-news.png) | [手机消息（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-news.png) |
| 研究助手 | [手机消息（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-agent.png) | [手机消息（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-agent.png) |
| 设置 | [手机设置（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-planning/phone-light-settings-settled.png) | [手机设置（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-after/phone-390-light-settings.png) |
| Android 横屏键盘 | [原生遮挡（前）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-native/android-landscape-keyboard-before.png) | [原生输入／发送（后）](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-native/android-landscape-wide-keyboard.png) |

原生模型的[输入字段](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-native/android-landscape-model-field.png)、[测试／保存入口](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-native/android-landscape-model-actions.png)与[非零底部安全区](C:/Users/tutic/.codex/visualizations/2026/11/12/01a1cd37-d0b1-7f40-ae52-2f13854eb8e6/release-ui-native/android-nonzero-safe-area.png)另有实际设备截图。
