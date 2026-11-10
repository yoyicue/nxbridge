# S2Arm 实验后端

NXBridge 对 S2Arm 的独立接入，用于逐款验证 NS2 游戏。固定修订见 [实验锁](../../config/s2arm-experiment-lock.json)。**用户提供的 OoT NUP 已完成 900 秒原生离屏游玩短测；1402 通过 Rust GameStream 读回森林存档，并用触屏移动、转动视角。无效 GPU 反馈造成的误停已修复，保护阈值保留。稳定 30 FPS、游戏速度、音画同步与长期兼容仍待验收。** 原有 nxnative、BOTW / TOTK 服务继续使用各自的后端。

## 已实现的接线

- `input.c` 实现 S2Arm 的 `s2a_vpad_read`，复用 NXBridge 会话令牌校验、组合按键 / 四轴快照、短按队列及断开中和；不依赖窗口焦点。
- `AudioDriver.cs` 实现固定上游的音频设备接口，以 5 ms 时钟混合为 48 kHz 双声道 PCM16，送入既有 PCM / Mach 管道。客体缓冲消费后才完成；暂停保留排队数据，欠载补静音。支持上游音频渲染器循环复用四个描述对象：每次复制其 PCM，并分别记录完成位置；仍借用客体内存的缓冲禁止提前复用。运行中的队列满时阻塞生产者，消费后唤醒；关闭也会唤醒等待者，不能靠增加容量或丢弃 PCM 掩盖积压。
- `present.inc` 在无 `CAMetalLayer` 的呈现分支使用 Metal 3 / 4 渲染队列，将纹理画入 IOSurface。保留生产者事件、GPU 驻留及完成回调，复用现有有界帧槽、Mach 传递和硬件编码；不经过桌面捕获或 CPU 像素读回。
- `launch.py` 复用游戏目录配置的 `host` / `game_dir` 和 `DM_SAVE` / `DM_TEMP`，为每个会话启动独立宿主。`DM_SAVE/ryu` 持久保存账号及进度，排他锁跨 `exec` 保留；诊断与缓存放在独立会话目录。显式禁用 `S2A_SAVE_DIR` 的存档种子导入和自动玩家。
- `gpu_timing.h` 校验 Metal 反馈是否位于 CPU 提交前采样至完成回调的时间区间内。不合法时，以完整宿主时间区间作为 GPU 安全预算的保守上限，同时把该帧 GPU 性能读数标为未知；保留 1,000 ms 停机阈值。它处理错误计时造成的误停，不表示已经修复底层时间戳来源。
- `integration.patch` 修改音频工厂、无窗口呈现、GPU 计时校验、关停及链接框架；不添加某款游戏专用地址或兼容成功桩。补丁保留上游窗口路径，但此次只提供 headless 构建入口。

## 准备与构建

以下变量均由使用者填入本机路径，产物应放在忽略目录。`S2A_SOURCE` 必须是实验锁所列提交；准备脚本只克隆已提交内容，拒绝覆盖已有输出，也不导入源工作区的未提交改动。

```sh
python3 adapters/s2arm/prepare.py --source "$S2A_SOURCE" --output "$S2A_ENGINE"
```

需要 Apple Silicon、匹配的 Xcode / Metal 工具、Bash 5、上游 `global.json` 指定的 .NET SDK，以及上游构建依赖。此次在 M4 / macOS 26.5 / Xcode 26.5 上使用 .NET SDK 10.0.403 完成构建；Bash 5、Meson、Ninja 与 Python 依赖须先准备，脚本不代装。游戏着色器转换还要求 `spirv-cross` 与上游 `translator/dmsh_chain.lock` 一致，此固定修订为 `1.4.363.0`；缺失时在直连库工具校验处失败，补齐后可以沿用原输出目录续跑。

```sh
env DOTNET="$S2A_DOTNET" DOTNET_ROOT="$S2A_DOTNET_ROOT" \
    S2A_PY="$S2A_PYTHON" S2A_CS_CACHE="$S2A_CS_CACHE_DIR" \
    S2A_RUN_ROOT="$S2A_BUILD_RUN_DIR" \
    make -C "$S2A_ENGINE" host S2A_CS=1
```

`DOTNET_ROOT` 是指定 SDK 的根目录。PATH 中的 `bash` 应为 Bash 5；不要开启 `S2A_MAC_FRONTEND=1`。产物是 `build/s2arm-host`，仍需用户自己的游戏构建与资源。

游戏准备沿用固定版本的 [上游 macOS 构建入口](https://github.com/jhaizhou-ops/S2Arm/blob/93bf8c6cab24934b3fe05af4bc04cba1fec84328/pack/oneclick/oneclick.sh)，选择 `--target mac`。具体源包格式、标题 / 版本、系统数据及转换结果必须按实际文件核实；本目录不提供或下载游戏、密钥、存档。

游戏转换还需要 C 改写器的源码依赖，不能用已安装的 Python wheel 代替。首次构建时，在独立 Python 环境安装上游 `launchers/macos/requirements-runtime.txt` 与 `pack/fonts/requirements.txt` 的固定依赖，并显式准备 C 依赖：

```sh
"$S2A_PYTHON" "$S2A_ENGINE/runtime/rewriter/prepare_deps.py" "$S2A_ENGINE/build/rewriter-deps"
```

此下载脚本校验固定 SHA-256，拒绝覆盖已经展开的依赖；已有完整依赖时无需再运行。此次外置实验盘中的 Python 原生库加载出现停滞，换用本机盘的独立工具环境后通过。游戏资源首次扫描也受大量小文件读取影响，现已校验并搬到本机 SSD 继续构建；原始 NUP 保留。上游使用跨阶段资源清单缓存。

## 接到实验游戏目录

参照 [多游戏配置](../../docs/experimental-multiple-games.md)，把新入口的 `host` 设为 `$S2A_ENGINE/nxbridge/launch.py`，`game_dir` 设为准备好的 Mac 模块 / 资源根。该根需要 `main.patched.bin`、`main.npdm`、`romfs` 及对应的其他模块 / 着色器。启动前调用上游 `mac_chain_check.py`，拒绝已识别的着色器链不匹配。

为这个玩家和游戏建立新的 `DM_SAVE`、`DM_TEMP` 及 sandbox 配置；不要引用原 BOTW / TOTK 的存档。宿主要求有效 `NXB_SESSION_ID`、`NXB_VIDEO_GENERATION`，这些由现有 GameStream worker 提供。Mach 音画和输入的会话环境继续使用现有 worker 契约。只读 shader 链检查在启动器进程内执行：本机回归证明，Python 启动预检子进程会丢失继承的 Mach 媒体端点，不能换回该方式。

在上述环境齐备后，`launch.py GAME_ROOT --dry-run` 只校验、输出路径，不启动游戏、不创建会话，不输出输入令牌。真实启动会加存档锁并 `exec` 宿主；它不会自动注册游戏或修改正在运行的服务。已在引擎 / 游戏目录禁止写入的 sandbox 下通过 Mach worker 测试，并接通独立 Rust GameStream / Pad 入口；该策略并非完整文件访问沙箱。实测串流配置为 1280×720、H.264、30 FPS、5 Mbps，配置值不代表客户端实际显示帧率。

`DM_SAVE/ryu` 保存的是此后端的完整持久状态，不是 nxnative 的原始存档格式；不要直接迁入旧存档。保存目录与工作、游戏、引擎目录不能相互覆盖，`ryu` 不能是符号链接。现有存档迁移须另行备份、验证。

## 无游戏回归

```sh
python3 tests/s2arm_prepare_test.py
python3 tests/s2arm_launch_test.py
python3 tests/s2arm_input_test.py
python3 tests/s2arm_gpu_timing_test.py --engine "$S2A_ENGINE"
"$S2A_DOTNET" run --project tests/s2arm_audio/Audio.csproj \
    -p:S2ArmRoot="$S2A_ENGINE"
python3 scripts/check-s2arm-video.py --engine "$S2A_ENGINE" --output "$S2A_NEW_TEST_DIR"
make check-native-media
make check
```

视频检查需要已编译引擎、Metal 4 设备、FFmpeg / ffprobe，输出目录必须尚不存在；分别测试默认 Metal 4 和强制 Metal 3，以及两者的帧槽耗尽场景。它验证生产者事件完成前不释放源纹理、丢弃后续帧不会提前推进完成栅栏、硬件 H.264 生成及解码像素；不是 FPS 基准。音频测试直接引用固定版本的 Ryujinx 音频接口，首次构建需提前具备上游依赖。

## 边界与来源

当前复用视频探针的 1280×720 输出与 6 个 IOSurface；输入沿用现有 8 边沿队列。音频上限为 8 个输出设备会话、每会话 64 个缓冲 / 2 秒 PCM，不支持直接音频输入，其他采样格式依赖上游兼容转换。无效格式、单包超限和未启动队列满仍报错；运行中队列满采用背压。该边界已在 OoT 冷编译阶段触发，背压不能当作低音频延迟的证据。自动存档已在新会话读回，重复存读、异常退出、音画同步及多人并发仍须验收。

OoT 使用上游 `runtime_only_lib.sh` 生成独立实验根，按需执行真实 shader 翻译。完整离线编译因磁盘余量中断；不能把该根当作离线预编译交付物。修复后的 900 秒测试现编 1,203 个 shader、0 翻译失败；仍有纹理类型告警、首遇停顿和音频间断。翻译成功不代表 GPU 语义全部正确。不要用空库掩盖已知 shader 语义错误，也不要修改正常构建根的阶段记录来冒充出库成功。

Pad 读档短测使用私有启动包装，将同一引擎 / 游戏的上局翻译缓存校验后复制到新会话；上游 chain / translator 指纹校验保留，实际复用 859 个、现编 42 个 shader。它只验证同一构建的缓存可用，**公开启动器尚未提供持久缓存功能**。这轮预设 120 秒原生呈现观察平均 56.59 FPS，但有 9 次超过 100 ms、最长 1.38 秒；不同场景 / 背景负载下的单次样本不能视为缓存性能 A/B 或稳定 30 FPS 结果。游戏峰值约 9.144 GiB，系统同时存在其他应用负载；共享内存不能直接相加，容量仍未知。客户端退出后还记录到尾部视频失败及音频发送失败计数，关停边界需继续核对，见报告。

初始化清单记录适配源码 SHA-256 并在启动时核对，便于发现补丁漂移；它不是完整工具链、二进制或游戏产物的可重现构建证明。上游 Host API 仍有草案接口，本适配使用当前源码已存在的实际钩子。

自有适配采用 MIT。上游根许可证和归属保留于 [LICENSE.upstream](LICENSE.upstream)、[NOTICE.upstream](NOTICE.upstream)，NVN 派生部分的 MIT 文本保留于 [LICENSE.nvn.upstream](LICENSE.nvn.upstream)。`present.inc` 沿用上游呈现机制；补丁中的上游内容仍保留其原许可。完整引擎与第三方库未随本目录分发。验证结果、已知失败和恢复状态见 [NXB-127 报告](../../docs/reports/NXB-127.md)。
