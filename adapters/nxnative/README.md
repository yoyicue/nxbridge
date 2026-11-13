# nxnative 离屏适配

本目录保存 NXBridge 的 nxnative 适配源码、接口定义和可复现补丁。实验性 Mach 通道已将真实游戏的 IOSurface 和 PCM 传到独立测试 worker；它还不是正式媒体 ABI 的跨进程实现。进展见 [NXB-035](../../docs/reports/NXB-035.md)。

只在记录精确修订的独立上游副本中应用适配，不直接覆盖用户已有的 NX2Apple / nxnative 工作副本。游戏、存档、密钥和本机实验产物不放在此目录。实现任务、依赖和验收以 [任务清单](../../tasks/backlog.json) 中的 NXB-035 为准。

此前的 Sunshine / 游戏输入接入位于 [输入适配](../input/)，不因本目录的建立而改变。

补丁所涉及的 nxnative 上游代码保留 MIT 版权声明，见 `LICENSE.upstream`。固定实验修订见 [实验锁](../../config/nxb010-experiment-lock.json)；其中包含本机派生修订，本次源码预览尚不承诺从公开上游重建完整游戏。NXBridge 自有适配代码使用根目录 MIT 许可。

针对上游 `8d44aaf8f3a59eb0e0f63ced14b3f7f26e903484` 另有 [离屏候选补丁](integration-8d44aaf.patch)，准备脚本只对该精确修订应用。原三份补丁保留给原实验锁；候选已经过补丁重放及音频 / 输入 / 生命周期 C 语法检查，图形源在本机 macOS SDK 26.5 因缺少新版 MetalFX 属性而不能通过语法检查。上游要求 macOS 26.6 和 Xcode 27.2；完整构建、真实游戏及离屏音画仍未验收，不替换日常版本。见 [NXB-137](../../docs/reports/NXB-137.md)。

`embedded-xci-update.patch` 针对该锁定修订：当更新参数是同时含本体和更新的 XCI 时，只选取 Patch 内容元数据列出的 NCA；缺失、重复或不匹配时拒绝继续。普通 NSP 更新维持原行为。实验中将同一 XCI 以本体和更新两个文件名的符号链接提供给现有构建脚本；文件名中的更新版本仍需与包内数据独立核验。详见 NXB-115 报告，不代表所有 XCI 变体已经验证。

`region-check-fail-closed.patch` 针对实验构建脚本：地址检查没有观察到游戏进入 `nnMain` 时直接报错，避免被系统拦截的进程超时后误写入已检查标记。脚本未修改已运行进程；如旧流程已写入错误标记，重试前须先移除并重新检查。
