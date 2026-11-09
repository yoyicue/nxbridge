# 实验性多游戏入口

`nxbridge-gamestream` 可通过本地 `NXB_GAME_CATALOG` 指定 JSON 文件，为每个游戏发布一个 Moonlight 入口，并按会话的应用 ID 选择对应的启动参数。未设置此变量时保留原有单游戏环境变量行为。封面可以映射到 Host 游戏库；启动配置、玩家管理和 NXB-060 生命周期仍未统一，不能作为生产方案。

配置格式为数组；以下路径仅示意，资源、存档和配置文件均保存在仓库外。每个目录必须已经存在，封面可省略或设为 `null`。`environment` 只接受 `DM_` 变量，须按各游戏实际构建填写所需的 ROMFS、着色器、语言等配置；服务不会自动复用其他游戏的 `DM_` 环境。示意值不能直接运行。

```json
[
  {
    "title": "BOTW",
    "library_game_id": "example-botw",
    "boxart": null,
    "worker": "/absolute/botw/worker",
    "host": "/absolute/botw/dmhost",
    "game_dir": "/absolute/botw/patched",
    "sandbox": "/absolute/botw/sandbox.sb",
    "environment": {
      "DM_ROOT": "/absolute/botw/build",
      "DM_ROMFS": "/absolute/botw/build/extract",
      "DM_MSL_DIR": "/absolute/botw/build/shader",
      "DM_SAVE": "/absolute/botw/save",
      "DM_TEMP": "/absolute/botw/temp"
    }
  }
]
```

需要共享封面时，将 `NXB_LIBRARY_COVER_DIR` 设为 Host 游戏库 TOML 所在目录下的 `covers` 绝对路径，并将 `library_game_id` 填成游戏库中对应的游戏 ID。单游戏环境变量入口使用同一目录与 `NXB_LIBRARY_GAME_ID`。主机从该目录的 `<游戏ID>.png` 读取；管理台修改封面后服务无需重启。启动映射仍需手工配置，游戏库不会自动创建串流入口。未配置映射时继续使用原有 `boxart` / `NXB_APP_BOXART`。管理台支持小于 5 MiB、最长边不超过 4096 的 PNG，不编辑图像；图片不随源码发布。

定制 Moonlight 327 识别应用列表中的可选封面 SHA-256，发现变化后下载并校验，成功后原子替换缓存；下载失败时保留旧缓存，重新进入列表可重试。轮询周期约 30 秒。缺少封面时服务不提供版本，客户端可能继续显示已有缓存。未修改的 Moonlight 忽略扩展字段，能按原协议取图，但不能保证自动刷新旧缓存。

每个游戏的 `DM_SAVE` 和 `DM_TEMP` 必须是独立、不互相包含的绝对目录；配置加载时会解析符号链接并拒绝重叠路径。同一服务只运行一个会话。选择后，服务持有存档目录旁的 `.<存档目录名>.nxbridge-writer.lock` 文件锁，避免同一存档被采用此机制的其他服务同时使用；锁放在目录外，避免引擎提交时替换存档目录导致锁失效。旧版服务、引擎直接启动以及其他软件不会获取此锁。结束进程、异常中止和存档完整性仍需 NXB-060 的生命周期实现，不能仅靠该锁推断安全恢复。启用前先备份已有存档，确保没有其他写者。

当前上游 Moonshine 依据标题生成应用 ID，改变标题会使客户端看到新的入口；稳定游戏 ID 尚未实现。游戏的可运行性、存档、输入、音画、重连及并发必须逐游戏验证。不得仅因入口显示而宣称可玩。
