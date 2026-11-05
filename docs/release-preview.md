# v0.1.0-alpha.1 开发者源码预览

仓库：[yoyicue/nxbridge](https://github.com/yoyicue/nxbridge)。本版面向参与开发和协议、媒体链路研究的开发者；完整家庭游戏服务仍在开发。

## 本次提供

- NXBridge Rust workspace、媒体 ABI、GameStream 兼容协议核心及本地管理台。
- 无游戏资源的合成画面 / 提示音、自检、Moonlight 启动与配对步骤。
- nxnative 与客户端输入的实验适配源码、补丁及来源说明。
- 有限队列、时钟、FEC、资源释放、断线等行为测试和 macOS CI。
- 设计、任务、历史实验记录，以及本次许可与内容审阅说明。

开始使用见 [快速开始](quickstart-synthetic.md)。历史报告中的 PID、设备别名及 `.nxbridge/` 路径只用于说明当时实验，不是公开部署配置；忽略目录的工件不随仓库提供。

## 不包含与未验证

本次不提供游戏、游戏构建、密钥、存档、凭据、Android APK、FFmpeg 或其他预编译运行时。Rust 依赖按锁文件获取，不把第三方 registry 缓存整体加入仓库。调用的系统库和 FFmpeg 由使用者自行安装。

完整游戏引擎和定制 Android 客户端仍缺公开可复现的构建链路。源代码中的实验 worker 位于 `tests/`，尚未拆为正式服务组件。BOTW 稳定 30 FPS、物理音画同步、端到端输入延迟、多人容量、锁屏和无显示器运行均不能由这次源码发布推导。

本机隔离源码构建会复用现有工具链与依赖；它不等于全新 Mac 安装。托管 CI 的实际结果以 [Actions](https://github.com/yoyicue/nxbridge/actions) 及 [NXB-105 发布记录](reports/NXB-105.md) 为准，合成文件自检不代表真实客户端验收。

## 许可与版本

自有代码默认 MIT；`nxbridge-gamestream` 使用 GPL-3.0-only；协议核心保留 BSD 上游内容及 GPL 集成修改的声明；FEC 包保留 BSD-2-Clause；客户端和 Sunshine 的派生部分遵循对应 GPL 条款。逐目录来源与许可证文本见 [来源清单](open-source-provenance.md)，Rust 依赖元数据见 `release-dependencies.json`。

四个 workspace 包与源码标签统一为 `0.1.0-alpha.1` / `v0.1.0-alpha.1`。上游包版本保留原值，补丁在各自 NOTICE 中记录。此次没有二进制发行物；以后增加 APK、应用包或原生库时，需要针对那些实际产物重新核对对应源码和许可。

安全问题使用 [私下反馈渠道](../SECURITY.md)。公开内容扫描与人工复核只覆盖这次源码候选，不构成全面安全或法律审计。
