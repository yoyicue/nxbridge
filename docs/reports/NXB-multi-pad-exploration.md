# 单路资源采样与双 Pad 探索

日期：2026-11-03。探索性实验，不改变 backlog 的验收状态。

## 目标与范围

采样一台 Pad 使用 BOTW 的主机资源，并准备使用独立存档、服务端身份和端口的第二路，探索双设备是否可以独立游玩。已有单路实验使用 Rust GameStream、nxnative 离屏输出、VideoToolbox H.264、PCM / Opus，Pad 设置为 720p / 30 FPS。

## 验证证据

Mac mini M4、10 核 CPU、16 GB 统一内存。加载户外存档，人物静止；2026-11-03 06:54:46–06:54:58 本地时间以 `ps` 每秒采 12 次，分别获得每个进程 CPU 和 RSS：

| 进程 | 平均 CPU | 平均 RSS |
| --- | ---: | ---: |
| 游戏 dmhost | 86.4% | 1468.9 MiB |
| GameStream 服务端 | 13.2% | 21.1 MiB |
| 媒体 worker | 1.4% | 29.1 MiB |

CPU 的 100% 表示一个核，采样期间合计约 101%。macOS `footprint` 另一次测量游戏为 2777 MB、服务端 10 MB、worker 10 MB；游戏值包括图形分配。RSS 和 physical footprint 不是同一口径，不可相加，进程间共享分配也不能简单合计为系统总内存。原始 12 个样本位于本机忽略目录 `.nxbridge/multi-pad/single-samples.json`；footprint 结果来自本次命令输出。此为短时探索，期间进行了客户端截屏和 footprint 检查，不符合正式性能规程；GPU 利用率、系统压力、swap、端到端延迟和稳定性均为 unknown。

第二路已准备独立的服务端身份、端口组、存档及缓存目录；存档来自先前的静态备份，另留备份副本。新增可选 `NXB_HOST_NAME`，用于区分实验主机名称，默认名称不变。第二台 Pad 最初 USB 传输挂起，重新连接后通过 xpad2 安装 NXBridge Pad 320，配对第二路，设置 720p / 30 FPS，并成功启动 BOTW。临时切换的输入法已恢复。

两台 Pad 同时显示不同的户外存档场景，两个服务、两个 worker 和两个游戏进程同时运行。依次在每台 Pad 按下并释放 `+`，两次都只在对应游戏的输入跟踪中看到 `1024` 与释放，另一侧没有相应事件；本机证据见 `.nxbridge/multi-pad/input-isolation.json` 及截图。这仅覆盖一次按钮隔离，音频隔离、保存重启、重复写者拒绝和故障隔离尚未验证。

07:19:02–07:19:14 同时对两路各进程进行 12 次 `ps` 采样：第一路游戏 / 服务 / worker 平均 CPU 分别为 54.0% / 8.7% / 1.2%，第二路为 96.3% / 10.1% / 1.2%；总计约 171.5%（100% 为一个核）。期间游戏进程 RSS 平均分别为 699.6 MiB 和 759.9 MiB；另行调用 `footprint` 时，分别为 2788 MB 和 2696 MB。系统一次性读数为 swap 已用 122.06 MiB，系统空闲百分比 30%；没有实验前基线，无法归因。原始样本及 footprint 输出在 `.nxbridge/multi-pad/`。两路场景不同、短时采样且包含诊断干扰，不是正式性能或容量验收；内存口径和共享分配的限制同上。

运行 `make check`、`make check-rust`、`cargo test -p nxbridge-gamestream --offline` 与 `cargo fmt -p nxbridge-gamestream -- --check` 均通过。工作区全量 `cargo fmt --all -- --check` 未通过，输出包含既有 `vendor/moonshine-core` 格式差异；未修改上游格式。测试不证明双实例可用。

## 恢复与交接

第一路单路采样后曾因 5 秒未收到心跳而正常退出游戏；重新连接后与第二路并行运行。交接时两路均保持运行，可在本机 `.nxbridge/experiment-035` 和 `.nxbridge/multi-pad` 核对 PID、状态与日志。第二路服务退出后可使用忽略目录内 `.nxbridge/multi-pad/start-host2.py` 恢复。下一步由用户体验双 Pad 操控和声音，再验证存档、音频、生命周期、逐路性能和异常隔离；不得把探索结果登记为已验证容量。
