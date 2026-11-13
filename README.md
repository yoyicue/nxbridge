# NXBridge

面向 NX2Apple / nxnative 的高性能局域网游戏串流方案。Mac 运行游戏，Android Pad 接收音视频并回传触屏或手柄输入。

**当前版本：`v0.1.0-alpha.1` 开发者源码预览。** Rust 合成音画服务端已通过单客户端原型验收，真实游戏离屏输出与双 Pad 独立画面已有实验链路；正式媒体和隔离验收、会话管理器、本机性能与多实例容量仍待完成。完整游戏引擎与定制 Android 客户端的公开构建流程尚未齐备。

NXBridge is an experimental Rust GameStream-compatible server for Apple Silicon macOS, with nxnative offscreen media and per-session input adapters. This source preview includes a game-free synthetic streaming demo. Game builds, assets, keys, saves and application binaries are not distributed. Stable game FPS, end-to-end latency and multi-player capacity are not yet validated.

从 [无游戏资源快速开始](docs/quickstart-synthetic.md) 编译、自检并连接 Moonlight；[版本说明](CHANGELOG.md) 和 [发布范围](docs/release-preview.md) 列出已实现能力及限制。

家庭场景面向 2–3 位玩家。设计按同时独立游玩预留实例隔离，同时支持轮流游玩、各自保存进度。实际并发数量由每台主机的测试结果决定，首轮测试从一个实例开始。

## 当前进展

- 实验游戏库已接入《旷野之息》《王国之泪》和《时之笛》入口。BOTW / TOTK 使用 nxnative，OoT 使用独立适配的 S2Arm 后端；各游戏的兼容性和性能分别验证。
- 两台实验 Pad 已接入各自的游戏服务、存档和缓存。OoT 已使用存档副本读档，BOTW / TOTK 已验证标题画面；双设备稳定游玩和并发容量仍待验收。
- Android 实验客户端支持按游戏编辑虚拟手柄布局。客户端 406 修复了浮动摇杆在触摸区域边界被裁切的问题，两台 Pad 已完成布局试按；游戏内实操回归仍待验证。见 [摇杆边界验证](docs/reports/NXB-120-stick-edge.md) 和 [客户端构建](docs/moonlight-experimental-build.md)。
- OoT 仍观察到长帧与音频间断，不能据此认定卡顿已经解决；最新部署与验证边界见 [上游更新记录](docs/reports/NXB-137.md) 和 [当前状态](docs/status.md)。

## 目标与技术路线

- 首个游戏：Switch 版 BOTW；首个主机：Apple Silicon Mac mini；首个客户端：Android 平板。
- 产品按多游戏设计；各游戏构建分别验证运行、音画、输入、存档与性能。后续 Web 管理、CLI 和 agent 共用 Host 管理 API。
- 每个被接纳的会话以稳定 30 FPS、低操作延迟为目标；不以串流接收 FPS 代替游戏 FPS。
- 目标为 Rust 原生的 Moonlight / GameStream 兼容服务端，直接管理 nxnative、接收离屏帧与 PCM，并通过 VideoToolbox / Opus 编码。
- NX2Apple / nxnative 构建工具和游戏引擎继续复用；正式运行链路无需 NX2Apple GUI 或独立 Sunshine。Moonshine 是 Rust 协议复用候选，Mac 后端需适配。
- Sunshine 保留为测量基线和过渡方案。首个原生版本以已登录、无可见游戏窗口为目标；无显示器、锁屏和未登录能力分别验证。
- 原生 macOS 多进程运行游戏。Lima、Docker、完整 macOS 虚拟机均非首版依赖。
- 独立玩家拥有独立存档、输入、画面和音频。多个客户端共享桌面不等于多实例。

## 从这里开始

| 文档 | 内容 |
| --- | --- |
| [AGENTS.md](AGENTS.md) | agent 的执行规则与交接要求 |
| [项目设计](docs/design.md) | 范围、用户流程、阶段与边界 |
| [系统架构](docs/architecture.md) | 模块、数据流和待验证的上游适配 |
| [游戏会话服务端设计](docs/game-session-server.md) | Rust 协议、离屏音画输出、服务封装、内存预算与实施阶段 |
| [Web 管理与多游戏支持](docs/web-management-and-game-library.md) | 管理页面、统一 API、游戏构建与配置、逐游戏验收及产品化顺序 |
| [多玩家与实例隔离](docs/sessions.md) | 生命周期、存档锁、资源调度、故障恢复 |
| [存储与清理边界](docs/storage-lifecycle.md) | 持久数据、缓存、会话临时文件及迁移限制 |
| [输入桥接口草案](docs/input-bridge.md) | 输入路由、释放、重连和体感 |
| [媒体接口 v1](docs/media-abi-v1.md) | 合成源与游戏源共用的时钟、缓冲与释放契约 |
| [输入桥迭代需求](docs/input-bridge-requirements.md) | BOTW 五个动作键、采样时序、人工接管与验收 |
| [虚拟手柄完整优化](docs/virtual-controller-optimization.md) | 触屏交互与 UI、布局编辑与配置、agent 调整、可靠输入及实施验收 |
| [实验客户端源码构建](docs/moonlight-experimental-build.md) | 固定上游的客户端补丁重放、本机编译和未验证边界 |
| [S2Arm / NS2 实验后端](adapters/s2arm/README.md) | OoT 离屏游玩、Pad 串流 / 触屏及读档短测已通；稳定 FPS 与游戏时序待验收 |
| [性能与验收](docs/validation.md) | 测量方法、单实例 / 并发验证门槛 |
| [Agent 工作流](docs/agent-workflow.md) | 任务领取、证据、评审与恢复上下文 |
| [当前状态](docs/status.md) | 已知事实、未验证项和下一步 |
| [开源来源与发布检查](docs/open-source-provenance.md) | 许可证、来源、分发边界及未完成的核查 |
| [技术预览候选](docs/release-preview.md) | 首次提交、候选范围及尚需完成的审核 |
| [参与贡献](CONTRIBUTING.md) | 开发、验证与提交约定 |
| [无游戏资源快速开始](docs/quickstart-synthetic.md) | 构建工作区、合成音画自检与已知限制 |
| [任务清单](tasks/backlog.json) | 机器可读的任务状态与依赖，任务状态的唯一来源 |
| [上游参考快照](config/upstreams.json) | 调研修订号；不是已验证的兼容依赖锁 |
| [设计决策](docs/decisions/0001-native-sessions.md) | 原生会话优先；Lima 不进入首版运行链路 |
| [Rust 与离屏输出决策](docs/decisions/0002-rust-offscreen-server.md) | 原生服务端目标、Sunshine 过渡定位和无窗口验收边界 |

## 许可证

除另有声明的部分外，NXBridge 自有代码和文档采用 [MIT License](LICENSE)。`nxbridge-gamestream` 集成包采用 GPL-3.0-only，协议核心保留 BSD 上游声明及 GPL 集成修改声明；`vendor/` 和客户端补丁中的第三方内容保留各自许可证。MIT 不覆盖第三方权利；逐目录来源、许可证文本和本次源码范围见 [开源来源清单](docs/open-source-provenance.md)。

## 仓库检查

文档检查与环境预检仅需 Python 3.10+；Rust workspace 检查需要本机 Rust / Cargo。检查不安装软件、不启动游戏、不更改设备。

```sh
make check
make check-rust
make check-native-media
make preflight
```

`make check` 检查文档链接、JSON、任务依赖和示例配置的一致性；`make check-rust` 离线检查 Rust workspace 是否可编译；`make check-native-media` 在 macOS 上运行无需 Pad 或游戏的媒体回归测试。这些检查不代表游戏、协议或性能通过。`make preflight` 只读检查当前 Mac 环境，JSON 输出到终端。

需要读取某台 Android 设备时显式指定序列号：

```sh
python3 scripts/preflight.py --adb-serial YOUR_DEVICE_SERIAL
```

本地配置从 [config/local.example.json](config/local.example.json) 复制到 `.nxbridge/local.json`，填入本机路径和设备标识。该目录已忽略。示例尚无运行服务读取，不代表配置开关已经实现。

## 目录与代码落点

```text
nxbridge/
├── Cargo.toml                 Rust workspace
├── crates/
│   ├── nxbridge-host/         主机与会话管理的源码落点
│   ├── nxbridge-media/        共用媒体 ABI 与租约管理
│   ├── nxbridge-stream/       GameStream 兼容串流的源码落点
│   └── nxbridge-gamestream/   实验 GameStream 服务端
├── adapters/
│   ├── input/                 输入桥与 Moonlight 客户端补丁
│   ├── nxnative/              nxnative 离屏适配与补丁
│   └── s2arm/                 S2Arm 实验后端与补丁
├── vendor/moonshine-core/      实验协议核心及许可证
├── vendor/fec-rs/              固定版本的 FEC 源码及矩阵构造补丁
├── tests/                     行为测试与设备探针
├── docs/                      设计、验收、决策和实验报告
├── tasks/                     任务状态与依赖
├── config/                    上游快照与本机配置示例
├── scripts/                   项目检查与环境预检
├── templates/                 报告与实验模板
└── .nxbridge/                 Git 忽略的本机副本、构建和实验数据
```

合成 GameStream 服务和实验性游戏离屏适配已实现；`nxbridge-host` 提供本机概览与游戏库登记原型，游戏离屏链路和完整会话管理尚未完成正式验收。现有 Sunshine / Moonlight 单会话输入原型见 [NXB-047](docs/reports/NXB-047.md)。游戏资源、密钥、存档和测量原始文件留在仓库外或忽略目录。

合成串流原型的运行方法、客户端验证、GPL 许可证与限制见 [NXB-032 报告](docs/reports/NXB-032.md)。

## 本机管理台

启动时指定本机实际监听 GameStream 的 IPv4 和端口：

```sh
cargo run -p nxbridge-host --offline -- --endpoint '串流入口=LOCAL_IPV4:HTTP_PORT'
```

在 Mac 本机打开 `http://127.0.0.1:48200/`。入口参数可重复，最多八个；不传入口时显示空状态。可增加 `--catalog FILE` 读取 [游戏库示例](config/game-catalog.example.toml) 格式的本机元数据；同时加 `--catalog-write` 可通过页面登记游戏、添加构建、首次绑定目录和修改名称。目录与私人数据留在本机，配置文件由 Host 独占管理，手动编辑前须停止服务。管理台不提供游戏启动、存档操作或局域网访问。范围和验证见 [NXB-041](docs/reports/NXB-041.md)。
