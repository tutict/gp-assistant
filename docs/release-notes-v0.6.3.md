# 股选优 v0.6.3 过渡版

> v0.6.3 过渡版发布说明。

v0.6.3 延续 v0.6.2 的研究工作流，重点补强行情证据展示、筛选结果解释、Android 布局稳定性和导入安全边界。

## 更新内容

- 选股与观察结果显示日线日期和行情日期；历史数据落后时明确标记，并保留覆盖不足和陈旧告警，方便判断结果依据。
- 市场能力状态集中展示数据来源、覆盖缺口、最近观测时间和核验状态；缺失数据仍按现有门槛限制相关能力。
- Android 在首帧渲染前读取系统安全区，并按生成项目的应用命名空间写入启动代码。
- 为行情提供方密钥建立独立凭据命名空间，与模型 API 密钥隔离。
- 加固旧版 RAG 包导入路径，阻止点路径目录逃出缓存根；CI 中每条 Rust 评估命令分别检查退出码。
- 更新桌面和移动端工作区、设置分组及新闻页视觉基线；增加筛选与观察流程的按需性能诊断。

## 已知边界

- adaptive 严格样本外门槛仍未通过完整点时数据验证。本版本不宣称该门槛已通过；缺少历史快照时仍会明确拒绝严格验证。
- 行情提供方授权和市场覆盖能力继续由数据证据门槛控制；凭据存储支持本身不代表数据源已经获准或可用。
- Android 签名 APK 已构建并核验包名、版本与签名；尚未在实体 Android 设备上安装或升级验证。
- Windows 安装包已构建，但 Authenticode 状态为 `NotSigned`；尚未完成 Windows 安装或升级验证。
- 同设备真实性能基线没有采集，因此性能门禁仍未评估。

## 发布产物

- Windows x64 NSIS 安装程序：`desktop/src-tauri/target/release/bundle/nsis/股选优_0.6.3_x64-setup.exe`（33.86 MiB，尚未进行 Authenticode 签名）。
- Android ARM64 签名安装包：`desktop/src-tauri/gen/android/app/build/outputs/apk/universal/release/guxuanyou_0.6.3_android_aarch64_release_signed.apk`。
- Android 清单：`applicationId=com.tutict.stockoptimizer`、`versionName=0.6.3`、`versionCode=603`；APK 签名方案 v2/v3 验证通过，证书 SHA-256 为 `9413a80ba4bf90ef051f3528fbf273ac99cd28e16e386f844889b967929fb191`。
- Android APK SHA-256：`E13A359DC9562EE0C1B2CE6833EB1E554A16576FD90E80869F1B82026BB1FA3A`。
- Windows 安装包 SHA-256：`19FB162E09E7AF5B07D49578D4D41EA2A10613A442AB22265FF6D5131E7B74C7`。

## 发布验证（2026-11-03）

- 完整代码检查通过：版本一致性、可靠性 fixture、前端 735 项单测、核心与 Tauri/GEPA Rust 测试、格式检查、桌面浏览器回归、对比度审计、Android 包名隔离测试和 Android 环境预检。
- `scripts/release-check.ps1 -SkipRust -SkipNode -SkipPrepare -SkipAndroidPreflight`：通过，构建并识别了 Android 签名 APK 和 Windows NSIS 安装包。Rust、前端和环境检查在同日的完整检查轮次中单独通过。
- `-EvaluateBaseline` 同设备真实性能对比：未执行。当前没有可核验的设备基线。
- 设备安装升级验证尚未完成；Windows 安装包没有 Authenticode 签名。
