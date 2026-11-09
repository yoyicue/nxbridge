# NXB-120：主动触屏绘制对照

日期：2026-11-09。任务状态以 `tasks/backlog.json` 为准。

## 方法

Mac M4、16 GiB、macOS 26.5；1372 Pad 为 TALIH-PD2、Android 13、Wi-Fi。服务端使用 NXBridge 的合成音画源；客户端配置 1280×720、30 FPS、5 Mbps，实际协商结果未独立记录。三次会话使用同一种带运动方块和帧号的合成源，但启动帧号不同；本次不运行 BOTW，不代表游戏表现。设备显示刷新周期由 SurfaceFlinger 报告为 16.67 ms。

使用固定 Moonlight Android `b48494cb96bff23d8886c4775cc4f39a1075495d` 和 NXBridge 366 补丁的隔离客户端，额外应用 `tests/fixtures/client-draw-ab.patch` 构建诊断版 367。按正常绘制 A1 → 跳过控件 `onDraw` B → 正常绘制 A2 顺序，各运行约 18 秒。B 只跳过 `VirtualControllerElement.onDraw`，仍保留相同布局、触摸命中、状态更新和 `invalidate`；因此它是探究控件绘制代码的诊断条件，并非可正常使用的产品模式。每组使用 Android `uinput` 对 L 按钮注入 18 次约 100 ms 的触摸；合成服务在每组均记录 18 次对应按下和释放。模拟触摸不代表真实手指的时序、组合或握持。

每组预热后重置应用 `gfxinfo`，读取游戏 Activity 窗口的 `framestats`；只计算 flags 为 0 且时间有效的 `IntendedVsync → FrameCompleted`，这是整个应用 UI 帧的调度及完成时间，不是单独的 CPU 绘制时间。并周期性采集视频 SurfaceFlinger 呈现时间，快照有重叠、按时间戳去重。截图只在采样前用于确认实验条件，不在性能采样期间持续截图。每组只有一次，未随机化；三组开始时运动帧号、网络和热状态可能不同。

## 结果

| 条件 | 有效 UI 帧数 | UI 帧时间 p50 / p95 | 视频层呈现 FPS | 视频呈现间隔 p50 / p95 / p99 |
| --- | ---: | ---: | ---: | ---: |
| A1 正常绘制 | 35 | 2.93 / 3.30 ms | 29.97 | 33.34 / 33.35 / 50.00 ms |
| B 跳过绘制 | 0 | unknown | 29.97 | 33.34 / 33.36 / 50.01 ms |
| A2 正常绘制 | 36 | 2.97 / 4.06 ms | 30.03 | 33.34 / 50.00 / 50.01 ms |

百分位数使用最近秩次；视频帧数依次为 541、541、542。B 的 `gfxinfo` 汇总为 0 帧，帧表中唯一一条为无效标志，故没有可用帧时间；这不表示触摸处理、合成或 GPU 开销为零，也不能从 A 和 B 相减得到控件 CPU 成本。A1 的系统汇总为 36 帧，但帧表中可用样本为 35；表格使用后者。视频呈现是 SurfaceFlinger 的视频层时间，不能当作游戏帧率或屏幕 input-to-photon。三组短时结果不能证明 UI 对任何游戏性能没有影响。

原始 `draw-ab-draw{a1,b,a2}-raw.json`、对应 `-gfx.txt`、分析 JSON、截图和 `draw-ab-synthetic.log` 保存在本机忽略目录 `.nxbridge/controller-120/`。`tests/surface_present_analysis.py` 检查快照重叠并去重，`tests/gfxinfo_framestats_analysis.py` 筛选窗口、无效标志及时间；各自的测试通过。

## 恢复与未验收

临时合成会话和服务正常退出，原 1372 游戏服务恢复空闲；实验未启动游戏或修改存档。1372 的原偏好文件逐字节恢复，临时虚拟触摸设备退出。两台 Pad 均安装同一份不含诊断开关的 368 APK，哈希与构建产物一致，DEX 中未发现诊断偏好键；368 相对 366 只递增版本以覆盖临时诊断版 367。此前 366 APK 备份在本机 `.nxbridge/controller-120/1402-before-clean-368.apk`、`1372-before-draw-ab.apk`，配对和应用数据保留。离线 Gradle 构建、诊断补丁在 366 源文件上重建检查、分析器测试和 `make check` 通过。

仍缺少可比较的 B 组 UI 资源消耗、主动触屏输入延迟 A/B、真实 BOTW 同场景测量、端到端 input-to-photon、真实手持和实体手柄验证；VC-13 不能因本次实验标记完成。
