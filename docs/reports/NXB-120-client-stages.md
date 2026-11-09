# NXB-120：客户端输入阶段采样

日期：2026-11-09。任务状态以 `tasks/backlog.json` 为准。

## 环境和测量边界

1372 Pad：TALIH-PD2、Android 13，经 Wi-Fi 连接 Mac M4、16 GiB、macOS 26.5。客户端使用固定 Moonlight Android `b48494cb96bff23d8886c4775cc4f39a1075495d` 和本项目 363 补丁的隔离副本，另加 `tests/fixtures/client-stage-instrumentation.patch`，构建临时诊断版 364；最终两台均恢复为没有该探针的 365。BOTW 构建 `v1114112`，离屏 nxnative / Mach 帧源。测试等待视频画面，但没有截图确认游戏具体场景；本次没有独立记录串流协商设置。

对 ZL 的触控 down / up 记录两段时间：Android `MotionEvent.getEventTime()` 到 `VirtualControllerElement.onTouchEvent` 入口，二者使用设备 uptime 毫秒时钟；入口到触屏来源调用 `NvConnection.sendControllerInput` 之前使用 `System.nanoTime()` 的同进程时间差。调用边界不等于 JNI 内排队成功、无线发送或主机接收。ADB `input swipe` 生成的是软件触摸事件，事件时间不是实际手指或触摸硬件时间。事件时间的毫秒量化和线程调度影响第一段；采样用 20 次约 100 ms 按放，间隔约 350 ms，样本少且节奏固定。诊断探针在触控回调结束后输出 logcat，日志可能扰动后续调度，影响没有量化；不能将其作为无探针版本的性能结论。

## 本次结果

20 次按放共 40 条客户端事件按顺序匹配并均出现控制发送调用边界；同次主机跟踪另外观察到 40 个对应游戏 HID 输入结构取样。两个设备时钟未校准，不对客户端与主机的时间戳相减。

| 客户端阶段 | 按下 p50 / p95 | 释放 p50 / p95 |
| --- | ---: | ---: |
| 软件事件时间 → 触控回调入口 | 1 / 2 ms | 0.5 / 1 ms |
| 触控回调入口 → 控制发送方法调用前 | 0.069 / 0.110 ms | 0.048 / 0.070 ms |

p95 使用最近秩次，每组仅 20 个样本。以上不包含真实触摸硬件、网络发送与传输、游戏逻辑、渲染或屏幕显示；固定注入节奏下按放数值的差异不能推广。没有同条件 UI 显示开 / 关 A/B，也没有外部 input-to-photon 测量，VC-13 仍未完成。

## 验证与恢复

- 原始客户端日志 `.nxbridge/controller-120/timing-clientstage-client.log`、主机日志和游戏取样 `timing-clientstage-server.log`、`timing-clientstage-game-input.txt`、样本文件 `timing-clientstage-results.json` 与 `client-stage-analysis-checked.json` 均留在本机忽略目录；行为日志不自动上传。客户端以 [分析器](../../tests/client_stage_analysis.py) 验证边沿数、顺序、时钟差和调用边界；分析器单元测试通过。同次主机样本由 `tests/input_stage_analysis.py` 匹配 40/40。
- 测试前备份 1372 存档，结束后其 20 个文件 SHA-256 不变；临时诊断游戏与 worker 正常退出，原 1372 服务恢复空闲。PSO 预热仅在测试服务临时关闭，已恢复原启动配置。
- 两台 Pad 已安装相同的无探针 365，安装包哈希与构建结果一致，且检查该 APK 的 DEX 不含诊断日志标识。365 相比 363 只增加版本号，以覆盖受管安装器不允许直接按同一版本替换的临时 364。两台 363 APK 分别备份在本机 `.nxbridge/controller-120/1402-before-clean-365.apk`、`1372-before-client-stage.apk`；应用数据和配对未清除。
- 诊断补丁应用到记录的 363 文件后可重建四个诊断源文件；它只用于实验，不进入正常客户端补丁序列。离线 Gradle 构建及 `make check` 通过；本次没有实测真实手指、实体手柄、长时间稳定或真实游戏内输入到画面延迟。

后续状态：两台客户端已更新为不含诊断开关的 368；后续主动触屏对照见 [记录](NXB-120-touch-draw-ab.md)。
