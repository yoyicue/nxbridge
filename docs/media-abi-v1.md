# NXBridge 媒体接口 v1

状态：描述符布局和租约语义已确定，已实现本机校验与有界租约管理；操作系统句柄传输、GPU 同步、编码和游戏离屏源尚未实现或验收。合成源与 nxnative 源使用同一份 [C 头文件](../crates/nxbridge-media/include/nxbridge_media.h) 和 [Rust 类型](../crates/nxbridge-media/src/lib.rs)。

## ABI 与会话

- ABI 为进程内 C 布局：`uint32_t` / `uint64_t`、自然对齐、`repr(C)`；当前仅约定 64 位 macOS。每个顶层描述符先携带 `struct_size` 和 `abi_version = 1`，v1 要求准确大小，未知版本、非零保留字段拒绝。它不是跨进程字节协议，不能直接把整个结构体当网络包发送；跨进程控制消息的编码另行定义。
- 每次运行由 Host 创建非零 128 位 `session_id` 与非零 64 位 `generation`。视频、音频、注册、释放必须匹配；重启、重绑源、无法保证安全复用时使用新 generation。旧代次事件一律拒绝，不让旧帧、PCM 或释放通知影响新运行。
- 视频、音频分别从非零序号开始，按来源递增；可因丢帧 / 明示音频间断而跳号。缓冲 ID 非零、只在对应 epoch 和媒体类型内有效，**不是**指针、文件描述符或 Mach port 名称。实际共享对象在注册时通过平台适配层单独传递并绑定 ID，重复注册、大小 / 格式不符或句柄无效必须拒绝。
- `pts_ns`、`duration_ns` 是同一台 Mac 的 `clock_gettime(CLOCK_MONOTONIC)` 时钟域中的纳秒；时间戳表示帧呈现目标或 PCM 首样本时间，非墙上时间、网络接收时间或 Android 时钟。每条流的 PTS 在同一 epoch 严格递增；跨设备时钟不可直接相减。

## 视频缓冲与完成信号

- v1 支持 8 位 SDR 的 BGRA8 / 单平面全范围 RGB，或 NV12 / 双平面 BT.709（全或视频范围）；传输特性与色彩原色均为 BT.709。尺寸范围为 1–8192，NV12 宽高必须为偶数。缓冲注册记录分配大小、每平面的偏移 / 长度 / 行跨度；越界、溢出、平面重叠和帧描述与注册不一致均拒绝。HDR、裁剪和动态分辨率变更需另行协商，不能悄悄沿用 v1 假设。
- 两种承载类型为 `CPU_SHARED` 和 `IOSURFACE`。合成源可以先使用本进程 CPU 缓冲，之后由 Mac 平台适配为编码器可使用的缓冲；游戏源可注册 IOSurface。跨进程 IOSurface 的 Mach 发送权必须经可信的 Mach / XPC 通道转移，接收端重新导入并持有引用；不能把进程本地 `mach_port_t` 的整数或裸指针塞进 ID。参见 Apple 的 [IOSurfaceLookupFromMachPort](https://developer.apple.com/documentation/iosurface/iosurfacelookupfrommachport%28_%3A%29?language=objc)。
- v1 的 GPU 完成信号采用**完成后发布**：源在 Metal command buffer 完成回调确认成功、所有写入完成后，才发布 `READY_GPU_COMPLETE` 帧；CPU 写入完成后发布 `READY_CPU_COMPLETE`。消费者只接受已完成的帧。v1 不传递尚未触发的 GPU fence，也不声称 GPU 等待已实现。将来可设计独立版本以 XPC 传递 `MTLSharedEventHandle` 和目标值；Apple 对跨进程共享事件的说明见 [MTLSharedEvent](https://developer.apple.com/documentation/metal/mtlsharedevent?changes=_6_1)。
- 注册源持有底层对象；成功发布后将对应 `(epoch, buffer_id, sequence)` 租约交给消费者，发布失败时仍由源持有。只有收到匹配的 `nxb_video_release_v1` 才允许源再次写入；重复 / 过期释放拒绝。队列丢弃未送编码器的帧可立即返回 `DROPPED`。已经提交给编码器的缓冲必须等异步编码完成及平台适配器确认无后续使用后才释放；提交失败、同步丢帧、回调失败各返回相应结果，销毁时先排空或隔离资源。VideoToolbox 可异步编码并保留图像缓冲，见 [Apple 编码接口](https://developer.apple.com/documentation/videotoolbox/vtcompressionsessionencodeframe%28_%3Aimagebuffer%3Apresentationtimestamp%3Aduration%3Aframeproperties%3Ainfoflagsout%3Aoutputhandler%3A%29?changes=__7)。
- 默认每会话视频池上限 6：最多 1 个源正在写、3 个等待编码、2 个编码器持有。队列满时仅丢最旧的未提交帧并释放，禁止覆盖在途帧；不得无限排队。实际性能可调整参数但不可破坏上限 / 释放语义。

## PCM 音频

- v1 格式固定为 48,000 Hz、立体声、交错 IEEE-754 `f32` 小端；每个块 1–480 个采样帧（最多 10 ms），字节数为 `frames × 2 × 4`。注册的 CPU 共享缓冲须足够大；非有限值和幅度处理在实际编码适配处另行验证。
- `first_sample` 为本 epoch 音频流内首个采样帧的绝对序号。连续块应当首尾相接、序号连续；以首块 `(sample₀, pts₀)` 为锚，后续 `pts_ns = pts₀ + floor((first_sample - sample₀) × 1_000_000_000 / 48_000)`，整数运算避免累积漂移。明确丢失时设置 `NXB_AUDIO_DISCONTINUITY`，重置锚点并记录丢失；未经标记的序号跳变、跳样、时间回退和重放拒绝。
- 音频最多 20 个等待块（最多 200 ms）、1 个消费者持有块、1 个源写入块，即池上限 22。队列满时拒绝新块且**不转移所有权**；源根据实际采样时钟标记后续不连续，不静默无限积压。消费者复制 / 编码不再使用该块后，返回匹配的 `nxb_audio_release_v1`；终止时未消费块可丢弃释放，正在消费的块仍须等待完成。

## 终止与错误

关闭时拒绝新发布，先以 `DROPPED` 释放未提交缓冲；在途缓冲仅在编码 / 消费完成后释放。源或消费者异常退出时，不因计时器到期而推定 GPU 或编码器已释放对象；隔离该代次，完成平台资源清理后再销毁，绝不把旧缓冲塞给新会话。Rust `VideoLeases` / `AudioLeases` 覆盖队列上限、状态和释放匹配，**不替代**未来真实 Mach/XPC、GPU、VideoToolbox 与进程崩溃测试。

布局由 `tests/media_abi_layout.c` 和 Rust `contract.rs` 分别检查；其测试只是 ABI 与模型，不是离屏音画或 GameStream 已实现的证据。
