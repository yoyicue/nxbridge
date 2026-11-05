# nxnative 离屏适配

本目录保存 NXBridge 的 nxnative 适配源码、接口定义和可复现补丁。实验性 Mach 通道已将真实游戏的 IOSurface 和 PCM 传到独立测试 worker；它还不是正式媒体 ABI 的跨进程实现。进展见 [NXB-035](../../docs/reports/NXB-035.md)。

只在记录精确修订的独立上游副本中应用适配，不直接覆盖用户已有的 NX2Apple / nxnative 工作副本。游戏、存档、密钥和本机实验产物不放在此目录。实现任务、依赖和验收以 [任务清单](../../tasks/backlog.json) 中的 NXB-035 为准。

此前的 Sunshine / 游戏输入接入位于 [输入适配](../input/)，不因本目录的建立而改变。

补丁所涉及的 nxnative 上游代码保留 MIT 版权声明，见 `LICENSE.upstream`。固定实验修订见 [实验锁](../../config/nxb010-experiment-lock.json)；其中包含本机派生修订，本次源码预览尚不承诺从公开上游重建完整游戏。NXBridge 自有适配代码使用根目录 MIT 许可。
