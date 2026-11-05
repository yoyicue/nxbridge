# 调研来源

检查日期：2026-11-02。精确修订号见 [上游快照](../config/upstreams.json)。这些来源支持设计中的事实；它们不替代本机测试。

| 来源 | 用途与限制 |
| --- | --- |
| [nxnative README](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/README.md) | Mac 构建与默认工具链要求 |
| [BOTW profile](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/dm/games/01007EF00011E000/README.md) | 帧时间机制与上游硬件观察；不是本机保证 |
| [nxnative 开发说明](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/docs/development.md) | DM_SAVE / DM_ROOT、Mac 运行和基准入口 |
| [Mac front end](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/dm/host/front_mac.m) | 遮挡、焦点、最小化和输入的实现约束 |
| [存档实现](https://github.com/seedds/nxnative/blob/ba366d26a24d5018cd095947187314e8526e0ca4/dm/host/hle_fs.c) | DM_SAVE、APFS 事务和 device save |
| [Sunshine 稳定参考版配置](https://github.com/LizardByte/Sunshine/blob/63d35f702ee9e362e43263742981836ec0710384/docs/configuration.md) | 显示器选择、端口组，不提供本设计的多实例证明 |
| [Sunshine 稳定参考版安装说明](https://github.com/LizardByte/Sunshine/blob/63d35f702ee9e362e43263742981836ec0710384/docs/getting_started.md) | macOS 输入限制与原生音频采集 |
| [libvirtualhid 许可证说明](https://github.com/LizardByte/libvirtualhid#-license) | 库与 Broker 许可边界不同；页面会随主线变化 |
| [Moonlight Android](https://github.com/moonlight-stream/moonlight-android) | 客户端复用候选 |
| [Moonshine README](https://github.com/hgaiser/moonshine/blob/79554f853135cbac44502b58093a4eb0f0f5e6f1/README.md) | Rust 服务端参考；Linux only，不代表 Mac 可直接使用 |
| [Moonshine Cargo.toml](https://github.com/hgaiser/moonshine/blob/79554f853135cbac44502b58093a4eb0f0f5e6f1/Cargo.toml) | 平台耦合与依赖审计；所查版本 0.16.1，未在 Mac 构建 |
| [Moonlight 协议核心](https://github.com/moonlight-stream/moonlight-common-c) | GameStream 客户端实现，可作兼容参照，不是服务端库 |
| [Apple IOSurface](https://developer.apple.com/documentation/iosurface) | 跨进程图像缓冲共享；具体同步与编码组合需验证 |
| [Apple VideoToolbox](https://developer.apple.com/documentation/videotoolbox/vtcompressionsession) | 视频帧压缩会话；不证明本项目离屏链路已实现 |
| [Moonlight 配置指南](https://github.com/moonlight-stream/moonlight-docs/wiki/Setup-Guide) | 客户端和局域网连接建议 |
| [Apple SCContentFilter](https://developer.apple.com/documentation/screencapturekit/sccontentfilter) | 指定窗口过滤；不证明游戏在后台持续渲染 |
| [Apple ScreenCaptureKit 介绍](https://developer.apple.com/videos/play/wwdc2022/10156/) | 音频过滤在应用层；同应用多进程隔离另需验证 |
| [Lima macOS guest](https://lima-vm.io/docs/usage/guests/macos/) | 实验性 guest，非本项目依赖 |
| [Lima krunkit](https://lima-vm.io/docs/config/vmtype/krunkit/) | 实验性 Vulkan GPU 路线，不等同于 native Metal 能力 |

Sunshine / Moonlight 的主许可证为 GPL-3.0，nxnative / NX2Apple 主许可证为 MIT；第三方代码依各自声明。NXBridge 自有代码原则上采用 MIT；GameStream 集成包目前保守声明 GPL-3.0-only，vendored 上游内容保留各自许可与声明，具体分发审核见 [开源来源与发布检查](open-source-provenance.md)。不分发游戏、密钥或构建产物。

Moonshine 仓库标注 BSD-2-Clause；复用前按固定修订核对具体文件和依赖声明。其 Linux 合成器、音频和 Vulkan 编码栈不是 NXBridge 的目标运行依赖，协议可抽取范围在 NXB-032 验证。
