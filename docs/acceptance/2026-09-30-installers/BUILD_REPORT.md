# 2026-09-30 完成情况核对与 Windows 安装包构建

**结论：v0.1.0 x64 的 NSIS 与 MSI 已生成，仍按 release candidate 交付。**
本轮复跑的前端、Rust 和静态检查全部通过；安装版待验门槛继续保留。
时间采用香港时间。

## 尚未完成

| 范围 | 当前状态 | 剩余工作 |
| --- | --- | --- |
| GitHub 总工单 [#51](https://github.com/q956085398-netizen/Local-Console-Hub/issues/51) | OPEN，未指派 | 核对并收尾总工单；其修复子工单 #52–#56、集成验收 #57 均为 CLOSED |
| I-7 安装图标 | 待验 | 查看安装后的快捷方式、程序、任务栏与托盘图标外观 |
| I-8 MSI 实际安装 | 待验 | 在管理员授权下实际安装并验证；本轮只构建 MSI |
| I-15 覆盖安装后的实际运行 | 待验 | 启动应用，核对会话配置、既有日志和新增运行记录 |
| I-16 安装版终端 | 窗口与交互部分待验 | 在安装版窗口启动 PowerShell 并输入命令 |
| I-17 安装版完整 MVP 冒烟 | 相应窗口与交互部分待验 | 按 `docs/VERIFICATION.md` §4 完成安装版检查 |
| 代码签名与自动更新 | 当前版本未提供 | 后续版本范围，不在本次构建中增加 |

当前功能边界仍包括：配置使用外部 YAML，没有内置配置编辑器或配置自动重载；
服务会话输出面板只读，需要交互的程序使用 terminal 类型；只管理配置中的会话。
HTTP 健康检查、Busy/Degraded、依赖顺序、自动重启、Workspace、资源监控和跨平台等
继续按 [Roadmap](../../ROADMAP.md) 的后续阶段安排。

本轮以在线查询确认工单状态。正常 Windows 用户 `NEWNAME\q9560` 的 GitHub 实时
用户查询成功，仓库 issue 查询成功。沙箱身份 `NEWNAME\CodexSandboxOnline` 返回 401，
属于沙箱凭据访问或执行上下文问题，不据此判定用户登录过期。

## 构建输入与检查

构建使用 `main@bbd1c358add024a019e0db5c04bfbee1005e902e` 加开始前已有的工作区改动，
包含原生验收脚本、依赖及可选 `acceptance` 数据路径分支。
发布构建未启用 `acceptance`：Cargo release 指纹中 features 为 `[]`，CLI 使用
`cargo build --bins --features tauri/custom-protocol --release`。
138 个构建输入文件的构建前后 SHA-256 一致，见 [输入指纹](source-hashes.json)。

| 检查 | 实际执行环境 | 结果 |
| --- | --- | --- |
| `npm test` | Windows workspace-write 沙箱，Vitest 默认 worker | 14 文件、205 项通过 |
| `npm run check` | Windows workspace-write 沙箱 | 通过 |
| `npm run lint` | Windows workspace-write 沙箱 | 通过 |
| `npm run format:check` | Windows workspace-write 沙箱 | 通过 |
| `cargo fmt --all --check` | Windows workspace-write 沙箱，src-tauri | 通过 |
| `cargo clippy --all-targets -- -D warnings` | Windows workspace-write 沙箱，src-tauri | 通过 |
| `cargo test -- --test-threads=1` | 沙箱外登录用户 `NEWNAME\q9560`，src-tauri，串行 | 335 单元、10 集成通过；真实进程树与 ConPTY 检查通过 |
| `npm run tauri -- build` | Windows workspace-write 沙箱 | 前端生产构建和 Rust release 编译成功；MSI 打包失败，见下 |
| `npm run tauri -- bundle --verbose` | 沙箱外登录 Windows 用户，仅重试打包 | NSIS、MSI 均生成，退出码 0 |
| 程序版本资源 | Windows workspace-write 沙箱，只读 | ProductName 为 Local Console Hub；ProductVersion、FileVersion 均为 0.1.0 |
| `git diff --check` | Windows workspace-write 沙箱 | 通过 |

已有的 2026-09-30 原生自动化归档为 12 项通过，见
[原生验收记录](../../NATIVE_AUTOMATION_ACCEPTANCE.md)。本轮核对脚本、路径源码和
Cargo manifest 哈希均与归档报告一致，未重复运行该套窗口脚本。
其证据属于带 `acceptance` 功能的独立构建，不作为本轮安装版验收结果。

## 构建失败、重试与警告

首轮打包停在 WiX `light.exe`。详细复查显示 LGHT0217：ICE 校验无法访问
Windows Installer Service，并出现 LGHT0216 / 0x643。前端和 Rust release 均已成功，
这两次沙箱命令的总体退出码仍为 1，不能记录为完整打包通过。

通过正常的单次审批机制，在登录 Windows 用户环境仅运行 `tauri bundle --verbose`，
保留 ICE 校验后生成两个安装包，退出码 0。本轮没有执行安装、卸载或覆盖安装。

日志保留以下非阻断警告，不把它们隐藏为无警告构建：

- Vite 的主 JS chunk 超过 500 kB。
- MSVC linker stdout 提示。
- WiX ICE03、ICE40、ICE57、ICE61 警告；实际 MSI 安装与升级仍待验。
- 单独重试 bundle 时报告 `__TAURI_BUNDLE_TYPE` 标记未找到，提示可能影响 updater。
  当前版本不提供自动更新，本轮未验证 updater。

完整命令输出见 [首次构建](build.log)、[详细复查](build-verbose.log) 与
[Windows 用户打包](bundle-windows-user.log)。

## 交付产物

产物目录为仓库内 `src-tauri/target/release/bundle/`。两份安装包均未签名。

| 产物 | 大小 | SHA-256 |
| --- | --- | --- |
| NSIS `nsis/Local Console Hub_0.1.0_x64-setup.exe` | 3,445,266 bytes / 3.29 MiB | `8baf9938dcfdf36baf978c78e6ff96e7f90e49f53d0f03e1a281f8b86d09b630` |
| MSI `msi/Local Console Hub_0.1.0_x64_en-US.msi` | 4,558,848 bytes / 4.35 MiB | `d29f5c3c794a02d8a0d7068e85f468d539ddc35f3b4eb9e6b2b98de176c6aabc` |

NSIS 在香港时间 18:28:39 生成，MSI 在 18:28:30 生成。
文件信息与独立 release exe 哈希见 [产物指纹](artifacts.json)。
本轮结果属于构建验证，不将剩余安装版检查改为通过。
