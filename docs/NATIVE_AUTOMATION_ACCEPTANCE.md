# Windows 原生自动验收

以脚本连接实际 Tauri 应用内的 WebView2，操作真实界面并调用真实 IPC。不启动浏览器预览，
不替换 IPC 返回值，不使用桌面鼠标控制工具。方案依据 [Playwright WebView2 文档](https://playwright.dev/docs/webview2)。

## 重复运行

首次准备依赖后，在仓库根目录构建独立验收二进制：

```powershell
npm ci
$env:CARGO_TARGET_DIR = Join-Path (Get-Location) 'src-tauri\target\issue-57'
npm run tauri -- build --debug --no-bundle --features acceptance
```

在正常登录的 Windows 用户环境运行（Codex 中需要对这个具体测试命令使用沙箱外执行）：

```powershell
npm run test:native
```

也可显式传入验收二进制：

```powershell
npm run test:native -- C:\path\to\acceptance\local-console-hub.exe
```

默认构建不启用 `acceptance`；只有显式验收构建读取 `LCH_ACCEPTANCE_ROOT`。
这个构建要求绝对路径，缺失或相对路径不会回退到用户目录。脚本为每次运行创建私有
配置、日志、元数据与 WebView2 profile，核对配置报告的实际路径后才启动测试会话。
CDP 只使用环回地址和临时端口；结束时停止自己启动的会话、终止自己的应用进程树，
并核对应用与会话 PID 已退出。证据保留在 `.scratch/native-acceptance/run-*/`，不自动删除。

## 2026-09-30 实际结果

最终运行时间为香港时间 **17:16:44–17:16:57，约 13 秒**。Windows `10.0.26300` x64，
`NEWNAME\q9560` 沙箱外运行；Node `v25.2.1`、Playwright Core `1.63.0`，串行场景。
被验代码为 `bbd1c35` 加当前验收脚本、依赖及 `acceptance` 数据路径分支；不能把这个带功能开关
的验收构建当作先前安装包。生产前端由 `npm run build` 生成并嵌入 Tauri 二进制。

二进制 SHA-256：`8b2c70cc988a9083d9daeef03fc736bf8fc4df1ed318f6f796dc6fd19fcc9446`。
脚本与数据路径源码哈希、各项耗时、实际目录和清理 PID 见
[归档报告](acceptance/2026-09-30-native-automation/report.json)。

| 实际原生检查 | 结果 |
| --- | --- |
| 独立配置报告路径、真实后台连接、四个已注册会话、无 UI 预览标签 | 通过 |
| T-11：运行中终端的用途与关闭影响原文；未配置字段时为 `—` | 通过 |
| 手动记录开始与停止的按钮和 Capturing/Off 状态 | 通过 |
| 窗口键盘输入经 xterm/IPC/ConPTY 到真实 PowerShell，得到独立输出标记 | 通过 |
| 运行中保存反馈、真实日志文件存在、文件动作可用 | 通过 |
| 真实文件系统写入失败显示原因、不报成功、会话仍运行 | 通过 |
| 前端重载后后台会话 PID/运行状态保持、窗口摘要为 4/4 运行 | 通过 |
| 合法/非法混合配置保留合法会话并显示 `invalid · type` 诊断 | 通过 |
| YAML 格式错误展示 | 通过 |
| 缺失配置展示首次运行 | 通过 |
| 合法空配置展示空配置说明 | 通过 |
| 配置路径为目录时展示真实读取失败 | 通过 |

日志写入失败通过在**私有**日志位置创建一个同名文件阻挡目录创建来复现，真实后端返回
`creating the log directory`、Win32 错误 183。不是伪造响应或实际修改用户目录权限。
终端输入拼接两段字符串后打印输出，完整标记不出现在输入回显中，避免把输入回显误判成执行成功。

六个应用进程与四个运行中的会话 PID 均确认退出。用户配置运行前后 SHA-256 相同。
原生截图保留于报告同目录，包括 [终端字段](acceptance/2026-09-30-native-automation/terminal-fields.png)、
[终端输入结果](acceptance/2026-09-30-native-automation/terminal-roundtrip.png)、
[保存结果](acceptance/2026-09-30-native-automation/saved-log.png) 与
[真实写入失败](acceptance/2026-09-30-native-automation/write-error.png)。

T-11 截图已对照 `assets/ui/ui-v2-terminal.png`：用途和关闭影响原文、头部与提示条层级一致。
真实 PID、cwd、会话数量和实时终端内容与参考 fixture 不同。截图来自真实 WebView2 内容区域，
不包含原生窗口装饰；不扩大为所有布局、DPI、托盘和图标外观均已通过。

## 其他检查与保留边界

前端 14 文件、205 项测试、类型、Lint、格式和生产构建通过；Rust 默认与 `acceptance`
功能开关下的 Clippy、Rust 格式通过。默认构建的完整 Rust 测试在沙箱外 Windows 用户环境串行
执行，335 单元与 10 集成测试通过，见 [完整日志](acceptance/2026-09-30-native-automation/rust-default.log)。
其中即时子进程归属、停止/重启、无关哨兵、ConPTY 均是真实 Windows 边界。

准备依赖时沙箱默认 npm cache 写入失败（EPERM）；改为仓库临时 cache 后安装成功。
npm 对现有 Vitest/Node 25 组合给出引擎兼容提示，本轮测试通过；不把提示写成产品故障。
构建保留大于 500 kB 的 chunk 提示和 MSVC linker stdout 提示，实际退出码 0。

以下范围**未由本脚本补验**：

- 真实托盘菜单点击、初始化快照延迟期间从托盘启停；窗口重载检查不能替代这项，仍引用 #54 已有人工结果。
- 原生文件/目录打开后的外部应用外观；本轮验证文件存在与按钮可用，未启动外部查看器。
- 安装版 I-7、I-8、I-15、I-16/I-17 的待验部分，以及窗口装饰、任务栏和托盘图标。

历史人工确认与版本记录不覆盖。本轮增加可重复的实际应用自动化证据，不代表正式发布门槛全部完成。
