# #60 直接启动并复用同一个 Hub：诊断与 Windows 验收

这份文档回答两件事：**用户任务栏上那个多余启动终端到底从哪来**，以及**同一个 Hub
是否真的被复用**。前者是诊断，后者是 `scripts/verify-single-instance.ps1` 在真实
Windows 上跑出来的一轮测量。

结论先写在这里：多余启动终端来自**开发启动**，不是日常入口；而日常入口的缺陷是
**连开两次会得到两个 Hub**（实测 2 个进程）—— 这正是本片修掉的那条。

---

## 1. 诊断：多余的启动终端来自哪里

规格 §「实现阶段先定位额外终端究竟来自开发启动、脚本入口、应用自身或受管程序，再修正
实际路径」，并明确警告不能只凭「release 用了 GUI 子系统」宣布通过。三种构建的实测：

| 构建 | 路径 | PE 子系统 | 启动后会不会多出一个终端 |
| --- | --- | --- | --- |
| 已安装的日常入口 | `E:\Local Console Hub\local-console-hub.exe` | `WINDOWS_GUI` | 不会（实测 `AttachConsole` 失败 = 该进程没有控制台对象） |
| release 产物 | `src-tauri\target\release\local-console-hub.exe` | `WINDOWS_GUI` | 不会 |
| 开发产物 | `src-tauri\target\debug\local-console-hub.exe` | `WINDOWS_CUI` | **会**：控制台子系统进程由无控制台的父进程拉起时，Windows 为它新建一个控制台窗口 |

`main.rs` 的 `windows_subsystem = "windows"` 是 `cfg_attr(not(debug_assertions), …)`，
所以开发构建是控制台程序，`npm run tauri dev` 与 `scripts\verify-*.cmd` 这类入口
都会带着一个 `cmd`/`conhost` 窗口活到应用退出为止。**这就是那个常驻启动终端。**

它同时也是「不能只确认 release 子系统设置」的原因：子系统这件事一句话就能查完，
但真正要回答的是**用户每天点的是哪一个**。这里查到的答案是：

```text
2026-09-30 改动之前的基线（此时安装版还是当天 18:28 的构建，sha256 E94EC508…，不含本片代码）

开始菜单快捷方式  C:\Users\q9560\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Local Console Hub.lnk
                  → E:\Local Console Hub\local-console-hub.exe
实测进程身份       pid=29608  parent=powershell.exe  （该次由探针启动；用户自己双击时 parent 是 explorer.exe）
窗口              hwnd=2102628 class='Tauri Window' title='Local Console Hub' 1294x837 visible=True
控制台            False（与 §2 同一种 `AttachConsole` 探针：目标进程没有控制台对象时它失败）
```

这一行是**安装版**的实测，不是从 PE 头推断的；探针与 §2 脚本里的 `Test-HasConsole`
是同一段代码，只是这里把它单独指向了安装版。它证明的是**安装版这一份构建不产生启动
终端**。

**修正的是什么。** 日常入口本身已经不产生终端，所以本片在这条路径上要修的不是子系统，
而是入口的**复用行为**：同一个入口连开两次，同一次实测得到两个 Hub（pids 29608、45800，
两个都活着）。两个 Hub 各自有自己的会话监管器与托盘图标，共用一份 `config.yaml`——
这才是「直接把 Hub 打开」每天真正会踩到的问题。#60 之后同一个入口第二次只把窗口恢复
回来，进程数保持 1（见 §2）。

也就是说，本片**没有**改 `windows_subsystem`，也**没有**改 `scripts\verify-*.cmd`：
诊断的结论是那条路径上没有需要改的日常入口。要改的是复用行为，已经改了。

**开发入口保持原样是有意的。** `scripts\verify-*.cmd` 里的启动窗口是验收流程需要的
（脚本自己写着「Keep this window open while testing」），它属于开发而不是日常使用。

### 顺手测到的一条：同一 WebView2 profile 下，不同路径的两个 Hub 不能并存

做这轮验收时机器上另有一个开发实例（另一个 worktree 的 debug 构建）在跑，它和安装版
**用同一个 identifier**，因此用同一个 WebView2 user-data 目录。实测：

| 情形 | 结果 |
| --- | --- |
| 同一路径的 exe 连开两次（改动之前） | 两个 Hub 都活着（各自监管一份会话） |
| **不同路径**的两个 Hub 同时开 | 第二个创建 webview 失败：`HRESULT 0x8007139F`，退出码 **101**（Rust panic） |

也就是说，改动之前「再点一次入口」的后果取决于第二次点的是哪一份拷贝：同一份拷贝会
多出一个 Hub，另一份拷贝会当场崩掉。两种都不是用户想要的，而 #60 之后这两种都不会发生
——第二次调用根本不会走到创建窗口那一步。这条不是规格要求的验收项，是这轮实测的副产品，
记在这里以免下次有人把它当成新问题。

它同时是 §2 那一轮的运行前提：跑验收时机器上不能有别的构建在运行。

---

## 2. 自动测量：真实 Windows 上的一轮运行

`scripts/verify-single-instance.ps1` 做的是**可测量的那部分**：进程数量、退出码、窗口
可见性、有没有多出终端、入口子系统与控制台。运行方式：

```bash
powershell -NoProfile -File scripts\verify-single-instance.ps1
```

（要测**实际安装的日常入口**，先重装、再用 `-App "E:\Local Console Hub\local-console-hub.exe"`
指过去——下面这一轮就是这么跑的。）

### 环境与被测构建

```text
机器        Windows 11 Pro（10.0.26300）
会话        登录用户会话；脚本在正常用户环境中运行
时间        2026-09-30
被测文件    E:\Local Console Hub\local-console-hub.exe      ← 实际日常入口（开始菜单快捷方式指向它）
sha256      9CE92B2E354D91055152BB8389DFA0F017946CB48D643DDF33508004E9D57F5D
PE 子系统   WINDOWS_GUI
安装方式    npm run tauri build -- --bundles nsis 产出的
            Local Console Hub_0.1.0_x64-setup.exe /S /D=E:\Local Console Hub
            （就地覆盖安装；安装后 HKCU\...\Uninstall\Local Console Hub 的
             InstallLocation 与开始菜单快捷方式都指向 E:\Local Console Hub）
运行前提    机器上没有别的构建在运行（见 §1 末节的那条 WebView2 限制）
```

本轮同一份脚本也在工作区的 release 产物上跑过一次（sha256 `184A029D…`，
22/22 PASS），两次结果一致；下表照抄的是**安装版**那一轮。

### 结果：22 项，22 PASS / 0 FAIL

脚本输出为英文（CLI 输出按仓库惯例与语言规范保持英文），下表照抄它的检查名与实测值。

| 组 | 检查（脚本原文） | 实测 |
| --- | --- | --- |
| H01 | `exactly one Hub process after opening` | 1 |
| H01 | `the Hub process is alive and responding` | `HasExited=False, Responding=True` |
| H01 | `the entry is not a console program` | `WINDOWS_GUI` |
| H01 | `a window exists` | `hwnd=9178144 class='Tauri Window' 1294x808` |
| H01 | `the Hub process owns no console` | `False`（子进程 `AttachConsole` 失败 = 该进程没有控制台对象） |
| H01 | `no conhost or shell in the Hub process tree` | 0 |
| H02 | `the second launch ends within the bound` | `exit code 0` |
| H02 | `still exactly one Hub process` | 1 |
| H02 | `the normal open restored the same Hub window` | 同一个 hwnd，`visible=True` |
| H02 | `the normal open started no terminal` | 0 |
| 恢复 | `minimized takes the window off the desktop, the Hub keeps running` | `visible=True iconic=True`，Hub 进程数 1 |
| 恢复 | `minimized then open again restores the window` | `visible=True iconic=False`，`exit code 0` |
| 恢复 | `minimized leaves exactly one Hub process` | 1 |
| 恢复 | `closed (hidden to the tray) takes the window off the desktop…` | `visible=False iconic=False`，Hub 进程数 1 |
| 恢复 | `closed (hidden to the tray) then open again restores the window` | `visible=True iconic=False`，`exit code 0` |
| 恢复 | `closed (hidden to the tray) leaves exactly one Hub process` | 1 |
| H02 | `cleared before the race` | 0 |
| H02 | `exactly one Hub survives the race`（间隔 40 ms 启动两次） | 1（pid 45856） |
| H02 | `one side handed over and ended` | 先启动=仍在运行，后启动=`exit code 0` |
| H02 | `the winner has a window` | `pid=45856 hwnd=4985610` |
| H02 | `the winner owns no console` | `False` |
| 收尾 | `no Hub process left behind` | 0 |

两点说明：

- **「关闭窗口」用 `PostMessage(WM_CLOSE)` 触发**，走的是应用自己的关闭处理
  （D-006：关闭 = 隐藏到托盘），顺带证明关掉窗口不会退出 Hub。脚本里另有一条注释
  记着为什么不直接 `ShowWindow(SW_HIDE)`：tao 记着窗口是可见的，`show()` 会因为
  「状态没变」而不下发调用，于是窗口留藏在隐藏状态——那是脚本的假动作，不是产品的
  行为，用它做验收会得到一条永远为红的断言。
- **最小化后 `IsWindowVisible` 仍是 True**（Windows 只是把它收成图标），所以两种手势
  的「离开桌面」判据不同：最小化看 `iconic`，关闭看 `visible`。同一个判据套两种手势
  同样是永远为红的断言。
- **这一轮是在 #68 合并之后重跑的**（窗口不再有系统装饰，D-029）。主窗口因此从
  `1294x837` 变成 `1294x808`，但窗口识别、恢复、隐藏各条都仍然通过——标题文本由
  应用设置，与有没有系统标题栏无关；脚本判据里没有一条依赖系统装饰。

### 冷启动与超时

冷启动竞争用的是「先启动的继续运行、后启动的退出码 0」，不是靠时间差碰运气：后启动
的那个在 `CreateMutexW` 上拿到 `ERROR_ALREADY_EXISTS`，于是走交付路径。交付的时限是
`instance::DELIVERY_TIMEOUT`（20 s），服务端自己的读请求限时 3 s、Hub 就绪等待 15 s，
两者之和落在客户端时限之内（`instance::win` 模块注释里记着这个预算）。

这三条时限相对实际启动有多宽，本轮实测过：从 `Start-Process` 到主窗口出现，
三次分别是 **396 ms / 116 ms / 98 ms**（本机、WebView2 已预热）。窗口出现之后
`show_window` 才有东西可恢复，所以 15 s 的就绪等待比实测慢两个数量级；即便首次安装
WebView2 的机器慢上几十倍，仍在预算内。

超时不是没有代价的：真到了 15 s，调用方会看到「Hub 尚未完成启动」并退出码 1，而 Hub
本身可能只是慢。这里是**如实报告**而不是继续等——继续等会让「入口点了没反应」变成
默认体验，而报错至少说出了实情；Hub 仍在运行，用户再点一次即可。本机没有复现过这条
路径（实测差两个数量级），它按「有界且报错」记录，不按「已通过」记录。

---

## 3. 真实手势那两条（已运行）

这两条测的不是「代码路径能不能走通」（那是 §2 的事），而是**真实手势**：托盘菜单上
那一下点击，以及恢复之后终端还能不能继续用。手势必须由人的手在真实桌面上做，所以
下面写清楚谁做了什么、测量是怎么取的。

### 3.1 托盘菜单的「显示主窗口」

操作：窗口先由 `WM_CLOSE` 隐藏到托盘（应用自己的关闭手势，D-006），然后**由用户在
托盘图标上点击、在菜单里选择「显示主窗口」**。测量由脚本完成，每 200 ms 采样一次窗口
状态，所以留下的是一条时间线，而不是事后的一次读数：

```text
t=0.00  watch start hwnd=5837156
t=0.08  visible=False iconic=False showCmd=1   ← 已隐藏到托盘
t=20.91 visible=True  iconic=False showCmd=1 foreground=5837156   ← 托盘菜单那一下
```

`showCmd=1` 是「正常（非最小化/非最大化）」，同一条记录里窗口还成了前台窗口
（`foreground=5837156`）。此后 Hub 进程数仍为 1、pid 不变。**通过。**

如实记一笔：同一天更早的一次尝试里，点击后约一分钟测量到的仍是隐藏态，原因没有查明；
此后两次都是「点一次即恢复」。上面这条时间线属于后者。

### 3.2 恢复之后终端还能用

操作方式（全部由 agent 完成，没有让用户敲键盘）：用
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` 启动安装版，再用
CDP 向真实窗口派发**真实鼠标与键盘事件**（`Input.dispatchMouseEvent` /
`Input.dispatchKeyEvent`，不是页面里的合成事件），因此敲进去的键走的是 xterm → IPC →
ConPTY 这条真路径。终端里敲的是往 `%TEMP%` 写文件，之后**从磁盘核对内容**——「命令真的
执行了」比「界面看起来还在」可信。

会话选 `term-pwsh`（PowerShell），启动后应用自己报告：`Ready`、`ConPTY · interactive ·
connected`、PID **46472**。

| 手势 | 手势后窗口 | 入口第二次调用 | 恢复后窗口 | Hub 进程 | 会话 shell PID | 恢复后再敲一条命令 |
| --- | --- | --- | --- | --- | --- | --- |
| 最小化 | `iconic=True` | 退出码 0 | `visible=True iconic=False 1294x808` | 1（pid 44896） | **46472，不变** | `si-after-min.txt` 写入成功 |
| 关闭到托盘 | `visible=False` | 退出码 0 | `visible=True iconic=False 1294x808` | 1（pid 44896） | **46472，不变** | `si-after-close.txt` 写入成功 |

会话 PID 不只来自进程表：应用自己的「详情」页在两次手势之后读回来仍是 `46472`——
既不是重启出来的新进程，也不是复制出来的第二份。**通过。**

### 3.3 图标一致性

本片不动图标；#68 已合并，那条验收在 `docs/VERIFICATION.md` 的 R-8 / W-6。

---

## 4. 状态与边界

1. **在实际安装入口上复测「本片的行为」：已运行（§2）。** 用户确认后重装到原位
   （`/S /D=E:\Local Console Hub`，见 §2 的环境块），随后用
   `powershell -NoProfile -File scripts\verify-single-instance.ps1 -App "E:\Local Console Hub\local-console-hub.exe"`
   在开始菜单快捷方式指向的那份 exe 上跑完，22/22 PASS。安装前那一份
   （sha256 `E94EC508…`）只作为 §1 的**改动前基线**保留，不再是被测对象。
2. **托盘菜单手势与「恢复后会话继续可交互」：已运行（§3.1、§3.2）。** 前者由用户在真实
   托盘上操作、脚本按 200 ms 采样记录；后者由 CDP 派发真实键鼠事件、从磁盘核对命令效果。
3. **另一个 Windows 会话里的第二个 Hub。** 身份对象落在 `Local\` 命名空间，所以承诺
   的范围是**当前登录会话**（D-030 记录了为什么不是 `Global\`）。同一个用户的另一个
   登录会话是另一张桌面，不在本片承诺内。
4. **`-App` 指向 debug 产物时 H01 的子系统那一条会红。** 这是有意的：开发构建确实是
   控制台子系统，而 §1 说明了这正是那个多余启动终端的来源。验收日常入口时请指向
   release 产物或安装版。
5. **「返回真实结果」的边界。** 交付的应答是 Hub 自己给出的，它能区分并且已经区分：
   「已受理并下发恢复」「还没有就绪」「没有可恢复的主窗口」「不认识这个请求」。
   它**不**声称窗口已经被看到——`show` 是投递到主线程的，窗口还没画出来时回读
   `is_visible` 会读到调用前的状态，那会变成对一次即将成功的恢复报假失败。窗口真的
   出现在屏幕上，是由 §2 脚本在恢复之后**从外部观察**到的（`hwnd` 同号且 `visible=True`），
   而不是由应答断言。

---

## 5. 复现方式

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml
powershell -NoProfile -File scripts\verify-single-instance.ps1
```

也可以双击 `scripts\verify-single-instance.cmd`（它只是包一层 `-File` 并 `pause`）。

脚本默认要求「整个用户会话里只有这一份 Hub」；机器上跑着别的构建时它会拒绝执行。
那种情况可以用 `-AllowOtherBuilds` 把口径收窄到「本次入口自己的进程」，它会把拒绝改成
一条警告。本轮记录的那次**没有**用它——机器是干净的，这是更严的那一种读法。

脚本会**拒绝**在已有 Hub 运行时执行（它只结束自己启动的进程，不按进程名扫，避免误杀
用户正在使用的那个 Hub）；收尾用 `Stop-Process` 强制结束它自己启动的进程——本脚本
从不启动任何会话，被结束的 Hub 只是被打开过窗口。真正的退出路径由 #54/#57 的验收覆盖。

脚本是纯 ASCII：仓库里其它 `scripts\*.ps1|*.cmd` 同样是纯 ASCII，CLI 输出按语言规范
保持英文。中文结论在本文件里，不在脚本里。
