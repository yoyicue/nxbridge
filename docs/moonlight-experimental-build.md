# 实验客户端源码重放与构建

NXBridge 当前实验客户端基于 Moonlight Android GPL-3.0 上游及本项目补丁；来源和许可见 [输入适配来源](../adapters/input/NOTICE.md)。本方法先准备 390 源码，再应用 391 肩键联动和 393 触点归属补丁；再依次应用 395、398 版本号补丁、401 按钮外观、403 封面完整显示和 404 首包等待补丁。这不是已验证的干净 Mac 构建、正式发布包或可复现二进制；不要将个人游戏、存档、配对或签名密钥放入仓库。

## 准备源码

需要 Git、Python 3 和网络。将上游克隆到自己的目录，检出固定提交；然后让脚本创建一个**不存在**的独立工作区。脚本拒绝覆盖已有输出，保留上游工作副本的未提交文件。所有路径均由使用者自行选择。

```sh
git clone https://github.com/moonlight-stream/moonlight-android.git moonlight-upstream
git -C moonlight-upstream checkout --detach b48494cb96bff23d8886c4775cc4f39a1075495d
python3 adapters/input/prepare_moonlight_390.py --source moonlight-upstream --output moonlight-nxbridge-390
python3 adapters/input/apply_shoulder_group_391.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_touch_owners_393.py --moonlight moonlight-nxbridge-390
# 跳过诊断构建，恢复为正常版本后添加按钮外观功能
python3 adapters/input/apply_clean_version_395.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_clean_version_398.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_button_visual_401.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_cover_fit_403.py --moonlight moonlight-nxbridge-390
git -C moonlight-nxbridge-390 submodule update --init --recursive
python3 adapters/input/apply_first_video_timeout_404.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_first_video_timeout_405.py --moonlight moonlight-nxbridge-390
python3 adapters/input/apply_stick_edge_406.py --moonlight moonlight-nxbridge-390
```

第一个脚本应用 37 个顺序补丁，包括 318 基线和 320–390 迭代；第二个脚本在 390 基础上应用 391，第三个脚本在其基础上应用 393；第四和第五个脚本只将版本号分别提升到 395 和 398，第六个脚本应用 401，第七个脚本应用 403；初始化固定子模块后依次应用 404 和 405，分别修改客户端版本与 moonlight-common-c 的首视频包等待期限。405 将期限从 45 秒延长至 180 秒，已接收数据后完整帧的 10 秒期限不变；406 允许浮动摇杆图像超出触摸区显示，输入区域不变，详见 [边界裁切验证](reports/NXB-120-stick-edge.md)。仍需要实测长预热、取消和故障边界。脚本不下载 Android 工具链、不安装 APK，也不自动初始化子模块。中途失败时部分工作区会留在指定位置以便检查；确认无需要保留的文件后，可用 `git -C moonlight-upstream worktree remove --force moonlight-nxbridge-390` 删除**该次创建的**工作区。上游修订和补丁前提不一致时停止，而不是覆盖已有文件。

## 本机编译

本次使用 Gradle Wrapper 9.7.1、Android Gradle Plugin 9.1.1、Android SDK API 37、NDK 28.2.13676358，构建 `arm64-v8a` 的 `nonRootDebug`。需先按自己的 Android 开发环境安装 SDK、NDK、匹配的 JDK、Gradle 依赖并接受相应工具许可。依赖已在本机缓存时可运行：

```sh
cd moonlight-nxbridge-390
./gradlew --offline :app:assembleNonRootDebug
```

无缓存时需要联网解析 Gradle 和 Android 依赖；本次未验证全新机器的依赖下载及工具安装。产物位于 `app/build/outputs/apk/nonRoot/debug/`。Debug APK 使用构建环境的签名身份；签名不同的 APK 无法作为现有客户端的直接升级，不能用卸载应用或删除数据绕过签名冲突。

## 本次验证和限制

本机从固定提交创建独立工作区，按脚本完成 37 个补丁，402 个非子模块客户端源码文件与现用 390 构建树逐字节一致。递归子模块固定为 `moonlight-common-c` `874ac9548f1bd6f095ef2b435c42cdde460e7821`、`enet` `aca87840b57f045a1f7f9299e4b1b9b8e2a5e2f1` 和 `nanors` `b1e3c22ca0cdc0bb83e3cd6ed1a2fc77869ed99a`；在已有本机工具及依赖缓存下离线构建成功。

391 补丁在独立 390 工作区重放后，6 个修改文件与 391 构建树一致；391 已在本机缓存下离线编译并安装到两台 Pad。393 补丁在独立 391 工作区重放后，3 个修改文件与 393 构建树一致；393 在本机缓存下离线编译并安装到两台 Pad。401 补丁在独立 398 工作区重放后，528 个应用源文件与构建树一致，401 在本机缓存下离线编译并安装到两台 Pad。403 补丁在 401 工作区重放，3 个修改文件与本机构建树一致，并离线编译、安装到两台 Pad；详见 [封面记录](reports/NXB-123.md)。404 在独立固定修订工作区完成完整补丁链重放，客户端版本和子模块 `VideoStream.c` 与构建树一致；联网补齐依赖后构建并安装到两台 Pad。1372 的实际游戏首视频包到达时间为 15.3 秒，超过旧的 10 秒限制；45 秒超时边界和其他网络故障另待验证。完整链路的干净机器构建尚未验证。

上述 390 重建 APK 和当时安装构建的哈希不同：112 个 ZIP 条目中 4 项不同，分别为原生 `libmoonlight-core.so` 和 3 个签名元数据文件。两个原生库分别含有各自构建目录字符串，尚未证明其余原生差异来源，因此不能声称字节可复现或与已安装 APK 完全一致；新构建 APK 也未安装到 Pad。干净系统、依赖锁、统一签名与设备运行仍需单独验证。修改前的源码对照、构建记录与 APK 均留在本机忽略目录，不随仓库分发。
