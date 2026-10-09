# Windows 实机可靠性基线（2026-10-07）

## 设备与安装包

- 安装方式：当前用户 NSIS 安装
- 安装路径：`C:\Users\tutic\AppData\Local\股选优\stock-optimizer-desktop.exe`
- 安装包：`desktop/src-tauri/target/release/bundle/nsis/股选优_0.6.2_x64-setup.exe`
- 安装包 SHA-256：`01A5A9DE4E9D7D66F5448D7E6C37E91D7EBCDA277F3A20ADDE75B8FD0A3F0C9F`
- 可执行文件 SHA-256：`FAE91DB058104B9A85AAA31F8AC90587090E807A3DC8278B5DC82FDED93DD99A`
- 测试时间：2026-10-07（Asia/Shanghai）

## 冷启动、内存和 CPU

数据文件：`C:\tmp\guxuanyou-real-baseline-20261007\cold-start-memory.json`

| 轮次 | 首次窗口 | 峰值工作集 | 峰值私有内存 | 峰值线程 | 20 秒采样 CPU 时间 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 370 ms | 868,704,256 B（约 829 MiB） | 859,394,048 B（约 820 MiB） | 42 | 6.219 s |
| 2 | 297 ms | 878,215,168 B（约 837 MiB） | 862,507,008 B（约 823 MiB） | 53 | 6.190 s |
| 3 | 292 ms | 869,806,080 B（约 830 MiB） | 859,766,784 B（约 820 MiB） | 43 | 6.340 s |

初步基线：窗口出现约 **292–370 ms**；启动阶段峰值工作集约 **837 MiB**。该峰值包含当前用户缓存、WebView2、Rust/GEPA/embedding 运行时和正常后台行为，不能直接归因于单个模块。

## 强杀后重启

数据文件：`C:\tmp\guxuanyou-real-baseline-20261007\crash-restart.json`

| 强杀时机 | 重启窗口 | 重启峰值工作集 | 重启峰值私有内存 | 结果 |
| ---: | ---: | ---: | ---: | --- |
| 500 ms | 275 ms | 834,207,744 B | 831,971,328 B | 重启成功 |
| 1,500 ms | 261 ms | 855,695,360 B | 843,395,072 B | 重启成功 |
| 3,000 ms | 253 ms | 868,823,040 B | 859,361,280 B | 重启成功 |
| 6,000 ms | 258 ms | 908,001,280 B | 897,409,024 B | 重启成功 |

四次被杀进程退出码为 `-1`（进程级强杀预期结果），四次重启均能重新显示窗口并在关闭前保持运行。

强杀后对当前 AppData 下 10 个 SQLite 文件执行只读 `quick_check` 和 `integrity_check`，全部返回 `ok`。没有执行数据库修复或写入。

## 电池、发热和耗电

- Windows 电池：`FZ06083`，电量采样为 100%，状态 `2 / OK`。
- 电池报告：`C:\tmp\guxuanyou-real-baseline-20261007\battery-report.html`
  - 设计容量：73,568 mWh
  - 满充容量：73,568 mWh
  - 循环次数：83
- Windows 未暴露 `MSAcpi_ThermalZoneTemperature` 数据，当前无法从该接口得到可信温度。
- `powercfg /energy /duration 15` 未执行成功：当前 shell 没有管理员权限。不能把电池百分比变化或 CPU 时间伪装成瓦时耗电。

因此本次得到的是**进程内存/CPU与电池状态基线**，不是完整温度/功耗基线。要得到真实耗电，应使用 HWiNFO、Intel Power Gadget、厂商传感器或 Windows 管理员权限下的 ETW/PowerCfg 采集，并保持同一电源模式、亮度和网络状态。

## 磁盘满、文件系统异常、物理断电

本次没有直接填满系统盘，也没有切断电源：

- 当前 C 盘剩余约 122.07 GiB；用填盘方式测试会破坏机器可用空间，不在共享开发机上执行。
- `icacls` 只读 ACL 注入需要提升管理员权限，隔离 APPDATA 测试还会让 WebView2 依赖路径失效，未将其结果误记为产品失败。
- 物理断电不能由软件安全模拟；进程级强杀和已有 Rust 提交阶段故障注入只能覆盖部分场景。

已完成的替代证据：

- 现有 Rust 故障注入覆盖临时文件写入、同步、替换前后进程退出和 WAL busy。
- 当前安装实例强杀后所有 SQLite 文件完整性检查通过。
- 需要继续做真实磁盘/文件系统验收时，应使用专用测试 VM 或可挂载的临时 VHD，配置：小容量卷、独立 AppData、快照回滚、无真实用户凭据；物理断电则使用 VM checkpoint 或硬件测试机。

## 结论

当前 Windows 安装包可安装、可启动、强杀后可重启，且本次强杀没有造成已存在 SQLite 数据库损坏。主要风险信号是启动阶段峰值约 0.84–0.87 GiB 工作集，需在发布前继续拆分 WebView2/embedding/后台刷新贡献并在目标用户机器上复测。温度、真实瓦时耗电、磁盘满和物理断电仍未验证，不能标记为发布通过。
