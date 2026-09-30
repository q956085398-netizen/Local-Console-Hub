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
2026-09-30 实测（该行前提：安装版是当天 18:28 的构建，sha256 E94EC508…，不含本片代码）

开始菜单快捷方式  C:\Users\q9560\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Local Console Hub.lnk
                  → E:\Local Console Hub\local-console-hub.exe
实测进程身份       pid=29608  parent=powershell.exe  （该次由探针启动；用户自己双击时 parent 是 explorer.exe）
窗口              hwnd=2102628 class='Tauri Window' title='Local Console Hub' 1294x837 visible=True
控制台            False（与 §2 同一种 `AttachConsole` 探针：目标进程没有控制台对象时它失败）
```

这一行是**安装版**的实测，不是从 PE 头推断的；探针与 §2 脚本里的 `Test-HasConsole`
是同一段代码，只是这里把它单独指向了安装版。它证明的是**安装版这一份构建不产生启动
终端**，不是「本片的新代码已在实际入口上验证过」——后者见 §4 第 1 条。

**修正的是什么。** 日常入口本身已经不产生终端，所以本片在这条路径上要修的不是子系统，
而是入口的**复用行为**：同一个入口连开两次，同一次实测得到两个 Hub（pids 29608、45800，
两个都活着）。两个 Hub 各自有自己的会话监管器与托盘图标，共用一份 `config.yaml`——
这才是「直接把 Hub 打开」每天真正会踩到的问题。#60 之后同一个入口第二次只把窗口恢复
回来，进程数保持 1（见 §2 的 H02）。

也就是说，本片**没有**改 `windows_subsystem`，也**没有**改 `scripts\verify-*.cmd`：
诊断的结论是那条路径上没有需要改的日常入口。要改的是复用行为，已经改了；剩下的是
把新构建放到安装目录（§4 第 1 条）。

**开发入口保持原样是有意的。** `scripts\verify-*.cmd` 里的启动窗口是验收流程需要的
（脚本自己写着「Keep this window open while testing」），它属于开发而不是日常使用。

---

## 2. 自动测量：真实 Windows 上的一轮运行

`scripts/verify-single-instance.ps1` 做的是**可测量的那部分**：进程数量、退出码、窗口
可见性、有没有多出终端、入口子系统与控制台。运行方式：

```bash
powershell -NoProfile -File scripts\verify-single-instance.ps1
```

（要测**实际安装的日常入口**，重装后再用 `-App "E:\Local Console Hub\local-console-hub.exe"`
指过去；见 §4 的未运行项。）

### 环境与被测构建

```text
机器        Windows 11 Pro（10.0.26300）
会话        登录用户会话；脚本在正常用户环境中运行
时间        2026-09-30
被测文件    src-tauri\target\release\local-console-hub.exe
sha256      184A029DBF08C6FF0682A05ED7EF1291F8E9BE9582F9A2559C7330AA43669469
PE 子系统   WINDOWS_GUI
开始菜单快捷方式指向  E:\Local Console Hub\local-console-hub.exe（该文件是更早的构建，
                       见 §4「未运行」第 1 条）
```

### 结果：22 项，22 PASS / 0 FAIL

脚本输出为英文（CLI 输出按仓库惯例与语言规范保持英文），下表照抄它的检查名与实测值。

| 组 | 检查（脚本原文） | 实测 |
| --- | --- | --- |
| H01 | `exactly one Hub process after opening` | 1 |
| H01 | `the Hub process is alive and responding` | `HasExited=False, Responding=True` |
| H01 | `the entry is not a console program` | `WINDOWS_GUI` |
| H01 | `a window exists` | `hwnd=7799702 class='Tauri Window' 1294x808` |
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
| H02 | `exactly one Hub survives the race`（间隔 40 ms 启动两次） | 1（pid 46200） |
| H02 | `one side handed over and ended` | 先启动=仍在运行，后启动=`exit code 0` |
| H02 | `the winner has a window` | `pid=46200 hwnd=1770636` |
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

## 3. 人工步骤（脚本证明不了的部分）

以下三项需要人在桌面上确认。前两项属于本片，第三项属于 #68。

1. **托盘手势本身。** 从托盘菜单点「显示窗口」，或点击托盘图标打开菜单——脚本发的是
   `WM_CLOSE`，等价于关闭窗口那个隐藏路径，但不是托盘菜单点击本身。
   期望：窗口恢复，且恢复后仍是同一个 Hub（任务栏只有一个 Hub 窗口，托盘只有一个图标）。
2. **受管会话在恢复后继续可交互。** 主窗口里启动一个 PowerShell 会话，输入
   `Write-Output 'SINGLE-INSTANCE-OK'` 回车看到输出；然后依次做：最小化、关闭窗口
   （隐藏到托盘）、再用日常入口打开。每次恢复后回到那个终端，再输入一次同样的命令。
   期望：会话没有被重启或复制（PID 不变），输出正常，Ctrl+C 仍只中断命令。
   会话生命周期本身不经过启动请求这条路径（`app::launch` 只调用窗口恢复，不调用
   Session Core 的任何操作），这一条是把那句话坐实，而不是从代码推断。
3. **图标一致性。** 本片不动图标；#68 处理。

---

## 4. 未运行 / 边界

1. **在实际安装入口上复测「本片的行为」：未运行。** §1 测的是安装版的**身份与控制台**
   （那一行有实测支撑）；§2 测的是**新构建**在 `src-tauri\target\release\` 上的行为。
   两者之间那一步——把新构建装到 `E:\Local Console Hub\`，再从开始菜单快捷方式复测
   H01/H02——没有做。开始菜单仍指向 sha256 `E94EC508…`（2026-09-30 18:28 的构建，
   不含本片代码）。要把它记为通过：`npm run tauri build` 产出安装包、安装，然后
   `powershell -NoProfile -File scripts\verify-single-instance.ps1 -App "E:\Local Console Hub\local-console-hub.exe"`。
   §2 那一轮的测量方法对安装版同样适用，但**方法适用不等于结果已取得**；本轮没有替用户
   重新安装的原因是不在未经确认的情况下替换他机器上正在用的安装版。
2. **托盘菜单手势、受管会话继续运行、图标外观：未运行**，见 §3。
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

脚本会**拒绝**在已有 Hub 运行时执行（它只结束自己启动的进程，不按进程名扫，避免误杀
用户正在使用的那个 Hub）；收尾用 `Stop-Process` 强制结束它自己启动的进程——本脚本
从不启动任何会话，被结束的 Hub 只是被打开过窗口。真正的退出路径由 #54/#57 的验收覆盖。

脚本是纯 ASCII：仓库里其它 `scripts\*.ps1|*.cmd` 同样是纯 ASCII，CLI 输出按语言规范
保持英文。中文结论在本文件里，不在脚本里。
