# 多玩家与实例隔离

状态：设计草案。目标为家庭 2–3 位玩家；当前没有验证过任何并发容量。

## 为什么要实例管理

每名独立玩家需要一个游戏运行实例。多个客户端控制同一个游戏，只是同一会话共享控制；不能给三个人独立剧情和存档。

NXBridge 首版采用同一原生 macOS 会话内的多进程。Lima / 完整虚拟机不作为运行前提。这里的隔离用于防止串线、数据覆盖和故障传播，不是面对不可信租户的操作系统安全沙箱。

目标媒体进程为 NXBridge Rust Session，接收 nxnative 的离屏帧和 PCM；Sunshine 仅为基线与过渡后端。首个原生版本在已登录用户会话中不创建可见游戏窗口，其他登录 / 显示器状态分别验收，见 [ADR-0002](decisions/0002-rust-offscreen-server.md)。

## 身份与归属

- profile_id：持久玩家档案；玩家退出、Pad 更换后仍保留。
- title_id + build_id：游戏与运行时 / 资源版本。
- session_id：本次运行的唯一 ID；绑定 profile、host、游戏 PID / 启动时间、运行代次、媒体 worker、离屏帧 / PCM 源；窗口 ID 仅用于过渡采集。
- device_id：已配对客户端。首版每个会话只有一个控制者；旁观和共同操控不默认开启。
- lease_generation：控制权代次。重连接管增加代次，旧连接不能继续发送有效输入。

只知道 session_id 不构成授权。Host 校验玩家归属、配对设备及短期会话授权，再建立媒体 / 输入绑定。

原生 Host 管理持久配对身份，worker 持有当前会话授权和独立状态。Moonlight 的多会话身份 / 端点行为须先验证；若需要独立虚拟主机身份，需记录客户端兼容证据。连接数不能直接用作独立玩家数。

## 数据布局

以下为拟定的仓库外数据目录，示意中的 ROOT 不是要修改的系统环境变量。

```text
ROOT/
  host/                              # 持久配对身份与主机配置
  library/<title_id>/<build_id>/      # 已构建游戏与只读 shader 种子
  profiles/<profile_id>/<title_id>/
    save/                            # 玩家持久存档，APFS
    save_device/                     # 运行时实际使用时一并隔离
    preferences.json
    cache/<build_id>/                 # 该玩家可写 PSO / shader 缓存
  sessions/<session_id>/
    runtime/                         # DM_ROOT / 工作目录
    temp/                            # DM_TEMP
    input.sock
    media/                           # worker 配置、会话授权；过渡后端另存配对材料
    logs/
    manifest.json                    # 归属、PID 启动时间、端点、资源记录
```

- 显式设置 `DM_SAVE`、`DM_TEMP`、`DM_ROOT`、`DM_PSO_USER` 等已核实的运行时路径；审计 launcher 是否覆盖环境配置。
- 上游 `DM_SAVE` 事务使用 APFS clone，相关 `.work`、device-save 和临时目录均在玩家专属边界内。不同玩家不得共享同一个保存目录。
- 对 `(profile_id, title_id)` 使用排他写者锁；同一玩家重复启动应恢复既有会话或返回冲突。
- 已编译、不可变资源可以共用；各实例的可写缓存分开。磁盘只读共享不能当作 GPU 内存已经共享。
- 不复用上游默认的“同一 title ID 一个 Application Support 目录”作为多玩家存档布局。
- 同时运行期间默认不做跨设备存档同步。快照在退出 / 已确认提交边界获取；禁止直接复制正在变化的工作树作可靠备份。

多游戏构建、存档升级兼容和混合游戏容量的规则见 [Web 管理与多游戏支持设计](web-management-and-game-library.md)。同一玩家、同一游戏的不同构建仍受同一存档写者锁约束；各游戏及混合运行组合的兼容性与容量分别验收。

## 生命周期

```text
requested -> queued -> starting -> running -> draining -> stopped
                         |           |
                         |           +-> disconnected -> running
                         |                    |
                         +-> failed           +-> draining
```

- 调度器保留容量、取得存档锁，再准备目录并启动 worker / runtime；启动失败按逆序释放自己取得的资源。
- `running` 需要游戏和媒体健康检查；仅 PID 存在不够。
- 断线立即中和输入。重连宽限初始建议 60 秒，可配置；这段时间游戏可能继续运行，不能声称自动暂停。
- 宽限到期进入 draining，尽力完成正常退出和保存；没有可靠的保存确认时明确报告，不伪造“已保存”。
- Host 重启先核实 manifest 中的 PID 与启动时间，再恢复或标记遗留；禁止按复用 PID 误杀其他进程。
- 一个 worker / 游戏崩溃不结束其他玩家会话。资源回收只依据该 session 的登记内容。

## macOS 特有阻碍

上游 nxnative 在窗口遮挡时记录 display link 可能约每秒一次；最小化会暂停图形和音频。因此多个 `.app` 窗口同时打开，不等于三个实例能持续 30 FPS。

原生路线的验证顺序：

1. 在已登录用户会话中验证单游戏直接输出帧 / PCM，不创建可见游戏窗口；回归游戏速度、GPU 同步和音频消费时钟。
2. 两个原生实例使用不同测试存档，验证各自源、输入、保存和退出；各路分别记录帧时间。
3. 每个实例产生不同画面和音频标记，验证每个 Pad 只接收自己的内容。
4. 测试主机身份、配对、完整媒体 / 控制端点、退出与发现；原版 Moonlight 不兼容时，明确客户端扩展范围。
5. 分别测试无显示器冷启动 / 热拔插、锁屏和未登录；记录 supported / unsupported / unknown 及证据，不从无窗口推导其他能力。

窗口模式保留为诊断和过渡：不重叠、失焦、遮挡和最小化逐项记录。隐藏窗口或虚拟显示器采集结果不能作为原生离屏输出验收。

## 容量与调度

- 初始配置 `initial_session_limit=1`；`measured_session_capacity=null`。这是测试起点，尚无“一路已达标”结论。
- 再按 1 → 2 → 3 个实例逐步增加，所有会话使用真实游戏与固定场景。每一路分别达标才能提升容量记录。
- 记录 GPU 时间、各进程 physical footprint、系统内存压力、swap、编码耗时、网络和温度。统一内存上的 CPU / GPU 统计可能重叠，不直接相加当作实际总占用。
- 启动新实例前检查已测预算和当前状态；出现持续压力时暂停接纳新玩家并排队，不静默杀掉其他玩家或降低其承诺质量。
- 排队有序且可取消，释放槽位后重新检查存档锁；启动编译 shader 时避免同时抢占资源。
- 单台容量不足，可把下一个实例交给另一台 Mac；同一存档仍只有一个写者。首版不实现运行中跨机迁移。

首轮承载主机是 M4 / 16 GB；不能用 M4 Pro 的上游成绩推导本机可以三开。
