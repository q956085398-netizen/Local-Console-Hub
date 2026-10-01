# #63 日常 PowerShell 快捷方式进入 Hub：诊断与 Windows 验收

这份文档回答三件事：**用户自己的 PowerShell 快捷方式怎样变成 Hub 的入口**（§1–§2），
**同一个入口在冷启动 / 已运行 / 托盘隐藏时到底做了什么**（§3 的自动测量），以及
**新建出来的终端是不是真的被选中并拿到键盘**（§4 的窗口实测）。

结论先写在这里：本片把快捷方式变成一条 `--new-terminal [--directory <目录>]` 的命令行请求，
由 Hub **自己**创建终端（不是转交给窗口去做），成功才回答请求方；窗口侧的「选中并聚焦」由
事件加一次启动读取共同保证，冷启动那条也不会丢。29 项自动检查全通过，冷启动与托盘隐藏两条
的选中/聚焦在真实窗口上用 DevTools 协议实测通过。

---

## 1. 一次请求，两种拼写

`instance::protocol::Request` 同时描述命令行与管道帧：命令行是
`--new-terminal` / `--directory <路径>`，管道帧是
`json {"request":"newTerminal","directory":"…"}`（见 `docs/DECISIONS.md` D-033 第 1 条）。
不认识的参数**按名字停下**并报错，不是忽略——忽略会让一个要终端的入口只得到一个窗口。

`Hub::handle` 直接调用 `SessionCore::create_temporary_terminal`，也就是窗口内「新建 PowerShell」
按下的**同一条**路径；成功才 `delivered`，失败把原因（目录不存在、机器上没有 PowerShell）
作为回答交回请求方。这个选择是可测的：命令行的失败在启动进程退出码上看得见（§3 的 H06 两行），
而不是落在窗口的状态栏里。

## 2. 入口的安装与还原

`scripts/install-powershell-shortcut.ps1` 只改**命令行点名的那一个 `.lnk`**：

- 目标改成 Hub 的 exe，参数写成 `--new-terminal --directory "<目录>"`；
- 目录来自快捷方式自己的「起始位置」（**展开环境变量**——系统自带的 PowerShell 快捷方式写的
  是 `%HOMEDRIVE%%HOMEPATH%` 而不是路径），`-Directory` 可以覆盖；
- 目录不存在时**拒绝写入**：那种入口每点一次都会失败；
- 改写前把原文件备份成 `<名字>.lch-original.lnk`，`-Restore` 放回；
- 目标不是 shell 的快捷方式默认拒绝（`-Force` 才改），避免手滑把别的入口挪用；
- 目标是控制台子系统的构建（开发产物）时给出警告——那会带回一个常驻终端，正是本入口要消掉的东西。

实测（`scripts/install-powershell-shortcut.ps1`，工作树内一次真实调用）：

```text
after install : <工作树>\src-tauri\target\release\local-console-hub.exe --new-terminal --directory C:\Users\q9560\AppData\Local\Temp
after restore : C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile
refusal exit  : 2 (目录不存在时拒绝写入，不产生半成品入口)
```

参数的引号规则按 Windows 重新拆分命令行的规则来（含空格才加引号、内部引号转义、结尾反斜杠
翻倍），所以带空格与中文的目录能原样到达 Hub；§3 的检查组里有一条专门钉住这一点。

## 3. 自动测量：真实 Windows 上的一轮运行

`scripts/verify-shortcut-entry.ps1` 在临时目录里**用上面那个安装脚本**造一个入口（目标原本是
Windows PowerShell、起始位置是一个带空格与中文的目录），然后照用户的手势启动它并测量。

```bash
npm run tauri build -- --no-bundle
powershell -NoProfile -File scripts\verify-shortcut-entry.ps1
```

> **构建说明。** 必须用 `npm run tauri build`（可加 `--no-bundle`）。裸的
> `cargo build --release` 不带 `custom-protocol` 特性，产物会去加载 `devUrl`，窗口内容是
> 连接错误页；进程数、窗口与 shell 数不受影响，但要代表产品的那一轮必须用生产构建。

### 环境与被测构建

```text
机器        Windows 11 Pro（10.0.26300）
时间        2026-10-01
被测文件    <工作树>\src-tauri\target\release\local-console-hub.exe
sha256      EBDF89D206ABFD2338D28C59B941F5FDC44230C192FFEDDBA3BBC56746F98335
构建方式    npm run tauri build -- --no-bundle（生产构建，前端资源内嵌）
前提条件    机器上没有别的 Hub 在运行（脚本自己会拒绝）；脚本只结束它自己启动的进程
```

### 结果：29 项，29 PASS / 0 FAIL

脚本输出为英文（CLI 输出按仓库惯例保持英文），下表照抄它的检查名与实测值。

| 组 | 检查（脚本原文） | 实测 |
| --- | --- | --- |
| 入口 | `the installer rewrites the chosen shortcut at the Hub` | 目标 = 被测 exe |
| 入口 | `the request is a new-terminal request` | `--new-terminal --directory "…\lch-t63 工作 目录"` |
| 入口 | `the directory travels as one quoted argument (space and Unicode intact)` | 目录含空格与中文，作为**一个**带引号参数 |
| 入口 | `the original shortcut was saved beside itself` | `True`（`Daily PowerShell.lch-original.lnk`） |
| 入口 | `the user's own Start menu shortcut is untouched` | 仍指向 `E:\Local Console Hub\local-console-hub.exe` |
| 真实入口形状 | `the user's entry keeps its meaning: its start directory is expanded` | 副本原本是 `%HOMEDRIVE%%HOMEPATH%`，转换后是 `C:\Users\q9560`（真实存在的目录） |
| 真实入口形状 | `the user's own shortcut file is still theirs` | 磁盘上那一个仍指向它原来的 `powershell.exe` |
| H04 冷启动 | `H04 the Hub starts, exactly one process` | 1 |
| H04 冷启动 | `H04 the window is on the desktop` | `hwnd=… visible=True` |
| H04 冷启动 | `H04 the shortcut created one terminal inside it` | `1: pwsh.exe(…)` |
| H04 已运行 | `H04 the launch is handed to the running Hub and ends` | `exit code 0` |
| H04 已运行 | `H04 still exactly one Hub` | 1 |
| H04 已运行 | `H04 the second click added exactly one more terminal` | 2 shells |
| H04 托盘隐藏 | `the Hub is off the desktop (closed to the tray)` | `visible=False, Hub processes=1` |
| H04 托盘隐藏 | `H04 the shortcut restores the window and adds a terminal` | `visible=True exit code=0 shells=3` |
| H04 近乎同时 | `each independent click adds its own shell`（间隔 40 ms 两次） | 5 shells，两次都 `exit 0` |
| H04 近乎同时 | `the single-instance rule did not swallow the second request` | 1 Hub |
| H06 无效目录 | `a directory that is not there is refused with the reason` | 消息框原文含该目录路径 |
| H06 无效目录 | `the refusal is reported to the process that asked` | `exit code 1` |
| H06 无效目录 | `nothing was opened somewhere else instead` | shells 5 → 5（**没有**静默换目录） |
| H06 冷启动无效目录 | `cold: the Hub opens, names the directory, and starts no terminal` | `window=yes`，消息框含路径，`shells=0` |
| H06 冷启动无效目录 | `cold: the Hub stays open after refusing` | `1 (running)` |
| 普通入口 | `opening the Hub itself starts no terminal` | `exit code=0, shells 0 -> 0` |
| 普通入口 | `opening the Hub itself leaves exactly one Hub` | 1 |
| H17 | `the shell executable itself is unchanged` | `powershell.exe` sha256 前后一致 |
| H17 | `a PowerShell started outside the Hub behaves as it always did` | `exit code 7` |
| H17 | `the Hub is not its handler` | 该进程的父进程不是 Hub |
| 收尾 | `no Hub process left behind` | 0 |

两点说明：

- **「一个终端」用 Hub 的 shell 子进程数衡量**：每个临时终端是一棵 ConPTY 会话，shell 是 Hub
  的子进程。计数只看 `powershell.exe`/`pwsh.exe`，不看 `conhost`——「入口造了一个终端」是关于
  shell 的陈述，conhost 怎么被托管是实现细节。
- **「真实入口形状」那一组跑的是机器上真实 PowerShell 快捷方式的副本**（不是手搓的 fixture）：
  它的「起始位置」写的是 `%HOMEDRIVE%%HOMEPATH%` 而不是路径，正好覆盖安装脚本必须展开环境
  变量的那条分支——手搓的 fixture 永远测不到它。原文件全程只读，脚本反向断言它没被改动。
- **脚本是纯 ASCII**（仓库约定，见 `docs/SINGLE_INSTANCE_ACCEPTANCE.md` §5）。带空格与中文的目录
  名在脚本里用码点拼出来（`[char]0x5DE5` …）：Windows PowerShell 5.1 读取无 BOM 的 `.ps1` 用
  的是 **ANSI 代码页**，直接写字面量中文会在内存里变成乱码——那样这一条测的是另一个名字，
  而不是「Unicode 目录」。
- **无效目录那一组读的是消息框本身**：脚本用 `EnumWindows`/`EnumChildWindows` 找到请求方
  进程的 `#32770` 窗口并读出它的文字，断言里面有那个不存在的目录路径，然后关掉它、再读退出码。
  这比「退出码是 1」更强：它证明用户看到的是**具体原因**（story 17、H06），而不是一句泛泛的失败。

这一轮**同时**覆盖了规格 §5 的「同一个入口近乎同时两次」：40 ms 间隔的两次独立点击各自新增
一个 shell（5 = 3 + 2），而 Hub 始终只有一个——单实例规则没有吞掉主动多开。

## 4. 窗口实测：新增的终端被选中并拿到键盘

自动脚本测量不了「窗口选中了哪一个会话」（那在 WebView 里）。这一条用 DevTools 协议读真实
窗口的 DOM：

```bash
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333   # 启动 Hub 时带上
# 冷启动：Hub 未运行，直接以 --new-terminal --directory "<目录>" 启动
# 之后用 CDP 的 Runtime.evaluate 读侧栏与焦点
```

两次读数（`selected` = 侧栏中带 `session-row--selected` 的行，`focusedTerminal` = 键盘焦点是否
落在 xterm 的输入区）：

| 场景 | 启动前 | 读数 |
| --- | --- | --- |
| **冷启动**（Hub 未运行，直接带 `--new-terminal`） | 无 Hub | `selected="PowerShell 1"`, `tab="终端"`, `activeElement=TEXTAREA.xterm-helper-textarea`, `focusedTerminal=true` |
| **已运行且藏在托盘**（`WM_CLOSE` → `visible=False` → 再点入口） | `visible=False`, 1 Hub | 窗口 `visible=True`，`exit code 0`，`selected="PowerShell 2"`, `tab="终端"`, `focusedTerminal=true` |

冷启动那一条是本片最容易被做错的地方：请求在 `setup` 里就被执行，那时页面还没加载，
发给窗口的事件没有监听者。实测之所以通过，是因为后端**先存后发**（`PendingFocus`），窗口挂载时
**先订阅、再读一次** `take_launch_focus`（D-033 第 3 条）。两次读数里的会话名都是**刚建出来的
那个**（`PowerShell 1` / `PowerShell 2`），不是配置文件里已有的会话（工作区里另有 5 个配置会话）。

这一条用的是**本片新增的 `session-opened` 事件**，不是托盘的 `session-focus-requested`：
托盘那一行是「让我看看这个会话」，入口这一行是「落在我刚建出来的终端上」，两者的窗口行为
不同，托盘交付时的行为（只选中、不切页签）因此原样保留。

## 5. 边界与未运行项

1. **多点击那几组用的是临时目录里由安装脚本造的入口；用户日常入口只以「副本」参与。** 验收
   不该改动用户每天点的那个文件，所以脚本先在临时目录造一个同等形状的入口跑完所有启动测量，
   再把机器上**真实的** PowerShell 快捷方式复制一份、对它跑同一个安装脚本（见 §3 的
   「真实入口形状」两组）。原文件全程只读，脚本反向断言它未被改动——"只改用户明确选用的入口"
   这条因此有一半是**反证**出来的。
2. **图标一致性未验。** 本片不动图标，安装脚本也刻意保留原快捷方式的图标；统一图标由
   「合并标题栏并统一 Hub 图标」工单（#68，已合并）与「完整日常流程」工单（#69）补验。
3. **安装版未复测。** 本轮在被测工作树的生产构建上跑。要在实际安装的 exe 上复测，先重装再用
   `-App "E:\Local Console Hub\local-console-hub.exe"` 指过去（与 `verify-single-instance.ps1`
   同一约定）。
4. **开发构建会多一个控制台窗口。** 指向 debug 产物时安装脚本会警告：那是控制台子系统程序，
   与 #60 §1 诊断出的「常驻启动终端」是同一件事。
5. **同一 WebView2 profile 下不同路径的两个 Hub 不能并存**（#60 §1 末节）。跑本片验收前请先退出
   其它构建的实例，脚本自己会因为「已有 Hub 在运行」而拒绝。
6. **`cargo build --release` 不等于产品构建**（§3 构建说明）。这条是本轮实测踩到的，记在这里
   以免下次有人拿它去验窗口内容。

## 6. 复现方式

```bash
npm run tauri build -- --no-bundle
powershell -NoProfile -File scripts\verify-shortcut-entry.ps1
```

也可以双击 `scripts\verify-shortcut-entry.cmd`（它只是包一层 `-File` 并 `pause`）。

脚本会**拒绝**在已有 Hub 运行时执行，并且只结束它自己启动的 Hub 进程，不按进程名扫。三个
`.ps1`（安装脚本、本验收脚本、`verify-single-instance.ps1`）都是纯 ASCII，中文结论在本文件里。

## 配置应用入口（#89）

已保存应用另使用 --open-app <id>；创建方式及原生证据见 [APPLICATION_SHORTCUT_ACCEPTANCE.md](APPLICATION_SHORTCUT_ACCEPTANCE.md)。
