# Windows 打包与安装验收

> **属于：** T12（#13，release candidate）。**取代：** 无。
> **配套：** `docs/EXECUTION_PLAN.md` 的 T12 说交付什么，`docs/MVP_IMPLEMENTATION_SPEC.md` §17
> 说 v0.1.0 的完成定义，`docs/VERIFICATION.md` 说**行为**怎么验。本文档只管**安装包**：
> 怎么打、装到哪、用户数据放在哪、装 / 卸 / 升三条路径怎么验、验过了没有。
> 面向使用者的那一半在 [`RELEASE_NOTES_v0.1.0.md`](RELEASE_NOTES_v0.1.0.md)。

---

## 1. 产物

`src-tauri/tauri.conf.json` 的 `bundle.targets` 是 `["msi", "nsis"]`，一次构建出两个安装包：

| 产物 | 安装模式 | 安装目录 | 需要管理员 |
| --- | --- | --- | --- |
| NSIS `Local Console Hub_0.1.0_x64-setup.exe` | `currentUser`（显式钉住，不是默认值凑巧） | `%LOCALAPPDATA%\Local Console Hub` | 否 |
| WiX `Local Console Hub_0.1.0_x64_en-US.msi` | per-machine | `C:\Program Files\Local Console Hub` | 是 |

NSIS 的安装目录不是猜的：生成的 `target/release/nsis/x64/installer.nsi` 里
`!define INSTALLMODE "currentUser"` 配上 `StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"`，
装完之后 `HKCU\…\Uninstall\Local Console Hub` 的 `InstallLocation` 也写着同一条路径（§5 有实测）。
安装目录里只有 `local-console-hub.exe` 与 `uninstall.exe`——没有 `icons\` 之类的目录，
图标是编进 exe 资源的。

NSIS 的安装模式写死在配置里而不是留给默认值，理由见 §3：`currentUser` 把安装目录放在
用户自己的 `%LOCALAPPDATA%` 下，正是它让「卸载删掉安装目录」这件事**碰不到**用户数据
成为一条可以在本机跑一遍的规则，而不是一句设计意图。

两个包都**没有代码签名**：`bundle.windows.certificateThumbprint` 之类的字段留空，
CI 也不产出安装包。后果是第一次运行时 Windows SmartScreen 会拦一句「未知发布者」，
用户要点「更多信息 → 仍要运行」。签名需要一张代码签名证书，属于要凭据的决定，
所以 T12 只把它记成已知限制（`RELEASE_NOTES_v0.1.0.md` §6 第 1 条），不在这里实现。

两者共用同一套图标（`src-tauri/icons/`，源文件 `assets/brand/hub-mark.svg`，
经 `scripts/generate-icon.mjs` + `npx tauri icon` 生成，D-029），以及同一份
`bundle` 元数据：`publisher` / `copyright` / `category` / `shortDescription` /
`longDescription` / `homepage`。这些字段不是装饰——它们就是 Windows 属性页里
「产品名称 / 文件版本 / 公司」那一栏的来源。

## 2. 用户数据在哪

**安装目录里没有用户数据。** 这是 T12 的验收项之一，也是本节存在的理由：安装器会删掉
自己的安装目录，如果用户数据落在那里，一次卸载就会把配置和运行历史一起带走。

| 内容 | 位置 | 谁写 |
| --- | --- | --- |
| 配置 | `%APPDATA%\LocalConsoleHub\config.yaml` | 用户（Hub 只读） |
| 日志 | `%LOCALAPPDATA%\LocalConsoleHub\logs\<session_id>\<YYYY-MM>\` | Session Core |
| 运行元数据 | `%LOCALAPPDATA%\LocalConsoleHub\metadata\<session_id>\<YYYY-MM>\` | Session Core |
| 缓存 | `%LOCALAPPDATA%\LocalConsoleHub\cache\` | Hub |
| WebView2 用户数据 | `%LOCALAPPDATA%\com.localconsolehub.hub\EBWebView\` | WebView2 运行时 |

`src-tauri/src/config/paths.rs` 的 `AppPaths` 是这些路径的唯一来源：
`AppPaths::from_env()` 只问 `dirs::config_dir()` / `dirs::data_local_dir()`，
两者都不看可执行文件在哪。`app::bootstrap` 之外没有第二处解析路径的地方。

### 2.1 安装目录名与数据目录名必须不同

```text
安装目录   %LOCALAPPDATA%\Local Console Hub     ← productName
数据目录   %LOCALAPPDATA%\LocalConsoleHub        ← APP_DIR_NAME
```

两者今天只差**空格**。把产品名缩成 `LocalConsoleHub`（一次看起来很自然的清理）会让它们
重合：NSIS 把程序装进 `%LOCALAPPDATA%\LocalConsoleHub`，卸载器删掉整个目录，用户的
`config.yaml` 与 `logs\` 一并消失。这条不变量由
`src-tauri/src/release.rs` 的 `the_install_directory_can_never_be_the_app_data_directory`
静态守着——`cargo test` 就会红，不需要有人在 review 里想起来。

同文件还守着另外五件与发布有关、但没有任何编译器会比对的事：三份 manifest 的版本号一致
（`package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json`）、
`productName` 与 `ipc::APP_NAME` 是同一个字符串、`identifier` 没有被悄悄改掉
（Tauri 由它推导 WiX upgrade code 与 NSIS 卸载注册表项，改了不是改名，是发布成第二个
无法覆盖升级的应用程序）、两个安装目标都还在、WebView2 的安装模式没有被改成别的
（§1 与 `RELEASE_NOTES_v0.1.0.md` 的已知限制都建立在它之上）。

### 2.2 第三处落盘：WebView2 的用户数据目录，以及卸载器那个复选框

上表最后一行不属于 Hub 自己：Tauri 把 WebView2 的用户数据目录放在
`%LOCALAPPDATA%\<identifier>`，也就是 `%LOCALAPPDATA%\com.localconsolehub.hub\EBWebView\`。
它是运行时的浏览器配置文件（缓存、着色器缓存、Crashpad、`Local State` 等），实测约
**126 MB / 612 个文件**，由 WebView2 自己建立与维护，删掉只是让首次启动慢一点。

卸载器确认页上那个复选框删的就是这个目录。Tauri 模板里相应的两句是
`RmDir /r "$APPDATA\${BUNDLEID}"`（`%APPDATA%` 下同名的那个实际不存在）与
`RmDir /r "$LOCALAPPDATA\${BUNDLEID}"`，其中 `BUNDLEID` 是 `identifier`，**不是**
`LocalConsoleHub`。于是：

- 勾选复选框 → WebView2 的 126 MB，连同安装器自己的两个注册表值（安装位置、安装语言）没了；
  用户的 `config.yaml` 与 `logs\` **还在**；
- 不勾选复选框 → 一切都还在。

这个行为是刻意的：T12 的验收项是「卸载不会意外销毁用户日志 / 配置」，§2.1 那条不变量守着
它。想彻底清干净，得手动删 `%APPDATA%\LocalConsoleHub` 与 `%LOCALAPPDATA%\LocalConsoleHub`——
`RELEASE_NOTES_v0.1.0.md` §5 就是这么写给用户的。

**但复选框的文案一度不是这样。** 上游模板把它写成 `Delete the application data`，而它删的是
上面那份浏览器数据：勾选它的用户会以为自己的运行历史没了，其实还在。[#47](https://github.com/q956085398-netizen/Local-Console-Hub/issues/47)
修的就是这个「说的和做的不一致」，选的方向是**让文案说实话**，而不是新增一条删用户数据的路径：

- 文案来自语言文件。`src-tauri/installer/languages/English.nsh` 是 Tauri 自带 `English.nsh`
  的逐字副本，只改了 `LangString deleteAppData` 一句；`tauri.conf.json` 用
  `bundle.windows.nsis.customLanguageFiles` 把它接上，并显式写了 `languages: ["English"]`。
  **但要清楚这一句是可选的**：Tauri 的 config schema 说 `customLanguageFiles` 的 key
  「必须同时加进 `languages` 数组」，而 2.12.0 的**实际行为不是这样**——§5 实测：去掉
  `languages` 之后，我们文件里的文案照样被挂上（构建仍退出 0，生成的 `English.nsh` 里就是我们
  那一句）。所以 `languages: ["English"]` 是在满足写下来的契约，不是在满足一个量出来的依赖；
  留着它是因为将来某个 bundler 版本真按 schema 收紧了，缺了它就会悄悄退回上游文案。
- 这一句现在是 `Delete WebView2 browser profile (not your config or logs)`：说的是它确实会删的
  浏览器数据，并且点明配置与日志不受影响。它 56 个字符，而卸载器那个控件的宽度是
  模板写死的 `400 * DPI / 96`——**再改这句话时留意长度**，超了不会报错，只会被裁掉。
- `customLanguageFiles` 是**替换**而不是合并，所以这份文件必须保持完整；bundler 版本新增
  `LangString` 时要重新抄一份上游文件、再把这一句改回来。缺字符串是**静默**的：`makensis`
  只打一条 `LangString "x" is not set in language table of language English` 的 warning 然后
  退出 0（§5 实测），而 CI 只做 debug `cargo build`、根本不跑打包——所以 `cargo test` 里
  有三个守卫（`src-tauri/src/release.rs`）：接上了没有、这份文件是否仍与上游同样多字符串、
  以及这一句有没有重新变回 application data。
- 这份文件里**不要**加 UTF-8 BOM：bundler 抄写时会自己加一个，两个 BOM 会让 `makensis`
  在 `Invalid command: ";"` 上直接失败（§5 实测）。

## 3. 构建

前置：Node.js 22+、Rust stable（MSVC）、WebView2 Runtime（Windows 11 自带）。
Tauri 首次打包会把自己的 WiX 3.14 与 NSIS 下载到 `%LOCALAPPDATA%\tauri`（已缓存则跳过）。

```bash
npm ci
```

```bash
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/` 下。打包前 `bundle` 里的版本号三处必须一致，
`cargo test` 会拦漂移（§2.1）。

升级用的版本号是 `tauri.conf.json` 的 `version`：MSI 的升级以 WiX upgrade code
（由 `identifier` 推导）为键，NSIS 以自己的注册表项为键；同键的新版本覆盖安装，
换键的版本是另一个应用。

## 4. 装 / 卸 / 升清单

一层是**可以在本机跑一遍的**（静默安装 / 卸载 + 文件系统断言），一层**只能人点**
（快捷方式图标、属性页外观、窗口本体）。每一条都写了它验的是哪条验收项。

### 4.1 干净安装

| # | 步骤 | 期望 |
| --- | --- | --- |
| I-1 | 记录 `%APPDATA%\LocalConsoleHub` 与 `%LOCALAPPDATA%\LocalConsoleHub` 的现状（文件清单 + 大小） | 作为后面几条的哨兵；**整个流程不修改它们** |
| I-2 | 静默安装 NSIS 包（`…-setup.exe /S`） | 无 UAC 提示；退出码 0。**双击安装**（非静默）时 Windows 会先弹 SmartScreen「未知发布者」，点「更多信息 → 仍要运行」继续——包没有签名，这是 §1 的已知限制。静默安装不经过这个提示，所以这一句**未经本机实测**，归人眼 |
| I-3 | 看 `%LOCALAPPDATA%\Local Console Hub` | 只有两个文件：`local-console-hub.exe`（程序本体，图标编在资源里）与 `uninstall.exe`；**没有** `config.yaml`、`logs\`、`metadata\`（验收项「用户配置 / 日志在安装目录之外」） |
| I-4 | 从安装目录启动 exe | 进程起来并**持续存活**（不是启动即崩）；窗口标题是 `Local Console Hub` 且 `Responding = True`；托盘图标出现（托盘那半**人眼**）（验收项「干净安装能启动」） |
| I-5 | 启动后重看 I-1 的两个目录 | 与安装前**逐字节一致**——安装与启动都没有在数据目录里写东西，也都没有往安装目录里写日志 |
| I-6 | 看 exe 的属性 → 详细信息（或 `(Get-Item …).VersionInfo`） | 产品名 `Local Console Hub`、文件版本 `0.1.0`、公司 `Local Console Hub contributors`、版权里的 `©` 是真的 U+00A9（控制台打印成 `?` 是代码页，不是资源坏了）（T12 的「版本元数据」） |
| I-7 | 看开始菜单快捷方式与 exe 的图标 | 与 D-029 的 Hub 图标一致（**人眼**）。快捷方式指向 `…\Local Console Hub\local-console-hub.exe`，工作目录是安装目录 |
| I-8 | 双击 `.msi` | 走 UAC 提权、装入 `C:\Program Files\Local Console Hub`（per-machine；NSIS 那份不需要管理员，这是两种包的区别）。**本次未跑**：需要管理员，见 §5 |

### 4.2 卸载

| # | 步骤 | 期望 |
| --- | --- | --- |
| I-9 | 静默卸载（`Uninstall *.exe /S`，或「应用和功能」里卸载） | 退出码 0。**静默卸载等价于复选框没被勾选**：`un.ConfirmLeave` 不会跑，`$DeleteAppDataCheckboxState` 保持 0，所以它什么都不删（§5 的 V-3 实测） |
| I-10 | 看 `%LOCALAPPDATA%\Local Console Hub` | 目录与其中的程序文件都没了；开始菜单项没了；「应用和功能」里没有残留项 |
| I-11 | 重看 I-1 的两个数据目录 | **逐字节一致**——卸载没有动用户的配置与运行历史（验收项「卸载不会意外销毁用户日志 / 配置」）。这条是 §2.1 那条不变量的可观察面：如果安装目录压在数据目录上，这里就会看到 `config.yaml` 消失 |
| I-12 | 应用开着时卸载 | 卸载器的进程检查会先结束正在运行的应用，再删程序文件，卸载仍然完整收尾（实测：应用被结束、安装目录与注册表项都清掉、脚本没有卡住）。交互式卸载时这一步是弹窗提示，不是静默结束——**未实测**，归人眼 |

§4.2 的十一条都走静默路径，而**复选框只在图形界面里存在**：它由确认页在运行时创建
（`un.ConfirmShow` 的 `CreateWindowEx`），静默 `/S` 根本到不了那一页，`$DeleteAppDataCheckboxState`
也就永远是 0。要验「勾选之后发生什么」，只能把真实的确认页叫起来、把控件勾上再让它跑完；
怎么做的、结果如何，见 §5 的 V-1–V-4。外观（字号、与上一行说明文字的对齐）也一并截了图，
但换 DPI 或换主题仍归人眼。

### 4.3 升级

| # | 步骤 | 期望 |
| --- | --- | --- |
| I-13 | 在已安装的版本上再跑一次安装包（同版本覆盖即可模拟） | 装成功，安装目录里仍然只有一份程序，不出现 `Local Console Hub (1)` 之类的第二份 |
| I-14 | 覆盖安装后看数据目录 | 与前一次**逐字节一致**；`config.yaml` 与 `logs\` 的原有运行历史都在（验收项「升级不会静默抹掉配置 / 历史」） |
| I-15 | 覆盖安装后启动应用 | 侧栏里的会话与上次一样；新 run 落在既有 `logs\<session_id>\<年-月>\` 里，旧 run 的文件还在 |

> 真正的跨版本升级（0.1.0 → 0.1.1）在本仓库还没有第二个版本可试。**覆盖自己**验的是同一条
> 机制：升级路径就是「同键的新安装包装到同一个位置」，用户数据不参与其中。等 v0.1.1 存在时
> 这一条应当用真版本补跑一次。

### 4.4 安装包上的 MVP 冒烟

| # | 步骤 | 期望 |
| --- | --- | --- |
| I-16 | 把 `fixtures/verification-config.yaml` 放到 `%APPDATA%\LocalConsoleHub\config.yaml`，启动**安装版**的 exe | 侧栏列出 fixture 的会话；能起一个 PowerShell 会话并敲命令（验收项「发布产物通过 MVP 冒烟」） |
| I-17 | 完整跑一遍 `docs/VERIFICATION.md` §4 | 安装版与开发版行为一致 |

I-16 / I-17 中「窗口里能敲命令」这一半只能在桌面上做：agent 会话既点不到原生窗口，
也截不到它（`VERIFICATION.md` §5 末节）。**第 1 层证据说明的是同一份 commit 的构建**，
不是安装后的那份二进制；两者差别只有打包与 `dist/` 的嵌入方式，但差别是一个事实，
不能拿前者冒充后者的证据。

---

## 5. 运行记录

每次完整跑一遍 §4.1–§4.3 追加一段，不要覆盖。格式照 `VERIFICATION.md` §6。

### 2026-09-29 — T12 首次打包与安装验收

环境：Windows 11 Pro（10.0.26200），主检出（非 worktree）。二进制由本分支
`feat/t12-windows-packaging` 的 `1b29e89` 打出（merge-base `main` @ `25e9e03`）。
Rust 1.98.1 / Node 25.2.1；WiX 3.14 与 NSIS 用的是既有缓存（`%LOCALAPPDATA%\tauri`）。

§4.1–§4.3 这四步**跑了两遍**：一遍是首次打包（`d77d6fa`），一遍是 review 修复之后的
`1b29e89`；两遍的输出逐字相同。之所以要跑第二遍，是因为修复动了 `config/mod.rs` 与
`lib.rs`——虽然只是 `#[cfg(test)]` 下的东西，但「只是测试代码」是要验的说法，不是假设。

**构建（§3）。** `npm run tauri build` 退出码 0，release profile 编译 2m50s，两个包都出：

```text
bundle\msi\Local Console Hub_0.1.0_x64_en-US.msi     4.32 MiB
bundle\nsis\Local Console Hub_0.1.0_x64-setup.exe    3.27 MiB
```

**§4.1–§4.3。** 逐条结果：

| # | 结果 |
| --- | --- |
| I-1 | 哨兵：roaming 1 个文件（`config.yaml`，2740 B）；local 28 个文件（`logs\` 与 `metadata\` 的历史 run，外加一个 `external` 日志）。两份清单按路径 + 字节数排序后取哈希留档 |
| I-2 | `…-setup.exe /S` 退出码 0，无 UAC |
| I-3 | 安装目录只有 `local-console-hub.exe`（11 192 320 B）与 `uninstall.exe`（79 331 B） |
| I-4 | 启动后 10 s 仍存活；`MainWindowTitle = Local Console Hub`、`Responding = True`、工作集 57.5 MB；只有一个进程 |
| I-5 | 两个数据目录与 I-1 **逐字节一致**（`diff` 无输出） |
| I-6 | 安装后的 exe：产品名 `Local Console Hub`、文件版本 `0.1.0`、公司 `Local Console Hub contributors`、版权字符是 `U+00A9` |
| I-7 / I-8 | 未跑，见下 |
| I-9 | `/S` 卸载退出码 0 |
| I-10 | 安装目录、开始菜单快捷方式、`HKCU\…\Uninstall\Local Console Hub` 三样都已消失 |
| I-11 | 两个数据目录仍与 I-1 **逐字节一致** |
| I-12 | 应用开着时静默卸载：应用被结束，安装目录与注册表项都被清掉，脚本没有卡住或留下半装状态 |
| I-13 | 再装一次（同版本覆盖）：`InstallLocation` 不变，`%LOCALAPPDATA%` 下只有一个 `Local Console Hub`，目录里仍是那 2 个文件 |
| I-14 | 覆盖安装后两个数据目录仍与 I-1 **逐字节一致** |
| I-15 | 未跑，见下 |

整轮跑完机器回到起点：安装目录、快捷方式、注册表项都不存在，没有残留进程，
用户数据与开始时逐字节相同（`diff` 无输出）。

**没跑的四条，说清楚为什么。**

- **I-7（图标外观）**：需要人眼看，agent 会话截不到原生窗口与快捷方式
  （`VERIFICATION.md` §5 末节）。`icon: ,0` 指向 exe 内嵌资源，这一条只到「资源接上了」。
- **I-8（MSI）**：per-machine 安装要管理员，本会话没有提权通道。MSI 产物是构建出来的
  （4.32 MiB），但没有实际装过——所以「MSI 装到 `C:\Program Files\Local Console Hub`」
  这一句来自配置与 WiX 产物，不是本机实测。
- **I-15、I-16、I-17 的窗口那半**：同上，都是「在人面前点一下」。安装版与开发版共用同一份
  `dist/` 与后端代码，但那是推理不是证据；验收项「发布产物通过 MVP 冒烟」因此**只完成了一半**。

**这一轮新发现的一件事**：`%LOCALAPPDATA%\com.localconsolehub.hub\EBWebView\` 是 WebView2
的用户数据目录，实测 **126 MB / 612 个文件**；卸载器那个「删除应用数据」复选框删的是它，
不是 `LocalConsoleHub`。方向落在安全的一边（用户的配置与日志删不掉），但复选框的承诺与
实际删除的位置不一致。已记入 §2.2，并开
[#47](https://github.com/q956085398-netizen/Local-Console-Hub/issues/47) 跟踪——
修它要选一条路（写 `installerHooks`、或把 WebView2 的落盘位置并进 `AppPaths` 的 cache），
不是 T12 里顺手能定的。

### 2026-09-29 — #47：让卸载器的复选框说到做到

环境：Windows 11 Pro（10.0.26200），主检出（非 worktree），分支 `fix/47-uninstaller-checkbox`。
WiX 3.14 与 NSIS 用既有缓存。这一轮只验一件事：确认页上那个复选框**说的**和它**做的**
是不是同一件事，以及改动有没有碰到用户的配置与日志。

**构建（§3）。** `npm run tauri build -- --bundles nsis` 退出码 0，产物
`Local Console Hub_0.1.0_x64-setup.exe`（3 426 561 B）。

- 第一次打包**失败**，原因值得记下来。`makensis` 报
  `Invalid command: ";"`，`!include: error in script: "...\English.nsh" on line 1`：
  `customLanguageFiles` 指的文件被 bundler 抄进 `target/release/nsis/x64/` 时会自己写一个
  UTF-8 BOM，而仓库里那份副本也带着一个，两个 BOM 叠在行首，`makensis` 把第二个当成了命令。
  仓库里那份从此**不带 BOM**（写在这份文件的头注释里）。
- 另一件事是给 §2.2 的维护说明找依据：把 `English.nsh` 里的 `LangString deleteAppData`
  注释掉再单独编译生成的 `installer.nsi`，`makensis` **不报错**，只打一条
  `warning: 6040: LangString "deleteAppData" is not set in language table of language English`，
  退出码 0。缺字符串因此是**静默**的，而 CI 只做 debug `cargo build`、不跑打包——这就是
  `cargo test` 里那条字符串计数守卫存在的理由。

- 第三件是 `tauri.conf.json` 里 `languages: ["English"]` 到底是不是必需的。Tauri 的 schema
  写着 `customLanguageFiles` 的 key「必须同时加进 `languages` 数组」，而实测**不是**：
  把 `languages` 整条删掉、再把我们那份文件的文案改成 `EXPERIMENT MARKER WITHOUT LANGUAGES`
  重新打包，构建仍退出 0，生成的 `target/release/nsis/x64/English.nsh` 里就是 marker 那一句。
  所以 2.12.0 会无条件挂载这份文件，`languages` 是在满足**写下来的契约**而不是一个量出来的
  依赖；它留着，§2.2 也照这个说法写。

**复选框的文案。** 装完之后从外部读那个控件：`un.ConfirmShow` 在运行时用
`CreateWindowEx(..., w "$(deleteAppData)", ...)` 创建它，所以只有把真实对话框叫起来才看得到。
读法：`EnumWindows` + `EnumChildWindows` 取 `Button` 的 `GetWindowTextW`
（UIAutomation 这条路读不到这个控件，只返回空名），顺带把对话框本身截图留档。结果：

| # | 结果 |
| --- | --- |
| V-1 | 确认页上的复选框文案 = `Delete WebView2 browser profile (not your config or logs)` |
| V-2 | 首次显示时 `BM_GETCHECK` = 0——**未勾选**。这一点是量出来的：截图在那个 DPI/主题下看着像勾上了，读状态才知道不是 |
| V-3 | 静默 `/S` 卸载（等价于不勾选）：`%LOCALAPPDATA%\com.localconsolehub.hub` 原封不动，163 个文件 / 10.0 MiB |
| V-4 | 勾选后卸载：该目录**整个消失**（消失前 163 个文件 / 10.0 MiB），安装目录与卸载注册表项同时清掉 |
| V-5 | 全程 `%APPDATA%\LocalConsoleHub`（2 个文件）与 `%LOCALAPPDATA%\LocalConsoleHub`（28 个文件）按路径 + 字节数排序后取哈希，**逐字节不变** |
| V-6 | 重新安装并启动：该目录被 WebView2 自己重建；最后一次静默卸载后两个数据目录仍与开始时逐字节一致 |

勾选那一步是脚本驱动的：找到复选框、`BM_SETCHECK` 置 1、再对 `&Uninstall` 按钮 `BM_CLICK`。
静默 `/S` 不走 `un.ConfirmLeave`，勾选态没有别的办法验。

**守卫。** `src-tauri/src/release.rs` 新增三条，`cargo test` 即跑：接没接上我们自己的语言文件、
这份文件是否仍与上游同样多字符串、以及那句文案有没有重新变回 application data。
三条都做了反向验证，随后还原：把上游文案放回去 → 文案守卫红；从文件里删一条、加一条
`LangString` → 计数守卫红（26 与 28 各自报出）；把 `English` 从 `languages` 里去掉 → 接线守卫红。

**没跑 / 归人眼。** 复选框文字会不会在高 DPI 下被裁掉是**外观**问题：`GetWindowTextW`
拿到的永远是完整句子，量不出裁没裁。本机 100% DPI、浅色主题下截图看过一次，其余归人眼。
