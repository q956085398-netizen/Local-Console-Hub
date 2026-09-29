# 关键设计决策

本文档记录目前已经明确的产品级决策。后续如果修改，应说明原因，而不是悄悄改变行为。

## D-001：Windows 优先

**状态：Accepted**

早期版本优先解决 Windows 本地服务和终端管理问题。

原因：

- 当前目标场景主要来自 Windows；
- ConPTY、PowerShell、CMD、系统托盘与 Windows 进程树是核心；
- 先把一个平台做稳定，比一开始追求跨平台更重要。

后续可以抽象跨平台接口，但不影响 MVP 决策。

---

## D-002：受管会话，而不是全局终端劫持

**状态：Accepted**

Hub 只自动管理：

- 由 Hub 创建的终端；
- 通过 Hub 启动的服务；
- 用户明确添加到 Hub 的会话。

默认不拦截：

- 系统自己创建的控制台；
- 安装器；
- 第三方工具临时控制台；
- AI Agent 后台子进程。

原因：

- 更稳定；
- 更可预测；
- 降低误收和兼容性问题；
- 用户能够明确知道“为什么它出现在 Hub 中”。

---

## D-003：交互终端是一等公民

**状态：Accepted**

终端区域不能只是日志窗口。

对于 Interactive Terminal Session，必须支持真实输入和 PTY 语义。

原因：

- 一些测试服务、CLI、REPL 需要用户输入；
- 只有 stdout 查看能力无法替代实际终端；
- “统一终端窗口”的核心价值之一就是用户能继续正常操作。

---

## D-004：终端缓冲和持久化日志分离

**状态：Accepted**

看到终端输出不意味着一定写日志文件。

默认：

- 所有会话可以拥有内存缓冲；
- 普通交互式终端不自动生成持久日志；
- 持久化由日志策略决定；
- stdin 默认不记录。

原因：

- 减少无用日志；
- 减少磁盘写入；
- 降低敏感输入被持久化的风险；
- 让日志变得有序、可找到、可理解。

---

## D-005：应用已有日志时优先关联，不重复复制

**状态：Accepted**

如果应用自身已经维护权威日志文件，Hub 应优先建立索引和入口。

原因：

- 避免重复占用空间；
- 避免两份日志时间线不一致；
- 保留应用自身的轮转和格式。

---

## D-006：关闭主窗口默认进入托盘

**状态：Accepted**

点击主窗口关闭按钮：

- 隐藏到托盘；
- 不停止受管会话；
- 不退出 Hub 后台。

真正退出必须有明确入口。

原因：

- Hub 本质上是长期驻留的本地服务控制中心；
- 用户关闭窗口通常只是“不想占任务栏”，不是想停止服务。

---

## D-007：停止与强制结束分离

**状态：Accepted**

“Stop”优先尝试优雅结束。

“Force Kill”是单独的明确动作。

原因：

- 避免损坏数据；
- 避免中断应用收尾逻辑；
- 避免用户误以为普通停止就是直接 kill。

---

## D-008：状态不能只看 PID

**状态：Accepted**

长期服务需要区分：

- 进程存在；
- 端口可用；
- 健康检查通过；
- 应用忙碌；
- 应用异常。

MVP 可以先只有基础状态，但架构不能把 `PID exists == service healthy` 固化。

---

## D-009：低占用优先于持续高精度监控

**状态：Accepted**

资源状态、端口检查、健康检查应低频、事件驱动或按需。

Hub 不应该为了展示漂亮的实时曲线而持续占用显著 CPU。

---

## D-010：配置是用户可读的

**状态：Accepted**

会话配置应：

- 可人工阅读；
- 可人工修改；
- 能被版本控制；
- 不把核心配置只藏在二进制数据库里。

UI 可以提供配置编辑，但文本配置仍是重要能力。

---

## D-011：日志目录本身必须可理解

**状态：Accepted**

即使不打开 Hub，用户也应该能从文件系统找到并理解：

- 这是哪个 session；
- 哪一天；
- 哪一次运行；
- 哪个日志文件。

因此不采用全部日志堆在随机 UUID 目录的方案。

---

## D-012：AI Agent 后台终端默认不纳管

**状态：Accepted**

AI Agent、IDE、第三方工具自己创建的后台终端默认保持原样。

原因：

- 它们通常是工具内部实现细节；
- 强制显示会让 Hub 再次变得嘈杂；
- 生命周期通常应由原工具负责。

未来可以提供显式适配，但不做默认全局捕获。

---

## D-013：应用图标按 UI 视觉语言自绘

**状态：Accepted（2026-09-28，仓库所有者签认）**

T00 落地时仓库中不存在已批准的应用图标（`assets/` 仅含 V2 UI 参考图）。为实现「Approved application icon is used」验收项，按 `docs/UI_STYLE_GUIDE.md` §10 的视觉语言自绘了图标：深色近黑圆角方形 + 蓝色终端提示符 `>` + 浅色块状光标（`assets/brand/app-icon.svg`），并由 `scripts/generate-icon.mjs` + `npx tauri icon` 生成完整图标集。

原因：

- 图标是 T00（应用身份、任务栏图标）的硬性需求；
- UI 规范已冻结视觉语言，可推导出一致的图标方向；
- 不引入额外设计依赖即可保持可复现（SVG 源 + 脚本）。

后续如需更换：替换 `assets/brand/app-icon.svg` 源文件并重新执行 `node scripts/generate-icon.mjs && npx tauri icon assets/brand/app-icon.png` 即可，不影响其它层。

---

## D-014：PTY 后端直接实现 ConPTY，不引入 portable-pty

**状态：Accepted（2026-09-28，T02 落地时签认）**

MVP §2 允许 Windows PTY 实现以 portable-pty 起步。T02 评估后选择直接用
windows-sys 实现 ConPTY（`CreatePseudoConsole` + `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`
属性附加子进程），理由：

- portable-pty 在本项目的精确部署形态（Tauri 2 release 构建、Windows GUI 子系统）下有未决缺陷：wez/wezterm#6946 报告该形态下会弹出多余的 cmd 窗口，至今未修复；
- 其依赖树较重（anyhow、filedescriptor、serial2、nix、winreg、shared_library 等），且探测不到 ConPTY 时直接 `expect` panic；
- 仓库已依赖 windows-sys（T03），直接实现不新增任何 crate；创建标志、句柄生命周期与错误路径完全受控。

按 T02 的 blocker rule：只替换 PTY 后端，前端/会话契约不变；对外契约是本层自己的
`Pty` 抽象。若后续直接维护 ConPTY 的成本超过收益，可在同一契约下换回 portable-pty
（重开本决策并记录原因）。

---

## D-015：停止开始时就进入 Stopping，而不是等待之后

**状态：Accepted**

DEVELOPMENT §6 的顺序建议把「更新 UI 为 Stopping」放在请求优雅退出与等待超时之后。
T04 的 Session Core 改为在**发起停止时立刻**进入并广播 `Stopping`。

原因：

- 一次停止可能用满整个宽限时间（默认 5 s）。在这段时间里仍把会话显示为 `Running`，
  等于告诉用户一件不真实的事，也让第二次点击看起来像重复操作；
- `Stopping` 是「正在收尾」，不是「已经结束」，提前显示不会让用户误以为进程已经没了。

§6 真正要保证的部分没有改变，仍然按序执行：请求优雅退出 → 等待 → 升级为针对受管进程树的
强制结束，并且**只有在确认进程树消失之后**才进入 `Stopped` / `Exited`（由 T03 的 stop
barrier 保证）。

用户可见行为：按下 Stop 时状态点立即变为 Stopping，直到进程树确认消失才落到
Stopped / Exited。

---

## D-016：运行元数据复用 IPC 的 RunRecord 形状，时间统一 UTC

**状态：Accepted**

LOGGING.md §6 给了一份「建议」的运行元数据示例：snake_case 字段 + 本地时区偏移
（`+08:00`）。T05 落地时改为**直接复用前端已经在读的 `RunRecord` 形状**：camelCase 字段、
RFC 3339 UTC 时间戳、JSON 缩进写入 `metadata/<session_id>/<YYYY-MM>/`。

原因：

- 同一份运行记录只被解释一次：UI 的运行历史列表与磁盘上的文件是同一形状，不会出现
  「界面说 A、文件说 B」的分叉；
- UTC 让两台机器的记录可以相互比较，日志文件的生命周期长于写出它时的时区设置；
- snake_case + 本地偏移只对「人在本机用文本编辑器打开」更顺手，而这个场景由 §5 的目录结构
  与文件名（会话、年月、run_id）已经覆盖。

目录与文件归属仍遵守 D-011（人不开 Hub 也能看懂），因此元数据文件与它描述的日志文件同名
同层，只是扩展名不同。

用户可见行为：运行历史列表里的字段名与终端里看到的完全一致；时间显示为 UTC。

---

## D-017：V2 视觉 token 以上游原型源码为准，中性色交互面

**状态：Accepted（2026-09-28，T06 落地时签认）**

V2 参考图（`assets/ui/ui-v2-service.png`、`ui-v2-terminal.png`）产自设计工作区
`E:\Grok-UI-Design\LocalConsoleHub`（Tailwind v4 原型）。T06 落地时以该工程的构建
样式表与组件源码为 token 权威来源，**不采用**视觉模型对截图的目测估值。

由此冻结的关键视觉事实（与目测版本相反）：

- 主按钮是**中性灰** `#c5ccd6` + 深色文字，**不是蓝色**；蓝色不参与交互面，
  颜色只承担生命周期语义（延续 UI_STYLE_GUIDE §10）；
- 状态色为柔和色域：Running `#6fba8a`、Busy/Warn `#c9b07a`、Error `#d27878`、
  Idle `#6d717b`；窗口底色 `#0b0c0f`，终端 `#090a0d`；
- 字体为 IBM Plex Sans / IBM Plex Mono，实现用 `@fontsource/*` 本地打包（桌面应用
  不依赖字体 CDN）；
- 终端面板**没有** macOS 三点窗口按钮；Tabs 激活下划线为 1px 中性色，不是 2px 蓝线。

原因：

- 参考图是截图，含抗锯齿与缩放，目测取色必然有偏差，且容易「补全」图中不存在的
  元素（本次首轮转译即虚构了三点窗口按钮与蓝色主按钮）；
- 上游源码是同一设计的可执行真值，取到的 hex、间距、文案无需猜测；
- 该工程仅作为**设计来源**：其 auth / database / deployment / preview-runtime
  子系统不进入产品（UI_STYLE_GUIDE §12）。

用户可见行为：界面为中性灰交互 + 状态色语义的深色控制台；Token 与
`docs/DESIGN_SPEC_EXTRACTED.md` 一一对应，可直接复用。

运维：若参考图更新，先直接读图核对，再回到上游源码核对 token，最后更新
`DESIGN_SPEC_EXTRACTED.md`。

---

## D-021：日志的打开与清理由 Session Core 解析路径，前端只提交会话与 run

**状态：Accepted（2026-09-29，T10 落地时签认）**

`open_log_file` / `open_log_folder` / `preview_log_cleanup` / `cleanup_logs`
四个命令只接受 `session_id` 与可选 `run_id`，**不接受路径**。路径由
`SessionCore::log_file_target` 按会话自己的状态解析（external → 应用自有日志；
指定 run → 该 run 的元数据记录；否则当前运行的文件，或最近一次运行留下的文件），
再交给 `shell` 层用系统默认处理器打开。

原因：

- 若命令接受路径，就等于给 WebView 一个「打开任意文件」的能力；日志视图不需要
  这种通用文件启动器能力，而 IPC 面的形状是唯一能把它挡在外面的地方（放在函数体里
  做校验则可以绕过）；
- 「这个会话的日志是哪一份」只应有一处定义。UI 与后端各判一次，就会出现
  两个不同的答案；
- 删除是不可逆动作，因此拆成两步：`preview_log_cleanup` 先说会删掉几个文件、
  释放多少空间，`cleanup_logs` 才真的删（LOGGING.md §9 的「先规划后删除」）。

用户可见行为：日志页的打开/目录/复制/清理只作用于该会话真正拥有的文件；
`external` 会话的入口永远指向应用自己的日志；清理只作用于 Hub 自己的日志根目录，
不会碰应用自有日志，也不会碰其它会话的历史（D-005、D-011）。

运维：新增日志相关命令时沿用同一形状——命令给会话与 run 标识，Core 给路径；
破坏性动作一律先给计划再执行。

---

## D-018：交互终端由 Session Core 托管 PTY；「关闭会话」没有优雅阶梯

**状态：Accepted（2026-09-29，T07 落地时签认）**

一个 `type: terminal` 会话的运行体是 **PTY**（T02 层，`crate::pty`），不是受监督进程；
`start_spec` 按会话类型分派（service → T03 进程，terminal → PTY），两条运行路径共用
同一套生命周期状态机（同一个 `Run` 枚举对外只回答生命周期要问的那几件事：等待退出、
读退出码、停止、强制停止）。

关闭终端 = 关闭控制台：`Pty::kill()` 终止 shell 并**确认其已退出**后才返回，因此
`StopReport.graceful_delivered` 恒为 `false`——不存在可投递的优雅信号（ConPTY 上没有
`CTRL_BREAK` 的对应物），报告 `true` 会是在声称一个从未发生的礼让。这与 D-007 对进程
树的阶梯并不冲突：D-007 管的是受监督服务，终端会话的「优雅手势」是**输入**——用户按下的
Ctrl+C 走 `terminal_write` 的 `0x03` 字节，它中断正在运行的命令，**不关闭会话**（spec §7）。

shell 未能在终止后退出时，停止操作按失败上报、会话落到 `Error`，而不是假装停干净了。

用户可见行为：终端会话的「停止」立即结束 shell；Ctrl+C 只打断当前命令；两者在 UI 上
是两个不同动作（`停止` 按钮 vs 键盘中断）。

---

## D-019：终端输出以带字节区间的事件批次到达 UI；attach 强制切一刀

**状态：Accepted（2026-09-29，T07 落地时签认）**

终端输出通过 `terminal-output` 事件送到前端，每个批次携带：

- `generation`——它属于哪一次运行（重启会 +1）；
- `start` / `end`——它覆盖该运行字节流的哪一段（半开区间）；
- `data`——该段字节，**base64**（标准字母表、带填充）。

批次在会话内部由 relay 合并：够 `BATCH_MAX_BYTES`（64 KiB）或最早一个待发字节等满
`BATCH_WINDOW`（16 ms）就切一刀；终端读取线程每 16 ms 唤醒一次作为静默期的兜底，
服务进程走 watcher 的 100 ms tick。这是 spec §9/§14「不要每行一个事件、一个高输出会话
不得拖垮整个 UI」的落点：2000 行输出在测试中被压成远少于 200 个事件。

**偏移量的契约**：`attach_terminal` 返回保留的 scrollback、该 scrollback 覆盖到的字节偏移
（`emitted`）与 `generation`；视图重放 chunks，然后**整批追加** `end` 超过该偏移的批次、
整批丢弃 `end` 不超过它的批次——不切批次。这个「不切」之所以成立，是因为 attach 在**同一
次持锁**里把待发输出切出来发布：`emitted` 因此只会落在批次边界上。`emitted` 在字节**进入
scrollback 时**推进（不是发布时），所以任一次 attach 读到的偏移与它读到的 scrollback 永远
自洽。

字节用 base64 而不是文本：控制台的一次读取可能落在多字节字符中间，按 chunk 解码会把接缝
渲染成替换字符（`terminal-output` 与 `attach` 的 chunk 都因此携带字节）。前端把字节交给
xterm.js，由它以流式 UTF-8 解码，跨 write 的多字节字符不受影响。

偏移量只数**当前这次运行**的字节：scrollback 是会话的（`docs/LOGGING.md` §8，重启不会
清空），所以重启后 attach 重放的内容会比 `emitted` 更长——这是有意的，重放覆盖了上一轮
的输出，而新运行的批次从 0 开始追加，仍然不丢不重。

用户可见行为：切换会话、切到「日志/详情」页签、隐藏窗口后回来，终端既不重复也不丢失已经
显示过的输出；被丢弃（超出上限）的历史由 `buffer.droppedBytes` 明示。

「有界」有两层含义，这里把两层都定下来：**内存**由结构保证——PTY 输出队列上限
256 KiB（T02）、会话 scrollback 上限（LOGGING §8）与单批上限 64 KiB 共同封顶，任何
进程都无法把 Hub 的内存推高；**事件速率**由 relay 的窗口封顶——每会话每 16 ms 至多一批，
与当前有没有视图挂载无关。第二层刻意**不**做「按需发布」：字节无论如何都必须进
scrollback（否则切换会话或隐藏窗口回来后看不到），省下的只是序列化与 IPC；如果 T11 的
性能收尾认为这笔开销值得省，再按「有无视图挂载」门控，而不是现在引入一份「谁在看」的
状态。

---

## D-020：会话在 UI 里是 config + snapshot 两半；`list_session_configs` 补上 config

**状态：Accepted（2026-09-29，T07 落地时签认）**

窗口渲染一个会话需要两半：它**是什么**（名称、类型、用途、关闭影响、端口、cwd、shell ——
即校验后的 config）和它**在做什么**（状态、PID、运行时长、缓冲 —— 即 `SessionRuntime`
snapshot）。T04 起 IPC 只有后者；T07 增加 `list_session_configs`，从**同一注册表**回答前者，
因此窗口列出的一行一定是 Session Core 能 start/stop/attach 的会话。

后端可用时窗口就以这两半渲染真实会话，后端不可用（浏览器预览、首次列表返回之前）才退回
T06 的 fixture 工作区——两种来源是同一个 `SessionView` 形状，不是两套模型。

实时会话没有分组（配置 schema 没有 `group` 字段，也没有归属工单），因此统一挂在
`Configured`（hint `config.yaml`）一个分组下：如实说明来源，而不是发明一套配置并不具备的
分类。fixture 工作区仍使用参考图里的三个分组，用于与参考图比对。

用户可见行为：应用启动即显示 `config.yaml` 中的真实会话；终端的启动/停止/重启作用于真实
会话；预览模式下的动作只提示不执行。

---

## D-022：保留策略只删日志文件，运行记录保留并在载荷上标注文件是否还在

**状态：Accepted（2026-09-29，T10 复查时签认）**

保留策略清扫 `logs/<session>/<YYYY-MM>/*.log`，而运行元数据在
`metadata/<session>/<YYYY-MM>/`（`LogRoots`）。两者此前不对账：一次清理之后
`get_run_history` 仍会返回日志文件刚被删掉的运行，Logs 页于是渲染出「点了就失败」的
文件操作（`shell::open_path` 回「路径不存在」）。

决定：

1. **不删除运行记录。** 记录是「这次运行发生过」的证据；因为日志过期而删掉它，等于拿历史
   换整洁（§6 把「UI 历史运行列表」列为元数据的用途）。清理不会进入 `metadata/` 根目录。
2. **在读取路径上标注文件是否还在。** 运行历史的每条现在是
   `RunHistoryEntry { …RunRecord 字段平铺…, log_file_present: bool }`（camelCase 上线为
   `logFilePresent`），前端据此决定是否给出「打开日志」。落盘文档仍是 D-016 冻结的
   `RunRecord`，不受影响；在磁盘上手动删掉文件也走同一条路径——按当前事实回答，不追溯是谁删的。
3. **不引入 tombstone 或清理账本。** 那要么改元数据形状（违反 D-016），要么另立一份需要
   自己保持一致的状态；而真相已经在文件系统上，读的时候问一次就够。

用户可见行为：清理后运行历史一行不少；文件不在磁盘上的那几行显示「日志文件不在磁盘上
（运行记录保留）」，仍可打开所在目录，但不再提供「打开日志/复制路径」。清理确认文案同时
说明「运行历史记录不会被删除」。

措辞只说事实、不说原因（「不在磁盘上」而非「已被清理」）：读取路径分不清「被保留策略清掉了」
与「`external` 的应用还没写出那个文件」，而替用户断定一个自己没有观察到的原因，是这个页面
一直在避免的编造。

---

## D-023：健康读数是快照上的独立字段，随 run 的 watcher 低频轮询

**状态：Accepted（2026-09-29，T08 落地时签认）**

spec §12 要求健康检查「低频、随会话生命周期可取消、非阻塞、与生命周期真相分离」。
落地点：

1. **读数与生命周期状态并列，永不参与状态机。** `SessionRuntime` 增加
   `health: Option<ServiceHealth>`，读数是 `{ processAlive, portOpen }` 两个**事实**，
   不是结论。`SessionStatus` 不会因为端口是否监听而改变——进程活着但端口未监听的
   服务仍然是 `Running`，UI 必须能同时表达这两件事（PRODUCT_SPEC §3；
   D-008 禁止把 `PID exists == healthy` 固化）。
2. **轮询挂在 run 自己的 watcher 线程上，不新开线程。** `watch_run` 本来每
   `WATCH_TICK`(100 ms) 醒一次；每 `POLL_INTERVAL`(5 s) 顺带做一次探测，run 结束
   watcher 就返回，探测随之停止。「随会话生命周期可取消」因此不是一套额外的取消
   机制，而是复用已存在的线程（spec §14 禁止无谓的每会话监控线程）。读数**只在变化时
   发布**：稳定运行的服务每个周期一次 loopback 连接、零事件。
3. **只探测配置了 `port` 的 service。** 没有 `port` 的 service 没有可回答的就绪问题
   （进程是否活着已经由 `Running` 回答）；terminal 的 schema 里根本没有 `port`。
   这类会话 `health` 保持 `None`——「没有检查」与「检查了、没有监听」是两个断言，
   UI 在前者不显示任何东西，而不是显示「未监听」。
4. **探测目标是 `127.0.0.1`，不解析主机名、不出机器。** 配置的 `url` 指向其它主机时，
   读数说的是**本机**端口。对一个只管理本机服务的控制面，这是诚实答案（PRODUCT_SPEC §1）。
5. **`busy` 仍然不产生。** §4 允许 `busy: unknown`。端口说明服务*可达*，不说明它*空闲*；
   能回答后者的只有 app-specific adapter，而它在 T08 的 out-of-scope 里
   （`docs/EXECUTION_PLAN.md`）。UI 保持不做声明，而不是找个近似值顶替。
6. **`ready` 不是视图模型上的字段，每次渲染从快照派生**（`derivations.isReady`）。
   视图上带一份副本会在 `session-state-changed` 到达时过期——列表时刻算出的值不会跟着
   新的健康读数更新，而这类 bug 只有等真实服务起来才会暴露。

用户可见行为：运行中的服务在端口开始监听后，头部徽标由 `Running` 变为 `Ready`；
「详情」页多一行 `健康`（`监听中` / `未监听` / `未监听 · 本会话进程已退出`），
没有读数的会话不显示这一行。行内不重复端口号——它已经在头部的元数据行里
（UI_STYLE_GUIDE §6）。端口未监听不会被显示成停止；端口在监听但本会话进程已经
退出时也不会显示 `Ready`（那是别的进程占着这个端口），只在详情里如实说明这两个事实。

运维：§12 提到的第三项检查（可选 HTTP GET）在 `ServiceHealth` 上加一个字段、在
`ServiceHealth::read` 里加一次探测即可；轮询、取消与去重都不用改。

---

## D-024：托盘是 Session Core 的读者；退出只做「已确认的停止」

**状态：Accepted（2026-09-29，T09 落地时签认）**

§11 与 UI_STYLE_GUIDE §9 要求托盘常驻、紧凑、能快速回到窗口、也能真正退出。落地点：

1. **托盘不拥有任何生命周期判断。** 菜单的每一行都从 `SessionCore::configs()` 与
   `snapshots()` 派生（`tray/model.rs` 是纯函数），每个动作都调用窗口按钮调用的同一个
   API（spec §3）。托盘因此不可能与窗口给出两种说法；「运行 / 失败」的**计数规则**也只有
   一份，在 `AppSummary::of`（`session/event.rs`），`SessionCore::summary()` 与托盘
   都读它，而不是各写一遍同一张 match。
2. **摘要数「运行 / 失败」，不数 `busy`。** §11 的示例写的是 `3 running (1 busy)`，
   但 MVP 没有 `busy` 的来源（D-023 第 5 条：端口说明服务*可达*，不说明它*空闲*）。
   托盘能诚实回答的是「有几个运行、几个失败」，就只显示这两个。
3. **失败 = `Error`，不是「退出码非 0」。** 判据沿用 Session Core 已有的划分（`watch_run`：
   自行结束且退出码非 0 落 `Error`，干净结束落 `Exited`），托盘不再从退出码里发明第二套定义。
   「重启失败的会话」因此只重启 `Error`；一个干净退出的会话不是需要修的问题。
4. **退出的顺序是「先说挡路的、再问、再停、再查、最后退」。** 有会话处于 `Starting`/
   `Stopping` 时退出**根本不问**——状态机不允许打断这两个状态（spec §5），问了就是一个
   Hub 兑现不了的承诺；它改为说明在等什么。有 `Running` 会话时问「停止它们并退出？」
   （`MB_OKCANCEL` 且默认按钮是 Cancel）。确认后**重新读一次注册表**再停——问一个问题
   要花用户愿意花的时间，而启动一个服务只要一次点击；停完再查一次，把此刻仍处于
   `Starting`/`Stopping`、因而根本无法停止的会话一并算作失败，**任一未停住就取消退出
   并列出名字**：进程树在本进程的 job object 里，带着它退出等于 §11 禁止的静默 kill。
   `docs/PRODUCT_SPEC.md` §8 的「保留可分离后台进程」不在 MVP 里提供——它需要一个尚不存在的
   安全交接方式，§11 明确禁止假装有。
   这条仍是尽力而为而非形式化保证：最后一次检查与 `app.exit` 之间那道几微秒的缝，需要
   Session Core 提供「退出中拒绝启动」才能彻底关掉，那是另一张工单的范围。
5. **退出路径不调用 `force_stop`。** `stop` 自己已经在宽限期后升级到强制路径（D-007、
   D-015），再补一次强制就是把用户没同意的动作塞进他同意的动作里。停不住就如实说停不住，
   由用户在窗口里显式选择强制结束。
6. **批量停止是并发的。** 一次 `stop` 要等自己的宽限期；串行停三个服务会把「停止全部」
   花掉三个宽限期。Session Core 本来就按会话加锁、从不跨生命周期操作持有注册表锁，
   并发安全是既有设计，不是为新功能加的。
7. **托盘重建由事件驱动，且只在读数变化时重建。** `TraySink` 只对 `session-state-changed`
   与 `app-summary-changed` 反应；`run-record-updated`、`terminal-output` 与托盘无关。
   窗口隐藏时事件照常到达（spec §9），这个过滤就是 §14「隐藏到托盘后接近空闲」的落点。
   它缓存上一次渲染的模型，因此一次状态变化带来的两个事件只重建一次菜单。
8. **图标沿用应用身份，不新造资产；缺图标则不启动。** 托盘图标取
   `default_window_icon()`——与窗口、任务栏同一个图标集（D-013），不是一个可能与它漂移的
   第二份资源。它是构建期从 `bundle.icon` 嵌进来的，取不到就意味着构建有问题；此时
   `tray::install` 返回错误、`setup` 随之失败，应用不启动。理由不是洁癖：关闭窗口会隐藏
   窗口，而托盘是回到隐藏窗口的唯一入口，一个没有图标的托盘在 Windows 上就是用户找不到的
   条目——继续启动等于做一个点一下 ✕ 就再也回不来的应用。
9. **会话行可点击，走一条只读请求。** 点某一行显示主窗口并选中该会话，通过
   `session-focus-requested` 事件（载荷只有 `sessionId`）。它不携带任何生命周期主张，
   前端只做选中；这也是托盘的会话列表不是装饰的原因——「托盘不是主界面的缩小版」
   （§9）针对的是复制工作区，不是让列表无法操作。
   需要点名的是：spec §9 列了四个事件，`session-focus-requested` 是**第五个**。它不是
   Session Core 的事件——Core 仍然只发 §9 那四个，前端镜像的那份契约没有变——它是托盘
   发给窗口的一条视图请求，由 `tray/` 自己拥有。工单只要求「会话列表」，点击行为是这次
   唯一的越界，记在此处以便撤回。

用户可见行为：点窗口的 ✕，窗口隐藏、进程与受管会话继续运行；托盘悬停显示
`Local Console Hub — 2 运行 · 1 失败`；托盘菜单自上而下是摘要、运行/失败的会话行
（最多 8 行，超出显示「…还有 N 个会话」）、显示主窗口、重启失败的会话、停止全部、退出；
「重启失败的会话」「停止全部」在无事可做时是灰的，不是消失的；退出在有会话运行时先问
一句，没有会话时直接退出。

运维：新增托盘动作时，在 `tray/actions.rs` 给出「作用于哪些会话」的纯函数、在
`tray/menu.rs` 给出菜单项与启用条件，`tray/mod.rs` 里只留循环与 AppHandle。菜单项 id
就是动作词汇表，定义在 `menu::ids`。

---

## D-025：连接状态问的是「宿主有没有应答」；`unavailable` 不是终局

**状态：Accepted（2026-09-29，T11 #12 落地时签认，修 #29）**

T07 的验收条件写着「UI 不得用只读/假终端路径顶替」，而发版路径上它并不成立：ping 只问
一次，一次失败就永久停在前端 fixture 工作区与只读终端面板上，除重启外没有回头路。
落地点：

1. **这个问题属于前端状态层，不属于 React hook。** `src/state/backend-connection.ts`
   拥有唯一一个问题：`ping` 有没有被应答。三个读数保持原样（`pending` / `connected` /
   `unavailable`），fixture 工作区仍是「答案不是 connected」时渲染的东西。`ping` 以函数
   形式注入，和终端附着协议注入 backend 是同一手法——这正是它能在 node 测试环境里被驱动
   （无 DOM、无 Tauri 宿主）的原因，也让 hook 缩回一个适配器：它只提供真的
   `invoke("ping")` 和这个模块的状态。
2. **`unavailable` 不是终局。** 宿主可以迟到（慢构建、冷 WebView、扫描器占着文件）却
   仍然到来，所以无人应答时按**有界**退避重问，一旦成功就停止追问。退避序列
   `[500, 1000, 2000, 5000, 10000, 30000] ms` 以 30 s 为上限而不是递增台阶：
   完全没有宿主的浏览器预览因此收敛成一次安静的低频轮询，而不是热循环。
3. **失败被如实说成「没有宿主在应答」。** 它永远不被报成会话失败或终端失败——会话的
   生命期不受窗口能否连上后端影响（spec §9），模块因此不持有任何每会话状态
   （`src/state/README.md` 禁止前端拥有运行时模型）。返回到窗口的路径也不新开：
   状态翻成 `connected` 后走的就是首次列表返回时已经在跑的那条路。

用户可见行为：启动时丢掉的那次 ping 不再把窗口永久留在预览工作区；宿主随后起来时，
窗口自己切到真实会话，不需要重启应用；没有宿主的浏览器预览保持可用，并以低频静默
轮询等待一个真的宿主出现。

运维：退避长度写在模块顶部的 `RETRY_DELAYS_MS`，改它只影响等待手感，不影响判定；
`src/state/backend-connection.test.ts` 用注入的 ping 与假计时器覆盖「失败仍继续问」
「后来成功即停」「有界不退化成热循环」三件事。

---

## D-026：安装范围是「当前用户」，安装身份从此冻结；用户数据永远在安装目录之外

**状态：Accepted（2026-09-29，T12 #13 落地时签认）**

T12 要把 v0.1.0 打成可安装的包，于是有三个必须写下来的选择——它们都不是实现细节，
而是「用户装完之后还能不能拿回自己的东西」这个问题的一部分。

1. **NSIS 的 `installMode` 显式写成 `currentUser`。** 默认值本来就是它，但这里不靠默认：
   `currentUser` 把程序装进 `%LOCALAPPDATA%\Local Console Hub`，不需要管理员，
   而且让「卸载器只删自己的安装目录」这件事可以在任何一台机器上直接跑一遍验证。
   MSI 保持 per-machine（装进 `Program Files`，要管理员），两份产物装到两个范围，
   用户按需要选。**注意真正让用户数据活下来的不是安装范围，而是第 2 条那条名字不变量**——
   per-machine 的安装目录同样碰不到数据目录，这一点两条路径一样。
2. **安装目录名与数据目录名必须不同，且由测试守着。**
   `%LOCALAPPDATA%\Local Console Hub` 与 `%LOCALAPPDATA%\LocalConsoleHub` 今天只差空格。
   把产品名缩成 `LocalConsoleHub` 会让两者重合，卸载就会带走用户的 `config.yaml` 与
   `logs\`。`src/release.rs` 的
   `the_install_directory_can_never_be_the_app_data_directory` 在 `cargo test` 里拦这一下，
   而不是指望 review 里有人想起来。
3. **`identifier` 冻结，升级靠它。** Tauri 由 `com.localconsolehub.hub` 推导 WiX 的
   upgrade code 与 NSIS 的卸载注册表项：同键的新版本覆盖安装、用户数据不动，换键的版本
   是**另一个应用程序**，装上去只能与旧的并存。所以它不是可以随手改的命名空间，
   而是升级契约本身。`the_identifier_that_upgrades_are_keyed_on_is_frozen` 钉住它。
4. **不做自动更新。** T12 的 out of scope 写的是「unless already trivial」——它不 trivial：
   Tauri 的 updater 需要签名密钥、一个分发端点和 HTTPS 托管，还需要「用户数据在升级中
   完好吗」的独立验证。这一版的升级路径就是再跑一次新的安装包；用户数据在安装目录之外，
   覆盖安装天然不动它。

同时落地的还有发布元数据：`publisher` / `copyright` / `category` /
`shortDescription` / `longDescription` / `homepage` 写进 `bundle`，它们就是 Windows
属性页里「产品 / 公司 / 版本」那一栏的来源；`webviewInstallMode` 钉成
`downloadBootstrapper`（Windows 11 自带 WebView2，只有缺失时才需要联网，代价写在
`docs/RELEASE_NOTES_v0.1.0.md` 的已知限制里）。

用户可见行为：卸载与覆盖安装都不会动 `%APPDATA%\LocalConsoleHub` 与
`%LOCALAPPDATA%\LocalConsoleHub`；卸载留下的这两个目录是有意为之，要清干净得自己删；
两个安装包的区别只有安装范围与是否需要管理员。

一处例外值得知道：内嵌 WebView2 把用户数据目录放在 `%LOCALAPPDATA%\<identifier>`，
也就是 `com.localconsolehub.hub`——**不是** `LocalConsoleHub`（这是 Tauri 与 WebView2
的行为，不是本决策的结果）。它因此落在 NSIS 卸载器「删除应用数据」复选框的删除范围内，
而用户的配置与日志不在。方向安全但不直观，记在 `docs/RELEASE.md` §2.2 与 #47。

运维：发布相关的静态事实集中在 `src-tauri/src/release.rs`（test-only 模块，`cargo test`
即跑）：三份 manifest 的版本号一致、`productName` 与 `ipc::APP_NAME` 同字、
`identifier` 未变、两个安装目标仍在、WebView2 安装模式未变。改了其中任何一条，
测试先红，再改这里与 `docs/RELEASE.md`。

---

## D-027：`purpose` 与 `close_impact` 是两种会话类型共有的字段，不是服务的特权

**状态：Accepted（2026-09-29，#38 落地时签认）**

原 §4 的字段归属表把 `purpose` 与 `close_impact` 划给 `type: service`，验证器因此在终端上
**拒绝**这两个字段。但 V2 终端参考图 `assets/ui/ui-v2-terminal.png` 画的恰恰是一个带用途行
和「关闭影响」提示条的交互终端——而头部 callout 与详情页的「能不能关」卡片都不看会话类型，
只念 `close_impact`。于是同一个 `SessionView` 的两个来源不一致，且**真实窗口**那一侧才是
对不上参考图的那一侧（fixture 工作区对得上，是因为 `src/state/fixtures.ts` 在替终端编造
这两个字段）。

1. **这两个字段改为两种类型共有**，验证器不再在终端上拒绝它们：`config::validate` 的
   `reject_cross_type_fields` 只留下 `command` / `url` / `port` 属于服务专有，
   `shell` / `initial_command` 属于终端专有。
2. **没有为此新增任何行为。** 两者是纯文本展示字段，没有运行时含义：头部、详情页与搜索
   过滤（本来就匹配 `purpose`）对两种类型用的是同一段代码，所以改动只落在「配置能不能写」
   这一层。
3. **不给终端生成默认文案。** 更省事的做法是替终端编一句「仅结束本终端」，但 T11 的验收
   条件明确要求关闭影响是**会话自己的文本**而不是通用填充；写了才算写了，没写就渲染 `—`。

用户可见行为：`config.yaml` 里的交互终端现在可以写这两项，运行中的终端头部因此显示配置里
的用途与关闭影响，与 `assets/ui/ui-v2-terminal.png` 一致；不写的终端仍然渲染 `—`
（`fixtures/verification-config.yaml` 的 `term-manual` 就是这种）。

运维：`fixtures/verification-config.yaml` 的 `term-pwsh` 带上了参考图里的那两句，
`config::tests::verification_fixture_loads_cleanly_and_covers_the_matrix` 钉住它们，
使「终端头部对得上参考图」这条手工验收是视觉比对而不是照着图重新打一遍字；
`tests/mvp_matrix.rs` 从配置文件一路断言到 Session Core 的配置。

---

## 如何修改这些决策

如果实现阶段发现某条决策需要改变：

1. 在 PR / Issue 中说明原决策；
2. 给出遇到的具体问题；
3. 说明替代方案；
4. 说明用户可见行为如何变化；
5. 更新本文档和相关规范。

设计决策可以变化，但变化必须可追踪。

编号：新增决策取 `docs/DECISIONS.md` 与所有**未合并分支**上已用编号的最大值 +1。
两条分支同时从同一点分出时，先合并者保持原号，后合并者让号——日志路径这条就因此
从 D-018 让到了 D-021（T07 先占用了 D-018–D-020）。
