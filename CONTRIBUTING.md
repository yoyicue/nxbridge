# 参与 NXBridge

感谢帮助完善 NXBridge。当前是实验阶段，支持范围和已知限制见 [README](README.md)、[状态](docs/status.md)及[任务清单](tasks/backlog.json)。

## 开发与验证

1. 按[无游戏资源快速开始](docs/quickstart-synthetic.md)准备 Apple Silicon macOS、Rust 和原生依赖，执行构建和 `make check-synthetic`。首次构建需要联网下载依赖。
2. 开始较大的改动前，先描述问题、范围和验证方式，避免与现有工作重叠。游戏体验、设备和多会话验证请分别说明环境与限制；不要把原型观察写成产品能力。
3. 按改动范围运行 `make check`、`cargo test --locked --workspace`，以及适用的 `make check-native-media`、`make check-synthetic`；报告未运行的项目。CI 中的格式检查针对 NXBridge 自有 crate，保持 vendored 上游源码便于追踪。
4. 提交说明应包括问题、行为变化、测试结果和未解决限制。文档或界面截图要删去私人路径、设备标识和凭据。

不要提交游戏、游戏密钥、存档、证书私钥、配对状态、凭据、个人路径或未经授权的第三方文件。项目不会提供游戏内容。需要报告问题时，使用虚构或脱敏的最小示例；运行 `make check-release-content` 有助于发现一部分问题，但不能代替人工审核。

## 许可与来源

自有代码默认按 [MIT](LICENSE) 提交；`nxbridge-gamestream` 集成包当前保守按 GPL-3.0-only 管理，第三方文件仍适用各自声明，见[来源清单](docs/open-source-provenance.md)。只提交自己有权贡献的内容；引用或修改上游文件时保留其版权、许可和确切来源，说明修改与分发范围。提交不改变第三方内容的授权。

安全问题请参阅 [SECURITY.md](SECURITY.md)，不要在公开问题或提交中粘贴可利用细节或秘密。
