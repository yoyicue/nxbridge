# 变更记录

## v0.1.0-alpha.1 — 2026-11-05

首个开发者源码预览。四个 NXBridge workspace 包统一使用 `0.1.0-alpha.1`，vendored 包保留上游版本并在 NOTICE 中说明修改；不向 crates.io 发布。

- Rust GameStream 兼容服务端，提供无游戏资源的合成 H.264 / Opus 串流、Moonlight 配对和输入原型。
- 共享媒体 ABI v1、nxnative 离屏 IOSurface / PCM 与会话输入的实验适配。
- 本机 Web 管理台、游戏库登记及构建目录绑定；完整会话与家庭玩家流程仍在开发。
- 音频源时间戳与间断处理、有界补偿、发送背压和过期 PCM 淘汰。
- FEC 直接矩阵构造与有界缓存；视频发送确认、最新原始帧选择及阶段耗时诊断。
- 协议与原生媒体回归、合成音画自检、源码隔离构建脚本和 macOS CI。
- 明确自有 MIT、GPL 集成、BSD vendored 源码与上游补丁的许可范围。

本次仅提供源码及文档，不附游戏、密钥、存档、Android APK 或预编译应用。完整游戏与客户端构建仍有本机依赖；稳定 30 FPS、input-to-photon、长期音画同步和多人容量尚未验收。详见 [发布范围](docs/release-preview.md) 与 [当前状态](docs/status.md)。
