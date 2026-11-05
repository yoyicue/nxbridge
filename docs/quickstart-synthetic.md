# 开发者快速开始：无游戏资源自检

本指南覆盖 Apple Silicon macOS 的 Rust 工作区和本地合成音画检查，不需要游戏、密钥、存档或 Android 设备。它是技术预览的开发入口；游戏引擎、正式 worker、Android 客户端和安装包还没有完整的公开构建流程。

## 准备

已验证环境：macOS 26.5、Apple Silicon、Xcode 26.5、Rust / Cargo 1.97.0、FFmpeg 8.1.1 和 Opus 1.6.1。其他版本尚未验证；`Cargo.lock` 固定 Rust 依赖版本，但并不固定系统工具链及原生依赖。需准备 Xcode 或可用的 Apple 命令行编译工具、Python 3.10+、Rust / Cargo、CMake、pkg-config、Opus 和 FFmpeg（包括 ffprobe、FFV1 编码器与 PCM 编解码）。首次 Cargo 构建需要联网下载锁定依赖；这些工具的安装方式由使用者选择，例如已安装 Homebrew 的开发者可以安装对应 formula。Apple 开发工具可能需要在首次使用时单独安装和接受许可。

例如，已安装 Homebrew 和 rustup 的开发者可运行以下命令安装其余依赖并选择本次验证的 Rust 版本：

```sh
brew install cmake pkgconf opus ffmpeg
rustup toolchain install 1.97.0 --component rustfmt --component clippy
rustup override set 1.97.0
```

最后一条命令会设置当前项目目录的 Rust 版本覆盖；需要撤销时在同一目录运行 `rustup override unset`。这些安装步骤尚未在全新 Mac 验证。

## 构建与本机自检

获取源码，在项目根目录执行：

```sh
git clone https://github.com/yoyicue/nxbridge.git
cd nxbridge
make check
cargo build --locked --workspace
make check-synthetic
```

自检生成 60 帧变化画面和两秒立体声提示音，在系统临时目录中使用 FFmpeg 的 FFV1 / PCM 完成编码、属性检查和解码，然后删除自己创建的临时目录。它检查本地源和文件编解码，并不测试 GameStream 协议、VideoToolbox、Opus 串流、Moonlight 或音画延迟。

执行 `make check-clean-source` 可将 Git 已跟踪及未忽略候选文件复制到临时源码树，独立编译整个 workspace 并运行相同自检；这会复用本机 Rust、原生库和 Cargo 缓存，**不等于全新 macOS 安装验证**。仅当依赖已缓存时，可用 `NXB_CARGO_OFFLINE=1 make check-clean-source` 禁止 Cargo 下载。临时目录仅包含该检查新建的数据，结束时自动清理。

## 合成串流原型

`nxbridge-gamestream` 在无 `NXB_GAME_WORKER` 环境变量时提供合成源，使用外部 FFmpeg 的 VideoToolbox 编码。Mac 和 Moonlight 客户端应在同一可信局域网内；使用空闲端口组，防火墙提示时允许所选局域网连接。

先确认 `ffmpeg -encoders` 包含 `h264_videotoolbox`。把下面的 `MAC_LAN_IPV4` 替换成这台 Mac 的实际局域网 IPv4（不能填 `0.0.0.0`）：

```sh
mkdir -p .nxbridge/preview-state
env -u NXB_GAME_WORKER cargo run --locked -p nxbridge-gamestream -- \
  MAC_LAN_IPV4 .nxbridge/preview-state 48989
```

1. 在 Moonlight 手动添加 `MAC_LAN_IPV4:48989`；服务名为 `NXBridge synthetic 032`。
2. 客户端显示四位配对 PIN 后，在项目根目录的另一个终端运行 `printf '%s\n' '1234' > .nxbridge/preview-state/pair-pin`，将 `1234` 替换为屏幕上的 PIN。服务仅在恰好一个客户端等待配对时读取并删除文件。
3. 选择 H.264、720p、30 FPS、立体声，启动 `NXBridge synthetic`。预期画面为移动图形和帧号，伴随周期提示音；这不运行游戏。
4. 退出客户端串流，再在服务终端按 `Ctrl-C`。配对状态保留在私有目录，下次启动可以复用。

这个端口组使用 TCP `48989`（HTTP）、`48984`（HTTPS）、`49010`（RTSP），以及 UDP `48998`（视频）、`48999`（控制）、`49000`（音频）。改变基准 HTTP 端口会同步改变相对偏移。不要与现有 Sunshine / NXBridge 服务使用同一地址和端口组，不要把证书、私钥或 PIN 文件加入版本库。

真实客户端的历史验收及局限见 [NXB-032](reports/NXB-032.md)。本次发布的自动自检仍不包含 Moonlight 画面或音画同步验收。

## 排障和限制

- `cargo build` 报无法下载或离线缓存缺失：确认网络、Cargo registry 配置及 `Cargo.lock`，首次构建不要启用离线模式。
- 原生编译报缺少 CMake、Apple SDK 或 Opus：检查工具是否在 PATH 中，运行 `pkg-config --modversion opus` 并检查 Xcode 工具链。
- 自检报找不到 FFmpeg、ffprobe 或 FFV1：检查 `ffmpeg -encoders` 和 PATH；FFV1 仅用于本地自检，并非产品串流编码。
- 本地自检不证明网络、游戏、输入、同步、性能及断线恢复。音频桥现已携带源时间戳和间断信息，并通过有界补偿和积压回归；实际 Pad 声音与音画同步仍需复测，见 [NXB-107](reports/NXB-107.md)。
- 游戏实验仍依赖本机忽略目录中的引擎修订、构建补丁及资源；Android 实验客户端构建也尚未形成从公开材料完成的链路。后续工作见 [任务清单](../tasks/backlog.json)。
