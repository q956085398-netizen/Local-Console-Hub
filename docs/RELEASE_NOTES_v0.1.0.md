# Local Console Hub v0.1.0 — 发布说明

> **属于：** T12（#13）。**状态：** release candidate；自动化结果按日期和基线记录，原生窗口与安装包验收及 MVP 复核修复仍有未完成项，见 §7。
> 面向使用者：这一版有什么、怎么装、数据放在哪、什么是**已知**的、什么不在这版里。
> 打包与安装的验收过程、以及每一台机器上的实际结果，见 [`RELEASE.md`](RELEASE.md)。

---

## 1. 这一版是什么

一个 Windows 优先的本地控制台：把 SillyTavern / ComfyUI / 测试 API 这类**长期运行的本地服务**，
和平时散落在任务栏里的**交互式终端**，收进同一个窗口。关掉主窗口只是收进托盘，
受管会话继续跑。

技术栈是 Tauri 2 + React + TypeScript + Vite，后端 Rust（`docs/MVP_IMPLEMENTATION_SPEC.md` §2）。
这一版对应 `docs/ROADMAP.md` 的 Phase 1 与 Phase 2，外加 Phase 3 的一小块（端口与基础健康信号）。

## 2. 怎么装

两个包，同一个版本、同一套图标，区别只在安装范围：

| 包 | 装到哪 | 管理员 |
| --- | --- | --- |
| `…-setup.exe`（NSIS） | `%LOCALAPPDATA%\Local Console Hub`，**只给当前用户** | 不需要 |
| `….msi`（WiX） | `C:\Program Files\Local Console Hub`，所有用户 | 需要 |

装之前需要 **WebView2 Runtime**（Windows 11 自带）。安装包用的是「检测不到就联网下载」的模式，
所以在一台没有 WebView2 又**不能联网**的机器上安装会失败——先离线装好
[WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) 再装 Hub 即可。

装完第一次启动，应用还没有任何会话——会话来自配置文件，见 §4。

## 3. 这一版能做什么

**会话**

- 左侧一栏受管会话，两种类型：Service 与 Interactive Terminal；
- 启动 / 停止 / 重启 / 强制结束；
- 打开网页、打开工作目录；
- 头部显示 PID、运行时长、cwd、端口 / URL、当前日志策略；
- 运行中的会话在被选中时显示「关闭影响」原文，破坏性动作之前就能看到。

**交互式终端**

- ConPTY 上的真终端：可以输入、Ctrl+C、改窗口尺寸；
- 中文、变音符号、emoji 原样进出；
- 切换会话 / 切到 Logs / 把窗口收进托盘都**不会**结束终端，回来时输出仍在。

**服务**

- stdout / stderr 可见，带流标记；
- 配置的端口与 URL 可见；端口开始监听后状态徽标从 `Running` 变 `Ready`；
- 重启先等旧进程退出，不会再有两个进程抢同一个端口；
- 停止是「优雅停止 → 超时 → 强制结束」的阶梯，强制结束只作用于该会话自己的进程树。

**日志**

- 模式 `off` / `always` / `on_error` / `manual` / `auto`，来源 `none` / `captured` / `external`；
- 普通交互终端默认**不**落盘；`external` 只建立入口，不复制应用自己的日志；
- 每次运行一个 `run_id`，按 `logs\<session_id>\<年-月>\` 存放，文件名里带时间戳与 run；
- 运行历史、打开日志 / 打开目录 / 复制路径、两步确认的清理；
- stdin 不落盘。

**托盘**

- 主窗口的 ✕ = 收进托盘，受管会话不停；
- 托盘菜单：摘要、运行中与失败的会话（点一下直接切过去）、显示主窗口、重启失败的会话、
  停止全部、退出；有会话在跑时「退出」会先确认。

## 4. 用户数据在哪

**安装目录里没有用户数据**——卸载与升级都不会碰下面这些目录：

| 内容 | 位置 |
| --- | --- |
| 配置 | `%APPDATA%\LocalConsoleHub\config.yaml` |
| 日志 | `%LOCALAPPDATA%\LocalConsoleHub\logs\<session_id>\<年-月>\` |
| 运行元数据 | `%LOCALAPPDATA%\LocalConsoleHub\metadata\<session_id>\<年-月>\` |
| 缓存 | `%LOCALAPPDATA%\LocalConsoleHub\cache\` |
| WebView2 用户数据（约 126 MB，可删，删了只是下次启动慢一点） | `%LOCALAPPDATA%\com.localconsolehub.hub\EBWebView\` |

最后一行是内嵌浏览器（WebView2）自己的配置文件目录，不属于 Hub 的日志或配置。

配置是给人读的 YAML（D-010），这一版**没有**内置编辑器：直接编辑文件，重启应用即可。
可参照的完整示例是仓库里的 `fixtures/verification-config.yaml`——它在仓库里，
**没有**随安装包分发。

## 5. 升级与卸载

**升级**：跑新版本的安装包，装到同一个位置。用户的配置、日志与运行历史**原样保留**，
新 run 落在既有的目录结构里。跨版本升级靠的是同一个安装标识（`identifier`），
所以这一版之后不要改动它。

**卸载**：从「应用和功能」卸载。程序文件与开始菜单项会被删除，
**`%APPDATA%\LocalConsoleHub` 与 `%LOCALAPPDATA%\LocalConsoleHub` 会留下**——
这是刻意的：想彻底删干净，自己删这两个目录（§4 表里那两行）。卸载前记得先关掉应用。

卸载界面上那个复选框要单独说一句。它现在写的是 **Delete WebView2 browser profile (not your
config or logs)**，删掉的就是内嵌浏览器（WebView2）自己的配置文件目录
`%LOCALAPPDATA%\com.localconsolehub.hub\`（约 126 MB 的浏览器缓存），**不是**你的配置与日志。
所以勾不勾选，`config.yaml` 与 `logs\` 都会留着，差别只是那 126 MB 的浏览器数据还在不在
（#47：这一版之前它写着「删除应用数据」，而它其实做不到，那句话已经改掉）。

这一版**没有**自动更新：新版本就是再跑一次新的安装包。

## 6. 已知限制

按「会不会影响你」排序。

1. **安装包没有代码签名。** Windows 会在第一次运行时弹 SmartScreen「未知发布者」，
   要点「更多信息 → 仍要运行」才能继续。这是没有代码签名证书的结果，不是安装包坏了；
   它也不会影响安装后的运行。下一版拿到证书后这一条才会消失。
2. **只有 Windows。** 跨平台在 `docs/ROADMAP.md` 的 Phase 6，不在这一版（D-001）。
3. **交互终端的原生视觉验收仍待完成。** `purpose` / `close_impact` 已按 D-027 和
   [#38](https://github.com/q956085398-netizen/Local-Console-Hub/issues/38) 成为两种会话类型共有的可选配置字段；
   本地配置、载荷与自动化覆盖不代替在真实窗口核对 T-11。真实窗口的终端头部与
   `assets/ui/ui-v2-terminal.png` 的视觉结果尚未记录为通过，见 [`VERIFICATION.md`](VERIFICATION.md) §4 / §7。
4. **受管服务没有可输入的 stdin。** 服务会话的终端面板是只读缓冲，顶部写
   `PTY 未连接 · 只读缓冲`。需要输入的程序（REPL、要确认的命令）请配成 `type: terminal`。
5. **配置里的路径是字面量。** 不做环境变量展开，`~` 也不展开；相对路径相对**应用进程**的
   工作目录，不是配置所在目录。请一律写绝对路径，例如
   `cwd: D:\Tools\SillyTavern`。
6. **`Ready` 只代表端口在监听。** 它不表示服务真的健康——HTTP 健康检查、
   `Busy` / `Degraded` 这类更细的状态在 `docs/ROADMAP.md` 的 Phase 3（D-008）。
   URL / 端口不是生命周期真相：进程活着但端口没听，状态仍是 `Running`。
7. **日志有上限。** 单次运行 16 MiB——到顶后文件不再增长，并在文件里写明已截断；
   每个会话 256 MiB 或 30 天，两者先到者生效，但**最近一次运行始终保留**。
8. **只管理配置里写过的会话。** Hub 绝不扫描或收编系统上已经开着的终端、
   别的工具自己起的后台进程（D-002 / D-012）。列表里没有的会话，就是没配。
9. **卸载器不会替你删配置与日志。** 它的复选框删的是内嵌浏览器（WebView2）的配置文件目录
   `%LOCALAPPDATA%\com.localconsolehub.hub\`（约 126 MB 缓存），文案也照此写；两种选择下
   `config.yaml` 与 `logs\` 都会留着，想清干净得自己删那两个目录（§5）。
10. **原生窗口与安装包仍有未完成的人工验收。** 2026-09-29 的 Windows 桌面记录确认托盘
    R-1 至 R-8 通过（`VERIFICATION.md` §6）；终端 T-11 的真实窗口视觉确认仍待记录。
    安装版的 I-7 图标外观、I-8 MSI 安装、尚未执行的 I-15，以及 I-16 / I-17 中需要实际窗口或交互的部分仍未完成，
    详见 [`RELEASE.md`](RELEASE.md) §4–§5。

## 7. 这一版验过什么、还差什么

**验过**（证据见 `docs/VERIFICATION.md` §3 与 §6）：

- v0.1.0 release candidate 原始验证快照：Rust 318 条单元测试 + `tests/mvp_matrix.rs` 10 条集成测试 +
  前端 182 条。该数量属于这份版本说明引用的历史快照，不代表当前套件数量；另一次有日期和基线记录的
  T11 运行见 [`VERIFICATION.md`](VERIFICATION.md) §6（2026-09-29，Rust 322 条、前端 180 条）。
- 多会话压力冒烟：4 个受管会话同时跑，一个被灌到缓冲上限，停一个、重启一个；
- 与两张 V2 参考图的逐区域视觉比对，没有 MAJOR 级差异；
- 安装包：干净安装能启动、安装目录里没有用户数据、卸载与覆盖安装不动用户数据
  （NSIS 那份在本机实测；`docs/RELEASE.md` §5 有逐条结果与没跑的那几条的理由）。

**还差 / 尚未验收**：

- **原生终端视觉：** `purpose` / `close_impact` 配置能力已实现；真实窗口里的 T-11 尚未记录为通过。
- **安装版：** I-7 图标外观、I-8 MSI 实际安装仍待验；I-15 尚未执行；I-16 / I-17 中的窗口与交互部分仍待完成或确认。
  NSIS 的文件系统安装、卸载与覆盖安装证据不替代这些项目。
- **MVP 复核修复：** 截至 2026-09-29，#52–#55 仍是未完成的跟进工单；本发布说明不将这些修复写成已实现或已验证。

因此本版仍为 **release candidate**。历史托盘 R-1 至 R-8 通过记录继续有效；§6 第 9 条（#47）是已知但不拦发布的一条。

## 8. 出处

- 产品与行为：[`PRODUCT_SPEC.md`](PRODUCT_SPEC.md)、[`MVP_IMPLEMENTATION_SPEC.md`](MVP_IMPLEMENTATION_SPEC.md)
- 日志：[`LOGGING.md`](LOGGING.md) · 决策：[`DECISIONS.md`](DECISIONS.md)
- 验证：[`VERIFICATION.md`](VERIFICATION.md) · 打包与安装：[`RELEASE.md`](RELEASE.md)
- 后续计划：[`ROADMAP.md`](ROADMAP.md)
