# ADR-0002：Rust 原生 GameStream 服务端与 nxnative 离屏输出

日期：2026-11-02。状态：目标架构已确定；实现、兼容性与性能待验证。本决策更新 [ADR-0001](0001-native-sessions.md) 中的媒体后端路线，继续沿用原生 macOS 多进程和数据隔离原则。

## 背景

用户希望 NXBridge 直接提供服务，减少外部启动器和串流主机依赖，并使 Mac 无需显示游戏窗口。此前以 Sunshine / Moonlight 建立了探索性桌面串流，USB 输入原型已到达游戏采样；原生 Rust 服务端和离屏运行尚未实现。

NX2Apple 负责构建与启动封装，游戏引擎是 nxnative；Sunshine 是 Moonlight 兼容服务端的一个实现。可接管运行管理并实现 GameStream 兼容协议，同时继续复用游戏引擎、硬件编码和客户端。

## 决策

- NXBridge 的 Host 和会话媒体 worker 采用 Rust；运行时与 Apple 平台接口通过绑定或少量 C / Objective-C 适配接入。
- 保留 nxnative 引擎和既有构建工具，NXBridge 管理构建编排、版本、启动、存档与生命周期。正式运行时无需 NX2Apple GUI。
- 原生媒体 worker 接收游戏离屏纹理和 PCM，以 VideoToolbox / Opus 编码，直接服务 Moonlight；正式原生链路无需独立 Sunshine。
- 优先审计 Moonshine 的 Rust 协议实现，固定调研修订 `79554f853135cbac44502b58093a4eb0f0f5e6f1`。它是 Linux 项目，协议可复用程度与 Mac 适配成本先由原型确定，不直接引入全部平台栈。
- Sunshine 保留为单路基线、协议行为对照和必要过渡。窗口级采集为可选实验，不作为原生路线的强制前置里程碑。
- 每位独立玩家保留独立游戏进程和媒体 worker；控制服务共享，媒体缓冲、输入状态和可写数据隔离。
- 首个原生版本要求在已登录用户会话中无可见游戏窗口运行。无物理显示器、锁屏、未登录分别记录能力；不以其中一项推导其他项。

## 原因与代价

直接控制帧、PCM 与会话生命周期，使多玩家路由、无窗口运行和缓冲预算有一致边界。Rust 原型需要自行承担配对、协商、媒体分包 / 纠错 / 加密、输入和恢复兼容；依赖 Moonshine 的现有实现也不能免除这些验证。

游戏仍使用 GPU 完整渲染。Rust、减少产品依赖和取消桌面采集都不自动证明内存下降，内存与性能收益须和 Sunshine 在同条件下比较。不得把所有游戏合并进一个进程来替代故障隔离。

## 验证与切换

先完成 Rust 合成音画协议原型（NXB-032）和运行时输出原型（NXB-035），再完成真实游戏单路接入（NXB-037）。随后验证多会话与生命周期，再登记容量。客户端兼容或离屏时序失败时保留证据和既有基线，不以窗口采集冒充无窗口成果。

完整接口、内存证据与支持矩阵见 [服务端设计](../game-session-server.md)；任务状态见 [backlog](../../tasks/backlog.json)。本 ADR 未新增运行代码或改变已启动的实验会话。

## 来源

- [Moonshine README](https://github.com/hgaiser/moonshine/blob/79554f853135cbac44502b58093a4eb0f0f5e6f1/README.md) 与 [Cargo.toml](https://github.com/hgaiser/moonshine/blob/79554f853135cbac44502b58093a4eb0f0f5e6f1/Cargo.toml)：Rust、Linux 与平台依赖。
- [Moonlight 协议核心](https://github.com/moonlight-stream/moonlight-common-c)：客户端 GameStream 实现。
- [Apple IOSurface](https://developer.apple.com/documentation/iosurface) 与 [VideoToolbox 压缩会话](https://developer.apple.com/documentation/videotoolbox/vtcompressionsession)：共享图像缓冲与视频帧编码基础。
