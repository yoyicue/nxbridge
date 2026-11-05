# 开源来源与发布前核对

本清单记录 2026-11-05 的 `v0.1.0-alpha.1` 源码预览范围。自有代码默认 MIT，GameStream 集成沿用 GPL-3.0-only；上游内容保留其许可和版权。此次没有二进制发行物，也不包含完整第三方 registry 源码；未来应用包、APK 和原生库需要按实际产物另行核对。本清单不是完整法律或安全审计。

| 组件 | 来源和本次使用 | 许可信息 / 待核实 |
| --- | --- | --- |
| NXBridge 自有代码、文档与补丁 | 本仓库各目录；权利归属需由贡献者确认 | 除另有声明部分外，采用根目录 MIT；host、media、stream 的 Cargo 包声明 MIT。`nxbridge-gamestream` 包仍保守声明 GPL-3.0-only；上游文件与补丁中包含的上游内容保留原有许可。 |
| GameStream 协议核心 | [rgsp-cast 子树](https://github.com/codebutler/rgsp-cast/tree/2eeb0fa63221a9e72dfc51ea77a01a3be8ff0e1e/vendor/moonshine/moonshine-core)，`2eeb0fa63221a9e72dfc51ea77a01a3be8ff0e1e`，含本地修改 | 上游 Moonshine 的 BSD-2-Clause 文本和 rgsp-cast 根目录 GPL-3.0 文本均保存在 `vendor/moonshine-core/`。已核对固定修订 README，其 `vendor/moonshine/` 子树标为 BSD-2-Clause；保留原始版权文本。NXBridge 集成修改继续使用 GPL-3.0-only，包元数据以 `BSD-2-Clause AND GPL-3.0-only` 表达组合内容，不改写上游 BSD 授权。详情见 [来源声明](../vendor/moonshine-core/NXBRIDGE-NOTICE.md)。 |
| Rust 依赖 | [Cargo.lock](../Cargo.lock) 和 `release-dependencies.json`，针对 `aarch64-apple-darwin` 的 Cargo metadata | 234 条 package 记录均有许可表达式；清单含开发和构建依赖。除两个 vendor 包外，registry 源码由构建工具按锁文件下载，不包含在本次源码归档。元数据检查不等于二进制发行物 SBOM 或全部条款审计。 |
| fec-rs | 2026-11-05 纳入 crates.io `fec-rs` 0.1.0，包内修订 `402f41f970bb8c3ef7c88639821032e3b6460f63`；增加直接矩阵构造与兼容测试 | 保留 BSD-2-Clause 原文。包 SHA-256、改动边界及双 Cargo 入口说明见 [来源声明](../vendor/fec-rs/NXBRIDGE-NOTICE.md)。 |
| nxnative 游戏引擎与 NX2Apple 构建工具 | 实验组合见 [实验锁](../config/nxb010-experiment-lock.json)：引擎派生提交 `b8d16169c5fb7a3c6a936a950e294b1df4680551`，NX2Apple `41f7d6ba920927856316f6e04af1c50e437eb9c3` | 本机引擎固定修订的 LICENSE 为 MIT，补丁上下文的原版权文本保留于 `adapters/nxnative/LICENSE.upstream`；构建工具及完整运行时不随本版提供。游戏及其资源、密钥、存档不随项目分发。实验构建补丁目前仍在本机忽略目录，锁文件不是完整构建材料。 |
| Moonlight Android 与 NXBridge 客户端补丁 | 本机实验使用 Moonlight Android `b48494cb96bff23d8886c4775cc4f39a1075495d`；仓库内 `adapters/input/` 有补丁和脚本 | GPL 派生内容和原始上下文保留对应条款，源码范围见 [输入 NOTICE](../adapters/input/NOTICE.md)，GPL 文本见 [LICENSES](../LICENSES/GPL-3.0-only.txt)；本版不分发客户端或其子模块。当前未证明从公开材料重建实验 APK。 |
| FFmpeg | 合成源调用本机独立 FFmpeg 进程；本机 `ffmpeg -buildconf` 包含 `--enable-gpl` 和 `--enable-version3` | 具体发行物取决于来源与编译选项；本机配置不能代表其他机器。依赖自己的许可和源码交付要求，见 [FFmpeg 法律说明](https://ffmpeg.org/legal.html)。 |
| Opus 与系统框架 | Rust 依赖及本机原生库；Apple 系统框架 | 以实际链接的版本和分发方式核对声明及再分发条件；尚未完成二进制物料清单。 |

GPL 与兼容的宽松许可模块可以组合，但不能因此忽略组合程序的 GPL 条件；具体权利义务取决于真实的代码和分发方式，单独进程也不能仅凭名称推断法律结论。参见 [GNU GPL FAQ](https://www.gnu.org/licenses/gpl-faq.en.html#GPLModuleLicense)。此处的保守处理不替代权利人确认或必要的法律审查。

## 机械检查与人工复核

`make check-release-content` 只扫描 Git 当前将纳入的已跟踪和未忽略文件；它启发式检查常见游戏、凭据文件类型、私钥标记、本机路径、大文件、二进制和符号链接，不显示命中内容。它无法发现所有敏感信息，且可能误报。`make check-release` 增加根许可证存在性检查；该检查通过不代表其他发行条件已经满足。两者均不能确认许可证是否合法、依赖是否完整、分发包是否与候选文件一致。

本次已对源码候选执行机械扫描并复核来源、测试夹具及历史报告；后续每次发行仍需审阅实际提交差异和最终发行包：核对贡献者权利、所有依赖与声明、完整对应源码及构建说明、游戏资源与密钥存档、凭据及私人路径；对误报先确认内容，再有针对性地调整规则，不要全局禁用检查。不同客户端、服务端和外部分发依赖需分别确认。
