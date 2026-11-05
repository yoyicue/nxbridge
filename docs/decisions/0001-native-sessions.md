# ADR-0001：原生 macOS 实例与现有串流协议

日期：2026-11-02。状态：首轮实验的暂定方案；性能和兼容性结论待验证。

后续决策：[ADR-0002](0002-rust-offscreen-server.md) 将正式媒体后端目标调整为 Rust GameStream 服务端和 nxnative 离屏输出；本 ADR 的原生多进程与无需虚拟机原则继续生效，Sunshine 保留为基线与过渡。

## 背景

NXBridge 需要让家庭 2–3 位玩家使用 Pad 操控 Mac 上的 NX2Apple / nxnative 游戏，并希望复用开源基础设施。用户询问了实例化、虚拟化和 Lima。

## 决策

- 游戏、Metal 渲染、采集与编码在原生 macOS 运行。
- 每个独立玩家运行独立游戏进程，并隔离所有可写数据、媒体源、控制权和串流 worker。
- 先以 Sunshine / Moonlight 建立单会话基线；正式媒体路线和多实例验证按 ADR-0002 推进。
- 首版不依赖 Lima、Docker 或完整 macOS VM。控制服务也无需为容器化额外引入虚拟机。
- 如果本机容量不足，排队或增加原生 Mac worker；不把虚拟机数量当作 GPU 容量。

## 原因与限制

Lima 的 macOS guest 当前为实验性功能；krunkit 的 GPU 路线是实验性的 guest Vulkan 能力，不能直接据此推出 macOS guest 中 nxnative / Metal 的兼容性或三路 30 FPS。

进程隔离降低环境开销，能直接使用宿主机原生图形 API，但仍需解决窗口遮挡、音频归属、存档目录和跨实例输入。它不提供对恶意租户的强安全隔离。

若后续需要不可信用户隔离、Linux 辅助服务或已有 VM 环境，再建立独立 ADR 和性能实验；此决策不是声称所有虚拟化方案都没有 GPU 支持。

## 参考

- [Lima macOS guests](https://lima-vm.io/docs/usage/guests/macos/)
- [Lima krunkit](https://lima-vm.io/docs/config/vmtype/krunkit/)
- [Sunshine 稳定参考版 macOS 限制](https://github.com/LizardByte/Sunshine/blob/63d35f702ee9e362e43263742981836ec0710384/docs/getting_started.md#macos)
- [ScreenCaptureKit 窗口过滤](https://developer.apple.com/documentation/screencapturekit/sccontentfilter)
- [nxnative 窗口生命周期](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/dm/host/front_mac.m)
