# v0.2.1 本地回归验收

修复服务停止后再次启动／重启的 `ERROR_INVALID_HANDLE`，临时 PowerShell 使用最小
空闲编号，并把头部三点菜单替换为直接的“关闭／移除”按钮。“保存启动配置”移到详情页。

临时终端“关闭”先读取后端状态，运行时完成停止后再移除，已停止或已退出时直接移除。
停止或移除失败时保留会话并报告错误。已保存程序的“移除”沿用确认弹窗及运行中拒绝规则。

## 环境与检查

Windows 原生进程、ConPTY、WMI 和 WebView2 验收均在已登录 Windows 用户环境、沙箱外
通过逐命令授权执行。源码编辑、静态检查、应用编译和 NSIS 打包在 workspace-write
沙箱内执行；MSI 的 WiX 打包阶段在沙箱外通过单独命令授权完成。

- Node 24.19.0：前端 32 个测试文件、356 项测试通过；构建、ESLint、Prettier 通过。
- Rust：串行套件 604 项单元测试通过；1 项 GUI 形态服务重启回归通过。
  MVP 集成有一次 17 项全通过；随后串行运行 16 项通过，临时终端自然退出用例超时，
  单独重跑该用例通过（1.60 秒）。没有把后一次串行集成运行记为全绿。
  两个忽略项是由父测试明确启动的子进程夹具。
- Windows 实际 PID 复用用例单独通过（66.68 秒）；串行套件仅排除此用例，不修改它的断言。
  此前并发运行出现过优雅停止、PID 复用等待以及输出批次数量的时序失败，保留这些事实，
  不把并发运行记为全绿。
- `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、`git diff --check` 通过。
- [真实应用验收报告](native-report.json) 记录验收二进制及脚本哈希、每个场景、清理结果和
  用户配置未改变的核对。30 项全部通过，各场景清理通过，用户配置哈希未改变。
  验收二进制启用已有 `acceptance` 特性，读取私有配置。

复查命令（原生测试使用上述 Windows 执行环境）：

```powershell
npm test
npm run build
npm run lint
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1 --skip ipc::listen::tests::a_real_pid_reuse_is_not_the_old_session
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::listen::tests::a_real_pid_reuse_is_not_the_old_session -- --exact --nocapture
node scripts/native-acceptance.mjs .scratch/v0.2.1-acceptance/local-console-hub.exe
```

## 测试窗口残留

单独复现了独立窗口测试过早结束 shell 后保留终端窗口的现象；另一个测试先杀 `cmd.exe`，
再按已消失的根 PID 清理，确实留下了 `ping.exe` 子进程。修正测试启动握手、清理顺序，
并让无需窗口的辅助进程使用 `CREATE_NO_WINDOW`。未改 Windows Terminal 全局设置，
也未按进程名称批量关闭用户窗口。

[清理专项记录](test-cleanup-probe.json)：三个测试在每次退出后的观察点均没有新增窗口，
也没有残留匹配的 `cmd/ping` 测试进程。此记录证明的是这三个测试的本次结果，
不保证任何测试、任何 Windows Terminal 配置下都不会保留错误页。
完整验收结束后的 WMI 检查（2026-10-06 03:22:54 +08:00）亦未发现匹配的测试等待命令、
`ping.exe`、Hub 或 Windows Terminal 进程。此检查没有关闭任何进程。

## 验收边界

服务回归使用私有 `.bat` 夹具（含空格的路径），通过实际 `.lnk` GUI 入口连续停止／启动
三次并重启；不是对用户 SillyTavern 数据和服务的直接运行验收。
截图来自真实 Tauri/WebView2，沿用现有按钮、图标尺寸和颜色；不包含安装器外观验收。

生产安装包使用默认特性构建，副本在 `releases/v0.2.1/`，
[生产文件哈希](production-SHA256SUMS.txt) 与该目录的 `SHA256SUMS.txt` 一致。
NSIS 和 MSI 分别从未打包的生产二进制生成，以写入各自正确的 bundle 标记；
便携 `.exe` 使用未打包副本。生产文件的 FileVersion 和 ProductVersion 均为 `0.2.1`。
本次未安装、升级或卸载用户已有版本。本文记录本地验收阶段，发布内容见
[v0.2.1 发布说明](../../RELEASE_NOTES_v0.2.1.md)。

[终端操作截图](terminal-actions.png) · [服务重启截图](service-restarted.png) ·
[编号复用截图](terminal-number-reused.png) · [保存配置截图](saved-terminal.png)
