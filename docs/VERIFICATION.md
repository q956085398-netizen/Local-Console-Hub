# 端到端验证与回归清单

> **属于：** T11（#12，发布门）。**取代：** 无。
> **配套：** `docs/MVP_IMPLEMENTATION_SPEC.md` §16 测试矩阵与 §17 完成定义说「要验什么」，
> `docs/DEVELOPMENT.md` §14 说「测试该覆盖哪些面」，本文档说「**怎么验、验过了没有**」。

改动碰到进程 / PTY / 日志 / 托盘 / UI 中任何一层时跑第 1 层；发版前（T12）三层全跑。
「验过了」的证据写在本文 §6，每次完整运行追加一段，不要覆盖。

---

## 1. 三层

| 层 | 是什么 | 什么时候跑 | 谁来跑 |
| --- | --- | --- | --- |
| **1 自动** | CI 的静态门 + Rust/前端单测 + 端到端集成套件 + 三个手工冒烟二进制 | 每次 push（CI 跑前三项）；改进程 / PTY / 多会话时补跑冒烟 | 任何人、任何 agent |
| **2 手工** | §4 的清单：真实窗口、真实托盘、真实点击 | 发版前；改动命中托盘或窗口生命期时 | 人（Windows 桌面） |
| **3 视觉** | 与 `assets/ui/` 两张参考图逐区域比对 | 改动命中 UI 时；发版前 | 人，或用 §5 的脚本截图后由 agent 比对 |

---

## 2. 命令

前端：

```bash
npm ci
npm run check
npm test
npm run lint
npm run format:check
npm run build
```

Rust（在 `src-tauri/` 下）：

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
```

三个手工冒烟二进制。**它们不进 CI**：会起真实进程、跑几十秒、`supervise_smoke` 还要人在
stdin 上敲一次回车。退出码 0 = 全部步骤通过，1 = 失败步骤在 stderr 上点名。

```bash
cargo run --example pty_smoke
```

```bash
cargo run --example supervise_smoke
```

```bash
cargo run --example multi_session_smoke
```

`multi_session_smoke` 是 T11 新增的多会话压力冒烟：4 个受管会话（3 个终端 + 1 个服务）
同时跑，其中一个被灌到超出 256 KiB / 5000 行的滚动缓冲上限，然后停一个、重启一个，
最后打印每会话的 pid / run / 缓冲 / 丢弃字节 / 落盘日志文件数。

单实例入口（#60）：同一入口连开两次、冷启动竞争、最小化与关闭后的恢复，测量进程数、
退出码、窗口可见性与有没有多带一个终端。**它不进 CI**：要起真实窗口，并且会拒绝在已有
Hub 运行时执行。

```bash
powershell -NoProfile -File scripts\verify-single-instance.ps1
```

结论与未运行项记在 [单实例入口验收](SINGLE_INSTANCE_ACCEPTANCE.md)。

视觉截图（§5）：

```bash
npm run build
```

```bash
npm install --no-save puppeteer-core
```

```bash
node scripts/capture-ui-states.mjs
```

图标四角（改图标源、或想知道产物是不是白底时）：

```bash
node scripts/check-icon-alpha.mjs
```

它用同一个浏览器把 `src-tauri/icons/` 里交付的 PNG / ICO 画进 canvas 再读像素，
四角 alpha 超出容差就退出码 1。这是**栅格产物的读数**，不是外观验收——任务栏、托盘、
快捷方式长什么样仍归 §4 的 W-6 与 I-7。

---

## 3. 第 1 层：矩阵的哪些行已经有自动覆盖

矩阵条款出自 `docs/MVP_IMPLEMENTATION_SPEC.md` §16。写在这里是为了让「这条没自动覆盖」
变成一个明确的事实，而不是一个没人知道的洞——没有自动覆盖的行全部落在 §4 的手工清单里。

### 交互终端

| 矩阵行 | 自动覆盖 |
| --- | --- |
| PowerShell 能起来 | `session::core::tests::terminal_tests::starting_a_terminal_hosts_a_shell_and_reports_it_attached`、`pty::tests::spawn_reports_a_pid_and_first_output`；三个终端同时起来见集成 `three_concurrent_sessions_hold_distinct_runs_and_processes` |
| 能输入命令 | `terminal_tests::a_command_in_the_hosted_shell_reaches_the_sessions_scrollback`、`each_terminal_answers_only_its_own_input`（集成） |
| Unicode | `terminal_tests::non_ascii_input_reaches_the_shell`、`non_ascii_output_survives_the_session`、`pty::tests::unicode_survives_the_round_trip` |
| Ctrl+C 打断长命令 | `terminal_tests::ctrl_c_interrupts_the_command_that_is_running`、`ctrl_c_is_input_and_does_not_close_the_session`、`pty::tests::ctrl_c_interrupts_the_running_command_not_the_shell` |
| 调整尺寸 | `terminal_tests::a_resize_of_a_live_terminal_reaches_the_shell`、`a_resize_before_the_start_geometries_the_shell`、`pty::tests::resize_reaches_the_shell` |
| 切换会话不销毁 PTY | `terminal_tests::a_terminal_keeps_running_while_no_view_is_attached`、集成 `a_terminal_keeps_running_with_no_view_attached_and_replays_on_attach` |
| **关闭终端结束所属进程树**（#61、D-028） | `pty::tests::kill_ends_the_shells_children_without_the_handle_going_away`、`the_terminal_owns_the_processes_its_shell_starts`、`a_start_that_cannot_own_the_shell_leaves_no_shell_running`（三个启动步骤的失败清理）、`terminal_tests::stopping_a_terminal_ends_the_processes_its_shell_started`、`a_shell_that_exits_first_still_ends_its_tree_before_the_session_ends`、`closing_a_terminal_leaves_other_sessions_and_unrelated_processes_alone`。集成侧不重复：`tests/mvp_matrix.rs` 的文档注释把这一层划归各模块自己的套件 |
| **一键新建临时 PowerShell**（#62、D-031） | `session::temporary::tests`（shell 偏好 `pwsh`→`powershell`、主目录/入口目录、缺目录按名报错、身份唯一且可作路径分量、带空格路径的引用）、`session::core::tests` 的四条拒绝（无 shell / 目录不存在 / 已配置不可删 / 未知会话）与 `only_a_settled_temporary_session_is_removable`（六态 × 有无 run 的删除门槛表，含「error 且仍握有 run 时不可删」）、`temporary_tests` 七条（一次点击得到家目录里的真实 shell、创建早于它的状态事件、两次点击两个会话、启动失败不留行、运行中不可删、结束后保留输出并可移除且迟到发布不复活、不落盘输出不写配置）、集成 `the_quick_entry_adds_a_terminal_on_top_of_a_loaded_workspace`（真实配置 + 真实终端 + 重载后不恢复） |
| **托盘隐藏/恢复不销毁 PTY** | **手工**（§4 托盘段）。自动侧只有它的两半：隐藏路径不碰 Session Core（`tray::tests::only_the_main_window_hides_on_close`），以及「没有视图挂着时终端照跑」（上一条） |
| **空工作区也能新建**（#62、story 7） | 后端与「已有工作区」是同一条路径（`temporary_tests` 与集成那条都不依赖预置会话）；空工作区那一屏是 `App.tsx` 的渲染分支，**自动侧无覆盖**，由 §6 的 2026-09-30 #62 原生轮次在真实窗口里核对 |

### 服务

| 矩阵行 | 自动覆盖 |
| --- | --- |
| 长驻服务能起来 | `session::core::tests::a_service_session_maps_to_a_supervised_process`、集成 `three_concurrent_sessions_hold_distinct_runs_and_processes` |
| stdout/stderr 可见 | `session::core::tests::logging::a_run_file_carries_both_streams_tagged` |
| 配置的端口 / URL 可见 | `a_running_service_reads_its_configured_port`、`a_running_service_whose_port_is_closed_reports_both_facts`、`a_service_with_no_port_is_never_probed`、`a_session_url_comes_from_the_session_that_owns_it` |
| 重启会等前一个进程退出 | `process::tests::restart_leaves_exactly_one_run_alive`、`terminal_tests::restarting_a_terminal_replaces_the_run_and_leaves_one_shell`、集成 `a_restart_starts_a_new_run_and_keeps_the_scrollback` |
| 启动器快速退出时，启动期间创建的子进程仍属于受管 Job | `process::tests::an_early_descendant_stays_in_the_run_after_its_launcher_exits`（真实 Windows 进程；子进程 PID 握手，并检查无关哨兵存活） |
| Job 创建、进程归属或启动恢复失败时不遗留挂起进程 | `process::tests::startup_failures_clean_up_the_suspended_process`（真实 Windows 进程；分别注入三个启动步骤失败并检查 PID 已退出） |
| 优雅停止可用 | `process::tests::stop_ends_a_live_run_and_leaves_nothing_in_the_tree`、`session::core::tests::force_stop_ends_the_run_without_waiting_for_it` |
| 强制结束只动受管树 | `process::tests::force_stop_removes_the_managed_tree_but_not_an_unrelated_process`、`stop_reclaims_a_descendant_left_behind_by_an_exited_run` |

### 日志

| 矩阵行 | 自动覆盖 |
| --- | --- |
| `off` 不产生持久日志 | `session::core::tests::logging::an_off_run_leaves_no_file_and_still_has_a_scrollback`、集成 `a_terminal_persists_nothing_and_a_captured_service_writes_one_file_per_run` |
| `captured always` 产生按 run 的文件 | `logging::a_captured_run_writes_a_file_its_run_record_points_at`、集成同上 |
| `on_error` 保留错误前上下文 | `logging::on_error_keeps_the_context_of_a_failing_run_only`、`stopping_an_on_error_run_on_purpose_writes_no_log`、`saving_a_running_on_error_log_commits_it` |
| `external` 只链接不复制 | `logging::an_external_session_links_the_application_log_and_captures_nothing`、`an_external_session_points_at_the_application_own_log`、`a_sweep_never_takes_an_application_owned_log` |
| stdin 不落盘 | **手工**（§4 日志段最后一行）——行为由「没有 stdin 写入路径」保证，但矩阵问的是可观察结果 |
| 保留策略只删文件、保留运行记录 | `logging::a_swept_run_stays_in_the_history_without_its_log`、`a_cleanup_leaves_logs_inside_the_retention_window_alone`、`a_cleanup_preview_describes_the_sweep_without_making_it`、集成 `a_sessions_log_path_is_resolved_from_the_session_not_from_a_caller` |
| 单次运行日志上限 16 MiB：截断并在文件里写明（LOGGING §9） | `logging::run_log::tests::the_file_cap_truncates_instead_of_growing_without_limit` |
| 每会话总量上限 256 MiB、保留 30 天，且最近一次运行始终保留（LOGGING §9） | `logging::retention::tests::one_session_exceeding_its_budget_does_not_delete_another_session`、`an_ancient_file_is_removed_and_the_budget_still_applies`、`files_older_than_the_age_limit_are_removed`、`a_zero_day_limit_disables_the_age_rule_rather_than_deleting_everything` |
| `off` 会话不会伪造空记录、`on_error` 正常退出不落盘 | `logging::an_off_run_leaves_no_file_and_still_has_a_scrollback`、`stopping_an_on_error_run_on_purpose_writes_no_log` |

### 多会话

| 矩阵行 | 自动覆盖 |
| --- | --- |
| 至少三个会话同时跑 | 集成 `three_concurrent_sessions_hold_distinct_runs_and_processes`、冒烟 `multi_session_smoke` |
| **运行中新增/移除会话时窗口与后端一致**（#62、D-031） | 前端 `session-registry.test.ts` 的 membership 一组（初始化期间创建、配置与运行态乱序、重复事件不重复插入、已移除后迟到状态不复活、删除先于快照、状态事件不被读成删除、停止后不再应用）；后端 `temporary_tests::a_creation_is_announced_before_any_of_its_states`（创建早于状态）、集成同上；托盘 `tray::tests::only_registry_and_state_events_reach_the_tray`（成员变更会重建菜单，输出批次不会） |
| 停一个不影响其它 | 集成 `stopping_one_session_leaves_the_others_untouched`、`tray::tests::stop_all_stops_the_running_sessions_and_leaves_the_rest_alone` |
| 一个刷屏不会让另一个不可用 | 集成 `a_noisy_session_does_not_disturb_the_quiet_one`、`pty::tests::high_volume_output_flows_and_the_shell_stays_responsive`、`a_flooding_terminal_stays_alive_while_unread` |
| 滚动缓冲有界且如实上报丢弃 | 集成 `a_snapshot_reports_an_intact_buffer_without_carrying_it`（未丢弃一侧）、冒烟 `multi_session_smoke` 的 `bounded scrollback` 步（丢弃一侧） |

### 托盘

| 矩阵行 | 自动覆盖 |
| --- | --- |
| ✕ 隐藏窗口 | `tray::tests::only_the_main_window_hides_on_close` |
| 会话继续跑 | `tray::tests::stop_all_stops_the_running_sessions_and_leaves_the_rest_alone` 等真实进程用例 + 隐藏路径不碰 Session Core |
| 显示主窗口能恢复现场 | `tray::tests::the_focus_request_serializes_to_the_camel_case_contract`（请求载荷）；**真实点击与窗口恢复为手工** |
| 摘要跟随运行状态 | `tray::model::tests::the_summary_counts_running_and_failed_sessions`、`the_summary_never_invents_a_busy_count`、`tray::tests::only_state_and_summary_events_reach_the_tray` |
| 退出不会静默毁掉正在跑的东西 | `tray::actions::tests::a_running_session_makes_exit_ask_first`、`an_idle_hub_exits_without_a_question`、`a_session_mid_flight_blocks_exit_before_any_question` |
| **托盘菜单本身（图标、文案、灰态、真实点击）** | **手工**（§4 托盘段）——`tray::install` 需要真的 `TrayIcon`，单测覆盖不到 |

### UI 契约（spec §10、§17）

| 条款 | 自动覆盖 |
| --- | --- |
| 关闭影响是选中会话自己的文本，不是通用填充 | 集成 `the_bootstrap_path_turns_a_config_file_into_stopped_sessions` 断言 `close_impact` 取自配置；`headerCallout` 的文案规则见 `src/state/derivations.test.ts`。**在真实窗口里看到它**是 §4 S-9 |
| Terminal / Logs / Details 三分离、不互相重复 | 规则层由 `src/state/logs.test.ts` 与 `derivations.test.ts` 覆盖；**渲染出来的分离**见 §5 的 `service-details` / `service-logs` 截图与 §4 L-1 / L-7 |
| PTY 交互是真的，不是只读模拟（§17 第 5 条） | `terminal_tests::*` 整组（真实 ConPTY 上的输入、Ctrl+C、resize、非 ASCII）；曾经存在的只读回退见 §6 |
| 只占一个主窗口（PRODUCT_SPEC §12 场景 2） | **手工**：§4 R-1 / R-4 —— ✕ 是隐藏、托盘是唯一入口 |
| Hub 不会自动收编别人的后台进程（PRODUCT_SPEC §12 场景 10，D-002 / D-012） | **手工**：§4 C-4 |

### 配置校验与进程安全

| 条款 | 自动覆盖 |
| --- | --- |
| 一条坏会话不藏起好会话（spec §13） | `config::tests::one_invalid_session_does_not_hide_valid_ones`、`app::tests::a_bad_entry_does_not_hide_the_good_ones` |
| 重复 id / 未知类型 / 缺 command / 缺 shell / 坏 URL / 坏端口 / 矛盾日志组合 | `config::*`（32 条，含 dto 与 paths 的形状与路径规则） |
| 没有任何已知路径会杀掉无关进程（§17） | `process::tests::force_stop_removes_the_managed_tree_but_not_an_unrelated_process`、`stop_reclaims_a_descendant_left_behind_by_an_exited_run`、`session::core::tests::input_into_a_service_is_refused` |
| 一个 `external` 会话永不被清理删除 | `logging::a_sweep_never_takes_an_application_owned_log` |

---

## 4. 第 2 层：手工清单

### 准备

仅补验日志操作时，可双击 `scripts/verify-log-actions.cmd`。该入口在操作者的实际 Windows
环境中检查配置：文件缺失时原子安装仓库验收 fixture，已有文件不覆盖；随后启动开发应用。
需要先安装项目依赖，退出已有开发应用并关闭它的启动窗口，避免开发端口 24120 被占用。
已有配置若没有 `term-manual` / `svc-fails`，仍需按下面步骤先备份再自行配置；入口不会替换它。
这只是显式的验收辅助入口，普通应用启动不会自动创建或修改配置。

1. 备份并替换配置：

   ```text
   copy fixtures\verification-config.yaml %APPDATA%\LocalConsoleHub\config.yaml
   ```

   这份 fixture 的会话就是下面每行的对象；删掉其中一个会话，就有对应行没得验（
   `src-tauri/src/config/mod.rs` 的 `verification_fixture_loads_cleanly_and_covers_the_matrix`
   会因此变红）。

2. 给 `svc-external` 一个真实存在的「应用自带日志」。fixture 里写的是
   `C:\Tools\demo-app\access.log`——一个占位路径，换成你机器上任意一个文件即可，
   或者先把那个文件建出来：

   ```text
   if not exist C:\Tools\demo-app md C:\Tools\demo-app
   copy nul C:\Tools\demo-app\access.log
   ```

   **路径是字面量**：配置不做环境变量展开（`src-tauri/src/config/validate.rs`），
   相对路径也只会相对**应用进程**的工作目录（`npm run tauri dev` 时是仓库根），
   所以要写完整路径。

3. 确认端口 **28900 / 28901 / 28902** 空闲。开发服务器用 **24120**，不要用 1420
   （Windows 保留端口段，`CLAUDE.md`）。

4. 起应用：

   ```bash
   npm run tauri dev
   ```

5. 两条**预期之内、不是缺陷**的观感，先说在前面，免得当成 bug 去追：

   - 详情页「它是谁」那张卡片在这个 fixture 下只显示一个 `.`。那是 `cwd: .`
     被如实显示——这张卡显示的是**配置里的值**，而 fixture 为了在任何机器上都能加载
     才写了 `.`（见文件头）。换成绝对路径就会显示绝对路径。
   - 服务会话的终端面板顶部写 `PTY 未连接 · 只读缓冲`，而服务参考图写
     `PTY attached · stdin 可用`。受管服务没有可输入的 stdin，参考图那一处是上游
     原型的示意值（`docs/DESIGN_SPEC_EXTRACTED.md` §5 第 7 条）。

### 交互终端（`term-pwsh`）

| # | 步骤 | 期望 |
| --- | --- | --- |
| T-1 | 选中 `PowerShell`，按「启动」 | 状态点变绿，头部出现 `PID` / `up` / `cwd`，终端顶部条 `ConPTY · interactive`，右侧 `connected` |
| T-2 | 敲 `dir` 回车 | 输出出现在面板里；可以继续敲下一条 |
| T-3 | 敲 `Write-Host "你好 — ünïcödé 🚀"` | 中文、变音、emoji 原样显示，没有替换字符 |
| T-4 | 敲 `ping -n 60 127.0.0.1`，然后按 Ctrl+C | 命令被打断，**提示符回到同一行**，会话仍是 `Running`（Ctrl+C 是输入，不是关闭） |
| T-5 | 拖动窗口改尺寸，再敲 `$Host.UI.RawUI.WindowSize.Width` | 数字随窗口变化（resize 传到了 shell） |
| T-6 | 切到「日志」再切回「终端」 | 之前显示过的输出**不重复、不丢失**，可以继续输入 |
| T-7 | 选另一个会话再切回来 | 同上；被切走的终端没有被销毁 |
| T-8 | 打开「日志」页 | 徽标是 `Off`，没有「当前日志文件」路径，历史里没有伪造的空记录 |
| T-9 | 敲一条命令后，在「详情」看 `内存缓冲` | 字节数 / 行数在涨，`未丢弃` |
| T-10 | 跑一个会**等你输入**的程序：`Read-Host "name"`，回车后输入 `ada` 再回车 | 提示行出现，输入被程序接收并回显；全程不需要别的窗口（DEVELOPMENT §14「需要输入的测试程序」） |
| T-11 | 选中 `PowerShell`，看头部名称下面那行与「关闭影响」条 | 两处分别显示配置里的原文（「日常交互终端，跑一次性命令与 REPL。」/「仅结束本终端；不会停止其它受管服务。」）——与 `ui-v2-terminal.png` 逐字一致；切到 `term-manual`（终端，两项都没写）时关闭影响是 `—`（D-027、UI_STYLE_GUIDE §5/§13） |

### 服务（`svc-listening`）

| # | 步骤 | 期望 |
| --- | --- | --- |
| S-1 | 选中 `Listening Service`，按「启动」 | 立刻 `Running`；端口开始监听后徽标变 `Ready`（D-023） |
| S-2 | 看终端面板 | `serving 1`、`serving 2`… 逐行出现（stdout 可见） |
| S-3 | 看头部元数据行 | `port :28900`、`cwd`、`log always` |
| S-4 | 按「打开网页」 | 系统浏览器打开 `http://127.0.0.1:28900`（会连上但无响应——这个 fixture 只监听不回应） |
| S-5 | 按「目录」 | 资源管理器打开会话的 cwd |
| S-6 | 按「重启」 | 旧进程先结束、新 run 才起来（PID 与 run id 都变），**不会出现两个 `powershell` 抢 28900** |
| S-7 | 按「停止」 | 立刻变 `Stopping`，随后 `Stopped`；任务管理器里没有残留子进程 |
| S-8 | 再启动一次，然后用头部溢出菜单里的「强制结束」 | 只影响本会话进程树；其它会话的 PID 不变 |
| S-9 | 启动前、运行中、以及选中另一个会话时，各看一眼头部的「关闭影响」条 | 运行中**且被选中**时才出现；文字是本会话配置里的原文（`svc-listening` 是「可停止；正在等待该端口的调用方会失联」），不是通用填充；切到别的会话就换成那个会话的（#12 验收条） |

### 日志

| # | 步骤 | 期望 |
| --- | --- | --- |
| L-1 | `svc-listening` 跑一会儿，看「日志」页 | 徽标 `Capturing` + `Hub captured` + `stdin 不记录`；当前日志文件路径存在 |
| L-2 | 按「打开日志」 | 用默认编辑器打开该文件，内容与终端输出一致（含 stderr，带流标记） |
| L-3 | 按「打开目录」 | 打开 `%LOCALAPPDATA%\LocalConsoleHub\logs\svc-listening\<年-月>\` |
| L-4 | 按「复制路径」再粘贴 | 路径与显示一致 |
| L-5 | 重启该会话（新 run），再看「运行历史」 | 上一个 run 留在历史里，新的 run 另起一个文件；**旧文件没有被覆盖** |
| L-6 | 手动删掉当前 run 的文件，再看「当前运行」卡片 | 消失的是「打开日志」和「复制路径」；「打开目录」仍在，并说明文件不在磁盘上（D-022，措辞只陈述事实不猜原因） |
| L-7 | 选中 `term-pwsh`，看「日志」页 | `Off`；「交互终端默认不产生磁盘日志」，历史里没有伪造的空记录 |
| L-8 | 选中 `svc-fails` 启动，等它自己以退出码 3 结束 | 历史里这一行是 `error`；日志文件**此时才出现**，且包含 `progress 1..10` 与 `about to fail`（错误前上下文） |
| L-9 | 选中 `svc-external`，看「日志」页 | 徽标 `External`；只有一个入口指向第 2 步那个文件，Hub **没有**复制一份 |
| L-10 | 往那个文件里写点东西，再按打开 | 打开的就是那个文件本身 |
| L-11 | 选中 `term-manual`，在「日志」页按「开始记录」，敲几条命令，再按「保存本次日志」 | 保存前徽标是 `Off`（策略允许不等于正在记录，LOGGING §3）；保存后出现本次 run 的文件 |
| L-12 | 在任意终端里敲命令，事后在日志文件 / 元数据里搜这些输入 | **搜不到**：stdin 不落盘（LOGGING §4、spec §15） |
| L-13 | 「日志」页右上「清理日志」 | 两步确认；执行后历史的运行记录**仍在**，只是文件动作被禁用 |
| L-14 | 让一个 `always` 会话持续输出到超过 16 MiB（`while ($true) { "x" * 200 }` 跑一会儿），然后看文件 | 文件涨到上限就停住，不再增长，并且**文件里写明已截断**而不是静默停止（LOGGING §9；行为由 `run_log` 的 `the_file_cap_truncates_instead_of_growing_without_limit` 保证，这一行是它的可观察面） |

### 多会话

| # | 步骤 | 期望 |
| --- | --- | --- |
| M-1 | 同时启动 `term-pwsh`、`term-manual`、`svc-listening` | 三个都在跑；头部摘要 `3/x 运行`；状态栏 `3/x 运行` |
| M-2 | 在 `term-pwsh` 里跑一个长循环刷屏（例如 `1..20000 \| % { "line $_" }`），期间在另一个终端敲命令 | 另一个终端仍然**立刻**响应；整个窗口不卡死、不假死 |
| M-3 | 停止 `svc-listening` | 两个终端仍在跑，PID 不变 |
| M-4 | 在 `term-pwsh` 按 Ctrl+C 停掉刷屏 | 窗口恢复正常，滚动缓冲上限被遵守（详情页可见「已丢弃」字节数） |
| M-5 | 反复切换三个会话并观察 | 输出不重复、不丢失；每个终端的滚动内容互不串台 |
| M-6 | 给某个会话一个很长的 `name`（例如把 `term-manual` 改成 `PowerShell (recording on demand)`），重起应用，看侧栏那一行 | 行**不会**压到右侧工作区：名字省略成 `PowerShell (recording on …`，右边状态/时长完整可见，侧栏宽度仍是 280。这一条没有自动守卫——仓库的前端测试环境是 node，没有 DOM（#25 明确不引入），CSS 布局断言不了，所以清单就是它的守卫。曾经长名字会把 `.sidebar` 撑到 316px 并盖住终端

### 托盘

| # | 步骤 | 期望 |
| --- | --- | --- |
| R-1 | 三个会话都在跑时，点主窗口的 ✕ | **窗口隐藏而不是退出**；任务栏图标消失，托盘图标在 |
| R-2 | 等 30 秒，用任务管理器确认 | 三个进程都还活着；隐藏期间被管会话继续输出 |
| R-3 | 右键托盘 | 菜单自上而下：摘要（`N 运行 · M 失败`）、运行/失败的会话行（最多 8 行）、显示主窗口、重启失败的会话、停止全部、退出；有会话时「重启失败」「停止全部」是灰的而不是消失的 |
| R-4 | 选「显示主窗口」 | 窗口回来，**现场就是隐藏前那个现场**：选中的会话、终端里已经显示过的输出、可以继续输入 |
| R-5 | 让 `svc-fails` 跑失败，再看托盘 | 摘要的失败数 +1，会话行进列表；「重启失败的会话」变为可用，点了只重启失败的那个 |
| R-6 | 三个会话都在跑时点托盘「退出」 | **先弹确认**（「停止受管会话并退出」/ 取消）；选取消 → 什么都没发生；选确认 → 会话被停止后应用退出 |
| R-7 | 没有会话在跑时点托盘「退出」 | 直接退出，不弹确认 |
| R-8 | 看任务栏与托盘图标 | 与 Hub 图标一致（D-029） |

### 标题栏与窗口（#68，对应原生验收 H16）

窗口自 #68 起没有系统装饰，内部深色标题栏就是窗口的标题栏（D-029），所以下面这些是
**只能在真实窗口上做的**检查——WebView 截图证明不了系统装饰、贴靠与任务栏图标。

| # | 步骤 | 期望 |
| --- | --- | --- |
| W-1 | 看窗口顶部 | **只有一层标题**：左边 Hub 图标 + `Local Console Hub` + （开发态）`UI 预览`，右边全局摘要与三个窗口按钮；没有第二层系统标题、没有重复的应用名 |
| W-2 | 按住标题栏空白处拖动 | 窗口跟着走；拖到屏幕上沿/左右边缘能贴靠（Windows 的 Aero Snap）；双击标题栏在最大化与还原之间切换 |
| W-3 | 拖窗口的四边与四角 | 都能缩放；缩放时光标变成对应的双向箭头；最大化时不能拖边框（Windows 语义） |
| W-4 | 点 `—` / `□`（最大化后变 `❐`）/ `✕` | 最小化到任务栏；最大化铺满工作区且按钮变成「还原」形态，再点回到原尺寸；`✕` **隐藏而不是退出**（同 R-1/R-2，会话继续跑） |
| W-5 | 看窗口在最小宽度（960）下，以及浏览器预览里的窄宽度（<768） | 会话列表按钮与窗口按钮都还在、互不遮挡；终端区域不被标题栏挤掉。桌面窗口到不了 768 以下（`minWidth: 960`），窄布局只能在预览里看 |
| W-6 | 看任务栏、托盘、开始菜单快捷方式与标题栏左上角 | 四处是**同一颗** Hub 图标（深色圆角方块 + 绿点 + 三条列表行，D-029）；tray 与任务栏图标在 16/32px 下仍可辨认 |

### 配置校验

| # | 步骤 | 期望 |
| --- | --- | --- |
| C-1 | 在配置里加一条 `type: nonsense`，重起 | 窗口正常打开；那条会话报错，其余会话照常列出（spec §13） |
| C-2 | 复制一条会话的 `id`，重起 | 重复 id 报错，其余不受影响 |
| C-3 | 给一条 terminal 写 `port: 1234`，重起 | 报「`port` only applies to `type: service` sessions」——按类型归属拒绝，而不是静默丢弃 |
| C-4 | 在别处自己起一个 `powershell` / `cmd` / 某个 node 服务，然后看 Hub 的侧栏 | 它**不出现**：Hub 只列出 `config.yaml` 里写过的会话，从不扫描或收编系统上已有的控制台进程（PRODUCT_SPEC §12 场景 10、spec §15、D-002 / D-012） |

### 进程安全

| # | 步骤 | 期望 |
| --- | --- | --- |
| P-1 | 另起一个同名进程（例如自己在 cmd 里跑一个 `powershell.exe`），再停止 Hub 里的 `term-pwsh` | 自己那个不受影响 |
| P-2 | 停止一个服务后，在任务管理器里搜它的子进程 | 没有残留（job object 树结束） |
| P-3 | 让 `svc-fails` 自然退出后，看它的状态 | `Error` 而不是 `Exited`（退出码非 0），退出码在详情里 |

---

## 5. 第 3 层：视觉验收

参考图是 `assets/ui/ui-v2-service.png`（服务态）与 `assets/ui/ui-v2-terminal.png`（交互终端态）。
它们画的是 **fixture 工作区**（六个会话、三个分组），`src/state/fixtures.ts` 存在的理由之一
就是与参考图比对（`DECISIONS.md` D-020）。

1. 截两个态：

   ```bash
   npm run build
   ```

   ```bash
   npm install --no-save puppeteer-core
   ```

   ```bash
   node scripts/capture-ui-states.mjs
   ```

   脚本用交付用的 `dist/` 起一个环回静态服务，在系统自带的 Edge 里把 fixture 工作区
   渲染成 1280×800，输出 `service-terminal` / `service-logs` / `service-details` /
   `terminal-terminal` / `terminal-logs` 五张 PNG（默认写 `%TEMP%\lch-ui-capture`）。
   Edge 的 headless 启动在本会话失败（`Code: 0`）时，可以把 `EDGE_PATH` 指向本机
   Chrome 走同一条路——#68 会话就是这么跑的，见 §6。

2. 逐区域比对（布局 / 组件 / 颜色语义 / 间距 / 字号）：标题栏、侧栏分组与行、
   选中会话头部（名称 / 类型 / 状态 / 动作 / 用途 / 关闭影响 / 元数据行）、页签、
   终端面板、状态栏。**MAJOR 级差异 = 拦住发布。**

3. 同时对照 `docs/UI_STYLE_GUIDE.md` §13 的发布验收条，尤其是：
   终端是主体区域、没有常驻右侧仪表盘、没有装饰性会话图标、没有重复的全局 Logs 入口、
   关闭影响在破坏性动作之前可见、Terminal / Logs / Details 不互相重复。

### 与参考图的有意偏差（**不是缺陷**）

`docs/DESIGN_SPEC_EXTRACTED.md` §5 记录了 8 条，比对时按「预期」处理，逐条如下：
不实现 `Ctrl K` / 全局设置 / 日志入口 / `退出`（窗口控制本身自 #68 起已实现，
见 D-029）；`UI 预览` 徽标只在检测不到
Rust 后端时出现；行内日志标签显示 `External` 而不是参考图的 `Auto`（`auto` 在到达前端前
已被解析）；详情面板不重复头部元数据；fixture 计时是相对的，字面值不同属正常；
`on_error` 拼作 `on error`；服务终端的连接条是 `PTY 未连接 · 只读缓冲` 而不是参考图的
`PTY attached · stdin 可用`（受管服务没有可输入的 stdin，UI_STYLE_GUIDE §7 禁止声称
快照没有报告的连接）；活动终端的「关闭影响」callout 在配置原文之下多一行
`停止该终端会同时结束它启动的子进程。`（#61，D-028）。

### 首轮环境无法完成的部分（历史边界）

原生窗口（真实 `tauri dev` 出来的那个窗口）在首次 T11 agent 会话里**没有截图能力**：
`preview_*` 工具只到浏览器面板，而面板在本会话被 worktree 隔离挡住。所以：

- **可以**用上面的脚本对 `dist/` 做完整的视觉比对（§6 记录的就是这么做的）；
- **不能**截图原生窗口本身。因此「原生窗口的窗口装饰、任务栏/托盘图标外观、
  托盘菜单的实际排版」这三项必须由人在桌面上看，归属 §4 的 R-8 与 R-3。

这不是对所有后续会话工具能力的断言。2026-09-30 的 #57 会话已能枚举原生窗口；
截图操作被用户以 Escape 中止。本轮用户随后明确确认 #57 验收通过，来源与限制见 §6。

---

## 6. 运行记录

### 2026-09-29 — T11 首次完整运行

环境：Windows 11 Pro（10.0.26200），worktree `t11-e2e-verification`（基 `main` @ `c5b809a`），
`CARGO_TARGET_DIR` 复用主检出的 `target`。

**第 1 层。** 全绿：

| 套件 | 结果 |
| --- | --- |
| `npm run check` | 通过 |
| `npm test` | 180 passed / 11 files |
| `npm run lint`、`npm run format:check` | 通过 |
| `cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test` | **322 passed**（312 lib + 10 `tests/mvp_matrix.rs`）/ 0 failed，约 14 s |
| `cargo run --example multi_session_smoke` | 全部步骤通过 |

`multi_session_smoke` 的实测数字（有界缓冲那一条的证据）：

```text
start all four: ok — [Some(39352), Some(30896), Some(56036), Some(34484)] in 75.9ms
a quiet terminal while another floods: ok — term-a answered in 50.7ms
flood completes: ok — 12000 lines in 8.23s — kept 262120 bytes / 4946 lines, discarded 374709 bytes
bounded scrollback: ok — discarded 374709 bytes and said so
the flood stayed in its own session: ok — no other session discarded anything
stop one session, keep the rest: ok — term-b ended; term-a, term-c and svc kept their own run and pid
restart the stopped session: ok — a new run and a new process, with the earlier scrollback still readable
per-session report:
  term-a  Running  pid 39352  buffer     624 B /    4 lines, dropped       0 B, 0 log file(s)
  term-b  Running  pid 44872  buffer     733 B /    3 lines, dropped       0 B, 0 log file(s)
  term-c  Running  pid 56036  buffer  262120 B /  4946 lines, dropped  374709 B, 0 log file(s)
  svc     Running  pid 34484  buffer     533 B /   12 lines, dropped       0 B, 1 log file(s)
```

四点值得单独记下：三个终端各自跑着、互不串台；12000 行洪水期间另一个终端 50 ms 内应答；
保留字节 262120 ≤ 上限 262144、4946 行 ≤ 5000 行，**且丢弃的 374709 字节被如实上报**
而不是悄悄消失；三个终端落盘 0 个日志文件、服务落盘 1 个——日志默认值在端到端路径上成立。

**第 1 层补记：** 写 `tests/mvp_matrix.rs` 时发现 `SessionCore::configs()` 的契约是
**id 序**而不是文件序（文档注释说的就是 id order，测试已按契约断言）。

**第 3 层。** 用 §5 的脚本截图后逐区域比对两份参考图：

- **服务态（`service-terminal` vs `ui-v2-service.png`）**：布局、侧栏三个分组与六行、
  头部（名称 / `Service` / `Busy` / 四个动作 / 用途 / 关闭影响 / `PID · port · up · cwd · log`）、
  页签、状态栏逐区域一致。唯一文字差异是本文件 §5 记的第 7 条有意偏差
  （连接条 `PTY 未连接 · 只读缓冲`），已补进 `DESIGN_SPEC_EXTRACTED.md` §5。
- **交互终端态（`terminal-terminal` vs `ui-v2-terminal.png`）**：逐区域一致，
  含 `ConPTY · interactive`、动作集合（无「打开网页」）、
  `log buffer only`、以及终端正文的 fixture 内容。计时字面值不同（`12m 0s` vs `23m 47s`）
  属有意偏差第 5 条。
- **Logs（`service-logs`）**：`Capturing` / `Hub captured` / `stdin 不记录` 三个徽标、
  当前 run 路径、滚动缓冲摘要、三个文件动作、运行历史里被扫掉的那个 run
  只留「打开目录」并写明「日志文件不在磁盘上（运行记录保留）」——与 UI_STYLE_GUIDE §8
  和 D-022 一致。
- **Details（`service-details`）**：「它是谁」/「能不能关」两张卡片 + 低频字段表
  （类型、状态、健康、启动命令、Run、退出码），**没有**重复头部的 PID / 端口 / cwd /
  日志模式（§13 禁止过度重复）。

**没有 MAJOR 级差异。**

**第 2 层。** 未在本会话执行，原因见 §5 末节：原生窗口与托盘无法被 agent 截图或点击。
§4 的清单已按可独立执行的形式写全，需要由人在 Windows 桌面上过一遍；其中
R-3 / R-4 / R-8 与 T-8、L-11 是**必须**人工确认的项。

### 本次运行发现的问题

| 发现 | 处置 |
| --- | --- |
| 连接条文案与参考图不一致（服务态） | 有意偏差，已记入 `DESIGN_SPEC_EXTRACTED.md` §5 第 7 条 |
| **交互终端无法携带 `purpose` / `close_impact`**，因此真实窗口里的终端头部永远对不上 `ui-v2-terminal.png`（fixture 工作区能对上，因为它在编造这两个字段） | 单开 #38。这是 config semantics 变更（spec §18 禁止静默改），需要独立决策记录，不在 T11 内改。**修于 #38**：D-027 把两个字段改为两种类型共有，§4 的 T-11 是它在真实窗口里的验收行 |
| T07 验收条件「UI 不得用只读/假终端路径顶替」在发版路径上不成立（一次失败的 ping 会把窗口永久留在预览工作区） | 本次会话内修复（#29），决策记录为 D-025 |

### 2026-09-29 补记 — 第 2 层手工验收那一轮

第 1 层与第 3 层是 agent 跑的；这一轮由人在 Windows 桌面上对着 §4 的清单点。托盘那几条
（R-1…R-8）当时通过。另外三件事是点出来的，都不是第 1 层能发现的——前两件是只有真实
窗口才看得见的渲染与布局，后一件是清单自己的 fixture 有问题：

| 发现 | 处置 |
| --- | --- |
| 受管服务的输出在终端面板里呈**阶梯状**向右走并在右边缘绕行 | 见下。修于 #42 |
| 长会话名把侧栏撑过它的 280px 轨道并**盖住工作区** | 见下。修于 #44 |
| §4 第 2 步让操作者创建的「应用自带日志」与 fixture 里 `logging.path` 指的**不是同一个文件** | 修于 #41：配置不做环境变量展开，路径是字面量；fixture 改成占位绝对路径并说明了两个坑，测试也补了会抓到这类错误的断言 |
| 详情页「它是谁」只显示一个 `.` | 预期之内：那是 `cwd: .` 被如实显示（§4 第 5 步已说明） |
| 服务停止时的浮层说「交互终端必须先启动进程」 | 修于 #42：按会话类型分开措辞 |

两处渲染/布局缺陷值得记下**为什么非手工不可**，因为它们的价值不在于「发现了 bug」，
而在于第 1 层从设计上就够不到它们：

- **阶梯**：交互终端跑在 ConPTY 上，那条路径会把 shell 的 `\n` 翻成 `\r\n`；受管服务的
  stdout 是普通管道，没人翻，裸 `\n` 到达终端后按 VT 规则只下移一行、不回第 0 列。
  两条路径的**字节**都是对的，错的是它们之间的不对称。断言只覆盖了单条路径，所以看不见。
- **侧栏交叠**：两个 `min-width: auto` 叠在一起——`.sidebar` 被自己的 min-content 托住，
  `.session-row__name` 的 `text-overflow: ellipsis` 因此是**够不到的死代码**。CSS 布局
  在仓库的 node 测试环境里断言不了（#25 明确不引入 DOM），所以 §4 的 M-6 就是它的守卫。

### 2026-09-30 — #55 服务启动归属回归

**第 1 层。** 在基线 `8c3fad7` 上，真实 Windows 回归测试暂停启动线程、等待子进程完成 PID 握手，随后旧的“先执行、再加入 Job”路径在启动器退出后得到 `AssignProcessToJobObject` Win32 错误 5；这确认竞态可动态复现。修复后，同一握手测试通过，进程先保持挂起并加入 Job，再恢复执行。

修复后的本地原生 Windows 验证：`cargo test` 的 335 个单元测试和 10 个集成测试全部通过（包含上述进程树回归及三种启动失败清理）；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 和 `git diff --check` 通过。

---

### 2026-09-30 — #52 日志文件入口补验

本轮由用户在真实 Windows 窗口操作，agent 读取临时本地诊断记录。测试会话为 `svc-fails`，
失败运行 `1a0f107ed320001` 的日志文件名为
`2026-09-30_14-37-03__run-1a0f107ed320001.log`。

| 原生验收项 | 结果与证据 |
| --- | --- |
| 打开已结束运行的日志文件 | **通过**。Session Core 解析到该运行的实际文件，文件存在；用户确认正常打开日志 |
| 打开该文件所在目录 | **通过**。目录存在；用户确认正常打开所在位置 |
| 手动记录开始/停止后的状态与按钮切换 | **通过**。用户确认开始后显示 Capturing/停止记录，停止后恢复 Off/开始记录 |
| 运行中手动保存后的成功反馈与路径 | **通过**。用户确认 `svc-fails` 运行期间主动保存成功，显示路径且可打开 |
| 命令失败的原生错误提示 | **已观察（修复前）**。用户历史截图显示会话结束后的保存请求报“没有可保存的运行”；随后 `47bcbda` 修复已结束会话仍提供该动作的问题，用户截图确认动作消失 |
| 实际磁盘写入失败返回 last_error | **本轮未单独复现**；错误不得报成功由日志操作协议回归测试覆盖，不声称真实磁盘故障已通过 |

诊断两次打开请求指向同一 run，路径编码正确，Shell 返回 42；用户实际看见文件与目录打开，
才作为原生通过证据，系统接受启动请求的返回码本身不算通过。

准备期间，应用读取配置两次返回 Win32 错误 3，且应用报告文件不存在；agent 命令环境却能
看到同一字面路径的文件。随后用户执行启动入口，入口在它自己的环境里新建了此前缺失的
测试配置，应用下一次读取成功。已确认执行环境间文件可见性不同，具体隔离机制未确定。
本轮没有修改 Shell 的打开逻辑，也没有证据将早先打开失败归因于 COM 或路径编码。

临时启动/打开动作追踪已从源码删除；应用源码恢复到 `47bcbda`，不保留诊断环境变量或
新增 COM 依赖。保留 `scripts/verify-log-actions.cmd` 和配置准备脚本，便于用户在真实
Windows 环境重复验收。配置准备验证通过：缺失配置与 fixture 哈希一致；再次运行保留
已有文件原始内容；无临时文件残留。该脚本检查在 workspace-write 沙箱内、仓库临时目录执行，
不替代用户已完成的真实窗口点击。

收尾检查：`npm test`（14 文件、205 项）、前端类型检查/Lint/构建、`cargo check`、Rust 格式
和差异检查通过。由于 Rust 应用源码没有最终变更，本轮未重跑 Windows 进程/ConPTY 全套测试；
历史结果仍按原日期与版本保留。#52 的开始/停止、主动保存与文件入口已获用户原生确认；
磁盘故障场景和其它发布清单仍按各自的未验边界保留，不因本轮结果改判。

---

### 2026-09-30 — #53 配置诊断窗口补验

用户在真实 Windows 窗口提供了配置错误、缺失与空配置的截图。补验开始时工作区为 `15fbacc`；截图中没有
二进制构建标识，不将工作区提交号当作独立核验过的可执行文件版本。

| 原生验收项 | 结果与证据 |
| --- | --- |
| 合法与非法会话混合时保留合法列表项并显示逐项诊断 | **通过**。窗口显示一个“正常终端”和黄色“配置问题（1）”；错误标识为“第 2 项 · bad-session · type”，说明 `nonsense` 应改为 `service` 或 `terminal`。用户进一步确认混合配置中的合法终端能够启动、输入命令并停止 |
| YAML 格式错误显示文件级诊断 | **展示通过**。窗口显示红色配置问题、实际配置路径及 YAML 解析失败原因，包含 line 2 column 7；不是普通空列表或首次运行提示 |
| 缺少配置时显示首次运行 | **通过**。更换为不补建配置的启动入口后，用户截图显示“首次运行”、实际配置路径及“配置文件尚未创建”，没有会话 |
| 合法空配置的说明 | **通过**。用户确认测试对象是内容为空的 `config.yaml` 文件；截图显示“配置文件中还没有定义会话”和“配置文件可正常读取，但当前没有会话条目”，符合预期；此结果不能作为读取失败提示通过的证据 |
| 配置读取失败时显示文件错误 | **通过**。在配置位置创建名为 `config.yaml` 的目录后，用户截图显示红色“配置问题（1）”、实际配置路径、`configPath` 字段以及“无法读取配置文件”“拒绝访问。（os error 5）”和检查权限后重启的提示；没有误判为首次运行或合法空配置 |
| 恢复原配置后的正常启动 | **通过**。用户两次明确确认恢复配置后可正常启动 |

截图显示实际读取位置为 `%APPDATA%\LocalConsoleHub\config.yaml`。
`.scratch/verify-config-*` 下的配置属于准备脚本临时检查目录，不是应用配置路径。
在仓库临时目录执行准备脚本，确认文件从缺失变为存在，且内容哈希与
`fixtures/verification-config.yaml` 一致；这只证明日志验收入口会补建配置，不代替原生验收。

补验使用 `scripts/verify-config-diagnostics.cmd`：该入口只启动开发应用，不创建或替换配置。
每次修改前从托盘退出应用并关闭旧启动窗口，再重新启动；配置只在应用启动时读取。
缺失场景需移走实际路径中的文件；读取失败场景可在该位置创建名为 `config.yaml` 的空目录。
完成后退出应用、移走测试内容并恢复备份。用户已确认恢复后正常启动；#53 的原生窗口
验收项已齐。本轮未改动应用源码，不重写既有实现与完整套件的历史证据；本项收尾不意味着
#54 的初始化期间托盘同步、#57 的同版本集成验证或发布/安装验收已经通过。

本轮在 workspace-write 沙箱定向运行
`cargo test --manifest-path src-tauri/Cargo.toml --lib app::tests::a_config_read_error_reports_the_file_and_repair_context -- --exact --test-threads=1`，
结果 1 通过、334 过滤。该测试以目录作为配置位置，覆盖真实文件读取到注册及报告的链路；
不启动用户会话或子进程，不替代读取失败提示的原生窗口验收。

收尾检查：workspace-write 沙箱中的 `npm test` 全部通过（14 文件、205 项），本记录的
Prettier 检查与 `git diff --check` 通过；Standards 与 Spec 两轴审查无可操作问题。
本轮无应用源码变更，未重跑 Windows 进程/ConPTY 全套测试或安装包验收。

---

### 2026-09-30 — #54 初始化期间托盘同步补验通过

使用 `scripts/verify-session-initialization.cmd` 的专用开发入口，真实快照先从后台读取，
再延迟一份列表的交付约 30 秒；后台会话、事件订阅和托盘操作继续运行。
`Ctrl+Alt+R` 只重新加载前端，因此可在快照等待期间操作真实 Windows 托盘。
步骤与夹具见 [`SESSION_INITIALIZATION_ACCEPTANCE.md`](SESSION_INITIALIZATION_ACCEPTANCE.md)。

| 原生验收项 | 结果与证据 |
| --- | --- |
| A：初始化期间从托盘停止 | **两种模式均通过**。用户确认符合步骤与预期，最终窗口和托盘符合停止状态 |
| B：初始化期间从托盘重启失败会话 | **两种模式均通过**。用户确认除徽标显示 `Ready` 外其余符合；截图显示测试 PowerShell 终端绿色 `Ready`、PID 22280、`connected`、PowerShell 提示符以及窗口摘要“1 运行” |
| C：初始化期间连续重启和停止 | **两种模式均通过**。用户确认符合步骤与预期，最终停止状态一致 |
| 恢复原配置后的正常启动 | **通过**。用户明确确认“恢复正常：是” |
| 模式 1 / 模式 2 各自覆盖 | **通过**。用户进一步明确确认“模式 1 和模式 2 都已完成”，覆盖运行态快照延迟与配置快照延迟两种模式 |

已核对 `src/state/derivations.ts`：`statusLabel` 只在底层状态为 `running` 且
`ready` 为真时显示 `Ready`；`isReady` 对该终端使用后台的 `ptyAttached`。
因此截图中的 `Ready` 符合运行态语义，之前步骤只写 `Running` 的预期过窄，现已修正。
截图不包含构建标识，不独立声称核验了二进制提交号；截图没有展示托盘菜单，
托盘动作及同步结果依据用户的明确人工确认，agent 未自行操作原生托盘。
#54 的初始化期间托盘同步原生补验已齐；不扩展为 #57 同版本集成、终端视觉或安装验收通过。

---

### 2026-09-30 — #57 同版本 MVP 集成验收

被验应用源码归档为 `4260c49`，包含 #52–#56 及 #54 的原生补验辅助代码；
前置工单当日实时确认为 CLOSED。完整环境、提交映射、命令、并发模式、失败调查及原生
证据来源见 [`MVP_INTEGRATION_ACCEPTANCE.md`](MVP_INTEGRATION_ACCEPTANCE.md)。

前端 14 文件、205 项，以及类型检查、Lint、生产构建通过；格式检查首轮因 7 个未跟踪
`.scratch/` 临时文件失败，排除该临时目录后同命令通过。workspace-write 沙箱的 Rust 格式和
Clippy 通过；沙箱外 Windows 用户 `NEWNAME\q9560` 串行运行 `cargo test -- --test-threads=1`，
335 单元与 10 集成测试全部通过。即时子进程归属、停止清理、重启旧 PID 退出、无关哨兵存活
均有具名真实 Windows 测试通过证据。

agent 的原生截图/点击链路被用户以物理 Escape 中止，之后未再操作。用户随后明确表示
“我确认没有问题，#57可以验收通过”。因此日志反馈、配置诊断、初始化托盘同步与 T-11
本轮记为**用户整体验收确认通过**；不补写不存在的截图、点击步骤或二进制构建标识。
R-1 至 R-8 仍引用 2026-09-29 人工轮次，不把它们改写为本版本重验。
I-7、I-8、I-15 及 I-16/I-17 的相应安装版窗口与交互部分继续待验。

---

### 2026-09-30 — #61 终端关闭结束所属进程树（D-028）

工单 #61（父规格 #59 的决策 13）。基线 `bbd1c35`，worktree
`select-complete-ticket-a533fa`，`CARGO_TARGET_DIR` 复用主检出的 `target`。工作区为
Windows 11 Pro（10.0.26300），真实用户会话；所有进程证据都来自真实 ConPTY shell 与真实
Windows 作业对象，不是 mock。

**先记录缺陷本身。** `pty::tests::kill_ends_the_shells_children_without_the_handle_going_away`
在修复前是红的，失败信息给出具体 PID：

```text
closing the terminal must end the shell's children, still saw [35860]
```

关掉 `watch_run` 里的 `settle_tree` 调用后，
`terminal_tests::a_shell_that_exits_first_still_ends_its_tree_before_the_session_ends`
同样转红（`the session reported an ending while the tree it owned was still running`），
随后恢复。两条都是「先看见红、再看见绿」，不是事后补写的断言。

**第 1 层（自动）。** 全绿：

| 套件 | 结果 |
| --- | --- |
| `npm run check`、`npm run lint`、`npm run format:check`、`npm run build` | 通过 |
| `npm test` | 209 passed / 14 files |
| `cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test` | **341 lib + 10 `tests/mvp_matrix.rs`，0 failed**（基线 335 + 10） |

本片新增的 6 个 Rust 用例与它们各自钉住的东西：

| 用例 | 钉住的行为 |
| --- | --- |
| `pty::tests::kill_ends_the_shells_children_without_the_handle_going_away` | 句柄还活着时 `kill` 也要结束子进程（停止的会话仍持有 run 句柄） |
| `pty::tests::the_terminal_owns_the_processes_its_shell_starts` | shell 起的子进程从第一条指令起就在 job 里 |
| `pty::tests::a_start_that_cannot_own_the_shell_leaves_no_shell_running` | 建 job／归属／恢复三步分别注入失败后没有遗留进程，错误信息点名程序与失败原因 |
| `terminal_tests::stopping_a_terminal_ends_the_processes_its_shell_started` | 停止终端后握手子进程消失 |
| `terminal_tests::a_shell_that_exits_first_still_ends_its_tree_before_the_session_ends` | shell 先退出时，所属树消失**早于**状态结束 |
| `terminal_tests::closing_a_terminal_leaves_other_sessions_and_unrelated_processes_alone` | 相邻会话 PID 不变，无关同名 `cmd.exe` 存活 |

前端侧 `derivations.test.ts` 新增 4 条：两条钉住活动终端的 callout 多出子进程那一行、
两条钉住服务**没有**这一行且 `closeMechanics` 对终端不再提「优雅结束」。

按 #59 的 Testing Decisions，集成侧不重复：`tests/mvp_matrix.rs` 的文件头把进程安全阶梯
划归 `process::tests` 与各模块自己的套件，本片沿用该边界（§3 的矩阵表已加行）。

**第 3 层（视觉）。** §5 的 `capture-ui-states.mjs` 本轮跑不起来：它在 agent 沙箱里启动
Edge 时 `puppeteer.launch` 报 `Failed to launch the browser process: Code: 0`，而单独执行
`msedge.exe --headless=new --dump-dom about:blank` 也无输出；不通过 `--no-sandbox` 绕过
（该标志不由本轮引入）。改用 Browser 面板加载 `npm run dev` 的同一份前端，
`preview_eval`/`preview_screenshot` 逐区域比对
`assets/ui/ui-v2-terminal.png`：布局、侧栏、头部（名称/类型/状态/动作/用途）、页签、终端
面板、状态栏一致；唯一差异是本片有意新增的一行（已记入 `DESIGN_SPEC_EXTRACTED.md` §5 第 8 条）：
活动终端的「关闭影响」callout 在配置原文之下多一行
`停止该终端会同时结束它启动的子进程。`（12px/muted）。1280×800 与 900×640（抽屉态）都确认
该行不被裁切、不影响终端区域。服务会话核对过：callout 只有配置原文，没有这一行。

同一轮修掉一处**已有的**错误措辞：「详情 → 能不能关」卡片底下固定写着
`停止会尝试优雅结束；强制结束是单独动作…`，而终端按 D-018 根本没有优雅停止阶梯。该句改为
按会话类型取 `closeMechanics`（终端 = 与 callout 同一句，服务 = 原句），两处渲染「关闭影响」
的界面因此不再互相矛盾。核对了两个会话的详情页：终端是子进程那句，服务是原句。

**AC「PID 重用不能扩大结束范围」的证据边界。** 结束时只 `TerminateJobObject` 一个句柄，
实现里没有任何按 PID 查找或按镜像名匹配的路径，所以复用同一 PID 的新进程不在 job 里、
不会被碰到；本轮**没有**构造出「逼 Windows 复用某 PID」的场景（该现象不可按需复现），
不把结构性论证写成一次实测。可观察的那一半由
`closing_a_terminal_leaves_other_sessions_and_unrelated_processes_alone`（无关同名 `cmd.exe`
存活 + 相邻会话 PID/状态不变）与 `process::tests::force_stop_removes_the_managed_tree_but_not_an_unrelated_process`
覆盖。

**第 2 层（原生窗口）。** 本轮的 agent 没有在原生 Tauri 窗口里点过「停止」——本机
`preview_*` 只能到浏览器面板，原生窗口截不到也点不到，所以**不声称** H14 已按「真实窗口
点击 + 任务管理器」的方式通过。已有的替代证据是：进程层的三条握手/存活/失败清理断言在
真实 Windows 用户环境下运行，界面那一行在真实浏览器里核对了渲染。H14 剩下的
「在原生窗口点一次停止、看任务管理器」与 H05/H15 的生命周期部分，按 #59 地图的设计归
#69「完成完整日常流程的 Windows 原生验收」组合执行；本轮不为它补写截图或点击步骤。

---

### 2026-09-30 — #68 合并标题栏与统一 Hub 图标

环境：Windows 11 Pro（10.0.26300），worktree `elegant-kilby-2aaf97`（基 `bbd1c35`，其后
合并 main 的 #70 提交），`CARGO_TARGET_DIR` 复用主检出的 `target`。改动：
`decorations: false` + 标题栏里的窗口控制 + 显式窗口权限 + 图标源换成
`assets/brand/hub-mark.svg`（D-029）。

**自动检查，全绿**（下表是合并 main 之后的复跑；合并前本分支上的同批命令结果相同，只是
没有 #61 带来的那些用例）：

| 套件 | 结果 |
| --- | --- |
| `npm run check`、`npm run lint`、`npm run format:check`、`npm run build` | 通过 |
| `npm test` | 215 passed / 15 files（含新增 `src/state/window-controls.test.ts` 6 项） |
| `cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test` | 344 lib + 10 `tests/mvp_matrix.rs` passed / 0 failed；其中新增 3 条 release 守卫 |

**带宿主的前端行为（第 2 层，`t68-harness.html` + 桩宿主，跑完删除）。** 在一个浏览器页里
安装 `__TAURI_INTERNALS__`（其中 `invoke` / `transformCallback` / `plugin:event|listen`
由桩实现）并在 `/src/main.tsx` 之前注入 Tauri **自己那份** `drag.js`（从 cargo registry
的 `tauri-2.12.0/src/window/scripts/drag.js` 读入，`__TEMPLATE_os_name__` 换成
`'windows'`），然后用 headless Chrome 驱动。实测：

- 三个按钮各发一条命令，且只发自己那条：`plugin:window|minimize` /
  `toggle_maximize` / `close`；
- 最大化形态是**读数**：桩里把 `is_maximized` 置真并派发 `tauri://resize` 之后，第二个
  按钮的 `aria-label` 变成「还原窗口」，置假再派发又回到「最大化窗口」；
- 拖动区：在应用名、摘要、Hub 图标、标题栏空白处按下鼠标都发
  `plugin:window|start_dragging`，在最小化按钮上按下**什么都不发**（按钮不参与拖动）；
  双击应用名发 `plugin:window|internal_toggle_maximize`（Windows 的双击最大化语义）。

这一层证明的是**我们的标记 + Tauri 自己的 drag.js** 的接法（`data-tauri-drag-region="deep"`、
按钮不触发拖动），以及三个按钮接到哪条命令；它证明不了这些命令在真机上是否被权限放行、
窗口是否真的动了——那是下面未运行的部分。

**视觉（第 3 层，`scripts/capture-ui-states.mjs` 对交付 `dist/`）。** Edge headless 在本
会话启动失败（`Code: 0`），改用 `EDGE_PATH` 指向本机 Chrome 跑同一条链路，输出五张
1280×800 fixture 截图（`%TEMP%\lch-68-capture`）。标题栏逐区域比对参考图：左边
Hub 图标 + 应用名 + `UI 预览`，右边全局摘要 + 三个窗口按钮，**无 MAJOR 级差异**；
参考图的 `Ctrl K` 与 `✕` 后面的 `退出` 按 §5 的有意偏差不算缺陷。另用一个临时脚本
（跑完删除，不进仓库）在 960 / 720 / 480 / 375 四个宽度量了标题栏各元素的矩形：
会话列表按钮（8–40）与窗口按钮（右对齐、`flex: none`）**不重叠**、窗口按钮不出视口，
窄宽度下摘要隐藏、名字省略，终端区域宽度未被标题栏挤掉。
截图里浏览器没有宿主，所以三个按钮是 disabled 形态（`aria-disabled` 语义上是 `disabled`，
只是不响应点击）；启用态的外观另由上面那份桩宿主截图确认。

**图标。** `node scripts/generate-icon.mjs` + `npx tauri icon assets/brand/hub-mark-1024.png`
重新生成整套 `src-tauri/icons/`（`tauri icon` 顺带产出的 `android/`、`ios/` 已删除）；
生成的 `icon.png` 与 `icon.ico` 已**看图确认**是 Hub mark（深色圆角方块 + 绿点 + 三条列表行）。
`release.rs` 的 `the_icon_set_comes_from_the_one_hub_mark` 钉住「图标集只有一个来源」。

**补记（小尺寸清晰度，D-029 的 2026-09-30 补记）。** 用户反馈图标在真实尺寸下太糊，源文件
因此换成上游设计工程原件的 `public/favicon.svg`（16 单位网格、元素更粗、tile 满画布），
按同一套命令重新生成。核对方式与结果：

- **小尺寸可读性**：把旧、新两份 `32x32.png` 并排按 4 倍最近邻放大看（Chrome 截图，
  非仓库产物）。旧的 tile 只占画布 75%、描边与列表行在 32px 下已互相糊在一起；新的满画布
  tile 下绿点与三条列表行仍各自成形。这是**尺寸对照的观感证据**，不等于任务栏外观验收。
- **四角透明**：`node scripts/check-icon-alpha.mjs`（§2）——把交付的 `icon.png`、
  `32x32.png`、`64x64.png`、`icon.ico` 画进 canvas 再读像素，四角 alpha 分别是全 `0`、
  全 `3`（圆角抗锯齿，RGBA 为 `(0,0,0,3)`——黑，不是白）、全 `0`、全 `0`，退出码 0；
  tile 内侧一点是 `rgba(24,26,33,255)`，即设计里的内层面板 `#181a21`。四份产物都没有白底。
- **标题栏那颗 mark 放大到 30px**（1.5 倍，产品决定，`DESIGN_SPEC_EXTRACTED.md` §5 第 9 条）。
  它是同一个文件、同一套机制，只改了一处 CSS。实测（把 fixture 工作区按 5 个宽度加载后量
  标题栏各元素的矩形）：mark 恒为 30×30；960 时 `menu=null`（窄布局未启用），
  720 / 480 / 375 时有会话列表按钮，四个宽度下 mark 与名称、名称与窗口按钮、
  会话列表按钮与 mark **都不重叠**，窗口按钮不出视口，终端区域宽度分别是 999 / 679 / 720 /
  480 / 375（未被标题栏挤掉）。375 下名称省略成 `Local Console…`。
  放大前后的观感对照（20 / 24 / 28 / 30 四档并排，Chrome 截图，非仓库产物）确认 30px 在
  44px 栏内不拥塞。

仍归 #69 的原生部分不变：任务栏/托盘/快捷方式在真机上的外观与 16px 下的实际可辨认度，
以及 30px 的 mark 在真实窗口里的观感。

**未运行（native-only，本轮没有能力执行）：**

- W-1…W-6 全部：真实窗口的一层标题、拖动/贴靠/双击最大化手感、四边缩放、最小化/最大化/
  还原/关闭的实际效果、任务栏与托盘与开始菜单快捷方式的图标外观，以及 16/32px 下 mark 的
  可辨认度。原因同 §5 末节：worktree 会话挡掉 `powershell`，而原生窗口在本会话无法启动与
  截图（应用的配置路径没有覆盖开关）。**WebView 截图不构成这些项的通过证据**，
  逐项留给 #69 的原生验收与人在桌面上走 §4 的 W 清单。
- 「更新应用快捷方式图标」的可见结果：快捷方式图标取自 exe 内嵌资源（`icon.ico`，
  `RELEASE.md` §4.1 的 I-7），资源已随本次重生成，但快捷方式外观本身要装一次包才看得见，
  归 I-7。#63 那条日常 PowerShell 入口的新图标按本工单的验收条件在 #69 的组合验收里核对
  （该工单不在本轮范围内）。

### 2026-09-30 — #62 一键新建可交互的临时 PowerShell（D-031）

工单 #62（父规格 #59 的决策 4–6、14、16；前置 #61 已合并）。基线为合并 `origin/main` 后的
`aa8f4f5`（含 #61 与 #68），worktree `select-complete-ticket-a533fa`，`CARGO_TARGET_DIR`
未设置、使用 worktree 自己的 `target`。环境为 Windows 11 Pro（10.0.26300）、真实用户会话
`q9560`。所有终端证据都来自真实 ConPTY shell，不是 mock。

**先记录一次红/绿。** `a_temporary_terminal_writes_nothing_to_the_app_data_roots` 第一轮是红的：

```text
a temporary terminal wrote run metadata: ...\metadata exists
```

断言的原意是「临时终端不落盘」，而 `docs/LOGGING.md` §6 的规则是每个受管运行都有一条运行
记录（「『这次运行发生过』本身就是记录」）。红的结果说明我把「不落盘输出」读成了「不落盘
任何东西」。按规范改的是**断言与文档**（输出不落盘、配置不写、运行记录照写且 `log_file`
为空），不是实现；这条边界现在由用例名、`docs/DECISIONS.md` D-031 第 6 条一起说明。

**第 1 层（自动）。** 全绿：

| 套件 | 结果 |
| --- | --- |
| `npm run check`、`npm run lint`、`npm run format:check`、`npm run build` | 通过 |
| `npm test` | **231 passed / 15 files**（基线 209） |
| `cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test -- --test-threads=1` | **368 lib + 11 `tests/mvp_matrix.rs`，0 failed**（基线 341 + 10） |

本片新增的用例与它们各自钉住的行为：

| 用例 | 钉住的行为 |
| --- | --- |
| `session::temporary::tests`（12 条） | 装了 PowerShell 7 就用它、没有就用 Windows PowerShell、两个都没有时按名字报错；入口不给目录用主目录、给了就用它、目录不存在按该目录报错；身份唯一且可作日志目录名；带空格的程序路径被引用 |
| `core::tests` 的 4 条拒绝 | 机器没有 PowerShell / 目录不存在时不注册、不宣布；已配置会话不能从窗口删除；未知 id 的删除是 `unknown_session` |
| `temporary_tests::one_click_creates_a_running_terminal_in_the_entrys_directory` | 一次点击产生真实运行中的 shell，cwd 是入口给的目录，shell 是解析出的那一个，日志策略是 `off`/`none` |
| `temporary_tests::a_creation_is_announced_before_any_of_its_states` | `session-created` 先于该会话的第一条状态事件，且载荷里 `temporary` 为真 |
| `temporary_tests::every_click_creates_a_separate_terminal` | 两次点击是两个 id、两个名字、两个进程 |
| `temporary_tests::a_terminal_that_cannot_start_leaves_no_row` | 解析通过而启动失败时撤回注册表项，命令回答启动的失败，事件流里 created 与 removed 成对 |
| `temporary_tests::a_running_terminal_cannot_be_removed` | 运行中删除被拒（「stop it before removing it」），会话原样保留 |
| `only_a_settled_temporary_session_is_removable` | 删除门槛的整张表：只有 `stopped`/`exited` 可删，`error` 仅在**没有** run 时（启动失败）可删，其余一律不可；配置会话任何状态都不可。前端的 `availableActions.remove` 与它同表，由 `derivations.test.ts` 的「error 态不给删除」用例钉住 |
| `temporary_tests::an_ended_terminal_keeps_its_output_until_it_is_removed` | shell 退出后滚动缓冲仍在、行仍在；删除后 `snapshot` 为空、删后发布不再产生事件（迟到结果不复活） |
| `temporary_tests::a_temporary_terminal_persists_no_output_and_writes_no_config` | `logs/` 与 `config.yaml` 都没被创建，运行记录在但 `log_file` 为空，输出只在内存缓冲里 |
| 集成 `the_quick_entry_adds_a_terminal_on_top_of_a_loaded_workspace` | 真实配置文件 + 真实临时终端共存（3 配置 + 1 临时），打字与退出，删除后回到 3；配置文件字节不变；重新加载同一文件不恢复临时项 |
| 前端 `session-registry.test.ts` 的 membership（10 条） | 创建被并入视图并归入 `temporary` 组；状态事件跟得上；初始化期间的创建/删除与快照乱序合并；删除后迟到状态不复活；命令答复与事件重复不重复插入；停止后不再应用 |
| 前端 `derivations` / `types` 的 5 条 | 只有「临时 + 已结束」才给删除动作；临时项归入自己的组；`temporary` 缺省即配置会话；三类新载荷的守卫（含「状态载荷不会被读成删除」） |

**第 3 层（视觉）。** 本轮 agent **第一次真正驱动并截到原生窗口**（机制见下），因此比对用的是
真实应用的三张截图，而不是浏览器预览：空工作区、单击入口后的临时终端、以及运行中终端的
「更多操作」菜单。与 `assets/ui/ui-v2-terminal.png` 逐区域核对，布局、侧栏分组与 hint、头部
（名称 / 类型 / 状态徽标 / 动作）、页签、终端面板、状态栏一致。本片有四处不同，均已记入
`docs/DESIGN_SPEC_EXTRACTED.md` §5：

1. `TEMPORARY` 组（参考图本来就有这一组，此前只是 fixture，现在由真实临时终端产生）；
2. 侧栏底部入口文案改为 `新建 PowerShell`（规格 #59 决策 7 取消混合文案）；
3. 空工作区多了一个入口卡（参考图没有这一态）；
4. 活动中终端若没有配置的 `close_impact`，callout 直接使用 Hub 自己那句
   `停止该终端会同时结束它启动的子进程。`，不再先渲染一行 `关闭影响 —`——临时终端正是没有
   配置文字的会话，而 `—` 看起来像加载失败。

**第 2 层（原生窗口）。** 本轮以 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
启动 debug 构建（同一份 `npm run dev` 前端），用 WebView2 的 DevTools 通道连接真实窗口，
做真实 DOM 点击（`Input.dispatchMouseEvent`）与真实键盘事件（`Input.dispatchKeyEvent`）。
所以下面的结果都是「真实窗口 + 真实 IPC + 真实 ConPTY」的结果，但边界要说清楚：**输入来自
WebView2 的 DevTools 通道，不是 OS 级 `SendInput`**；托盘、任务栏、窗口装饰未参与；退出走的是
结束进程，不是托盘的「退出」。

| 条目 | 观测 | 结果 |
| --- | --- | --- |
| H03（已有工作区） | 点侧栏入口 → 新行 `PowerShell 1` 出现在新的 `TEMPORARY` 组、被选中、状态徽标 `Ready`、`ConPTY · interactive`、`cwd C:\Users\q9560`、`log buffer only`、活动元素是终端的文本框；配置里的 5 个会话原样 | 通过 |
| H03（空工作区，story 7） | 用 `sessions: []` 启动（用户配置先备份）→ 空工作区显示入口卡 → 点击后同样得到一个运行中的真实终端并聚焦 | 通过 |
| H05 输入/输出 | 真实按键 `Write-Host LCH62-REAL` → 终端渲染出 `LCH62-REAL` | 通过 |
| H05 Ctrl+C | `Start-Sleep -Seconds 300` 后按 Ctrl+C → 提示符回来，会话仍是 `connected`，随后 `Write-Host LCH62-AFTER` 正常执行 | 通过 |
| H05 视图尺寸 | `Emulation.setDeviceMetricsOverride` 改变视图 → 面板重新适配（125 → 102 → 125 列）并重新上报尺寸；PTY 侧 resize 由 `pty::tests` / `terminal_tests` 覆盖 | 部分（窗口尺寸的 OS 级拖动未做） |
| H05 切换保留 | 切到另一个会话再切回 → 滚动缓冲整段replay（`LCH62-REAL`、被打断的 sleep、`LCH62-AFTER`） | 通过 |
| H06 | 本机装有 PowerShell 7：新终端跑的是 `PowerShell 7.6.6`；无 PowerShell 7 的回退由 `temporary::tests` 与「两个都没有」的拒绝用例覆盖 | 部分（本机无法摘掉 pwsh） |
| H07 | 日志页显示 `Off`/`None`、`仅内存缓冲，不写磁盘`、`该会话不写磁盘日志（仅内存缓冲），因此没有伪造的空记录`；文件系统上 `logs/terminal-*` 不存在 | 通过 |
| H08 结束后保留 | `exit` 后行仍在、徽标 `Exited`、终端仍显示本次输出；停止（而非退出）时同样是可删除的结束态 | 通过 |
| H08 移除 | 「更多操作 → 移除临时终端」后行消失、列表回到 5 个配置会话、`TEMPORARY` 组消失；4 秒后再看没有复活 | 通过 |
| H08 真正退出 | 结束进程后重启同一构建 → 只有配置里的 5 个会话、没有 `TEMPORARY` 组；`config.yaml` 前后 sha256 一致（`11b4c073…`，mtime 仍是 9-29） | 通过 |
| 进程清理 | 进程被结束后没有遗留 pwsh（作业对象的 kill-on-close；系统里余下的 pwsh 父进程是 CLI agent 自己的） | 通过 |

**一次无法归因的观察（不记为缺陷，也不记为通过）。** 运行中途列表里多出两个我方脚本没有点击
过的临时终端（`PowerShell 3`/`PowerShell 4`）。随后用 10 次以上「一次点击 + 一次按键」的组合
都无法复现：一次点击始终只产生一个会话，终端拿到焦点后按键只进入终端，单独按 Enter 不产生
任何会话。窗口当时在用户桌面上可见，同一时段另一个会话（#60）也在用不同实例做单实例验收；
我没有找到脚本侧的成因，也没有证据指向应用侧，故留作未解释现象：**若再现，先确认是否还有
第二个驱动源**。

**未执行 / 留待。** H15（关闭窗口隐藏到托盘、从托盘恢复后继续交互）与 H16（标题栏与各处图标）
属于 #68/#69 的组合原生轮次；H14 的「在原生窗口点一次停止 + 任务管理器旁证」仍归 #69；
H04/H09–H13 属于后续工单（外部入口、添加应用、应用复用）。本轮的键盘与鼠标都来自 WebView2 的
DevTools 通道，因此**不声称**「用 OS 级输入在原生窗口里点过」。

**清理。** 临时终端随应用结束，用户配置先备份后原样还原（sha256 一致，备份已删），
`npm run dev` 的 dev server 已停。

### 2026-10-01 — #64 添加并复用 Hub 内显示的应用（D-032）

工单 #64（父规格 #59 的决策 7、8、10、15；前置 #60、#62 已合并）。基线为基线提交
`0f6bc62`（含 #60/#61/#62/#68），worktree `trusting-blackburn-627b38`。环境为 Windows 11 Pro
（10.0.26300）、真实用户会话 `q9560`。下面的自动结果里，涉及真实进程与 ConPTY 的部分跑在
这个 Windows 用户环境里，不是 mock。

**第 1 层（自动）。** 全绿：

| 套件 | 结果 |
| --- | --- |
| `npm run check`、`npm run lint`、`npm run format:check`、`npm run build` | 通过 |
| `npm test` | **244 passed / 17 files** |
| `cargo fmt --all --check`、`cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test` | **430 lib + 12 `tests/mvp_matrix.rs`，0 failed** |

本片新增 54 条用例（lib 41、集成 1、前端 12），它们各自钉住的行为：

| 用例 | 钉住的行为 |
| --- | --- |
| `config::save::tests`（17 条） | 缺文件时新建文档且只写用户填过的字段；追加保留原文字节、注释与顺序；条目缩进跟随文件自己的写法；空列表、仅注释文件、以及空工作区的 `sessions: []` 都能追加（后者只去掉那两个方括号，行内注释照留）；格式损坏、未知根键、**带内容的**行内列表、重复 id 各自被拒且原文一字不动；写前重核原文（并发修改被拒且不覆盖）；只读文件无法替换时报告 I/O 失败、原文与临时目录都不留痕；非文件路径同理；替换后不留临时文件 |
| `app::applications::tests`（14 条） | 新增应用同时进入注册表与文件、且**不启动**；可选字段按填写内容落盘；已有条目原样保留；id 撞名让号（注册表与文件两边都算）；文件里有而注册表没有的 id 也算占用；纯中文名落到 `app` 且 `name` 保持原样；必填三项缺失时按字段拒绝且**不写文件、不建行**；目录不存在按 `cwd` 报错且无副作用；配置损坏时拒绝并撤回注册（无幽灵行）；没有配置文件位置时拒绝；端口 0 沿用配置层的消息与字段；slug 与长度截断；同一 id 第二次写盘被拒 |
| `session::core::tests` 的 `activating_starts_…` / `activating_a_starting_…` / `activating_a_stopping_…` / `activating_an_ended_…` / `activating_an_unknown_…` | 未运行就启动一次；已运行或启动中返回**同一个** pid 与 run id；停止中拒绝（`invalid_transition`）且不创建任何运行；结束态可再次打开；未知 id 报 `unknown_session` |
| `two_opens_that_arrive_together_create_one_run` | 8 个线程同时打开同一个未运行会话：无一失败，恰好一个创建了运行，注册表里仍只有那一个会话（连跑 15 次稳定） |
| `a_registration_can_be_taken_back_before_it_is_announced` / `a_session_that_owns_a_run_is_not_taken_back` | 撤回注册不发布任何事件（没人被告知过它），持有一个 run 的会话不会被撤回 |
| `instance::protocol::tests` 的 2 条新增 | `openApplication` 请求携带会话 id、其帧形状固定，且不会被读成普通 `open` |
| 集成 `an_added_application_joins_a_real_config_and_opens_once` | 真实配置文件的追加式保存 + 真实加载链路重新读出 4 个会话；追加后原文前缀不变；保存时**已在运行**的会话 run id 与 pid 不变（保存不重启任何会话）；激活两次是**一个**真实进程 |
| 前端 `AddApplicationDialog.test.ts`（11 条） | 三个必填项与五个可选项；不出现「独立窗口」字样，只陈述「Hub 内显示」；`loggingFor` 的每种策略都是配置层接受的组合、未指定时不写 `logging:` 块；`errorPlacement` 把后端给字段名落在对应输入框上、没有字段或表单没有这个输入框时退回 banner（含 `logging.path` 只在外部日志策略下才落位） |
| 前端 `Sidebar.test.ts`（1 条） | 两个入口并存，且不再出现被决策 7 取消的混合文案 |

**一次偶发失败（不是本片引入，记录在案）。** 一次全量 `cargo test` 里
`process::tests::stop_ends_a_live_run_and_leaves_nothing_in_the_tree` 失败过一次；该用例与
本片改动无关（`process` 模块未被本片触碰），随后单独复跑 3 次与全量复跑 8 次全部通过，因此
记为该用例本身的偶发，而不是本片的结果。本片的断言不依赖进程数量或 PID 复用。

**第 3 层（视觉）。** 本片新增的是对话框表面，参考图里没有这一态，所以比对的是视觉语言
（`docs/UI_STYLE_GUIDE.md` §10）而不是逐区域复刻。做法与 #68 的第 2 层同源：在工程根放一个
**临时**的 `t64-harness.html`（跑完已删除），在 `/src/main.tsx` 之前装好
`window.__TAURI_INTERNALS__` 的桩，用 `preview_*` 工具在同一浏览器页里驱动并截图，1280×800。
桩实现的是真实契约形状的 `ping` / `list_session_configs` / `list_sessions` /
`get_config_report` / `activate_session` / `add_application`，`add_application` 会真的往页内
列表里加一条并派发 `session-created`。观察到的：

- 侧栏底部两个入口（`新建 PowerShell` / `添加应用`）与参考图底部的入口位一致，颜色为中性灰，
  没有用到生命周期色；
- 点开后对话框居中（1280 视口下 x=354、宽 562），遮罩压暗工作区，卡片用 `--card`、1px
  `--border`、12px 圆角，与侧栏卡片同一套 token；
- 路径、命令、端口、地址用 `--font-mono`，名称与用途/关闭影响用正文字体——对应规范里
  「monospace text for … compact technical metadata」；
- 提交后对话框关闭、新行 `SillyTavern` 出现在列表并**被选中**，头部显示
  `Service / Stopped / :8000 / cwd D:\Tools\SillyTavern / log on error`，桩收到的载荷是
  `{name, cwd, command, url, port}`——未填的可选项（用途、关闭影响、日志策略）确实没有被发送；
- 端口填 `abc` 时对话框不关闭，消息落在端口输入框下方；桩返回一个不带字段的错误时，红色
  banner 出现在表单底部，用户已填内容原样保留；
- 空工作区（`?empty=1`）分支同样能打开该对话框，位置与尺寸不变。

**这一层证明的边界。** 它证明的是**我们的标记、样式与前端接线**（哪个入口发哪条命令、字段怎么
映射成配置层词汇、失败怎么回到表单），以及一次真实的「保存 → 立刻出现在列表 → 选中」的界面
行为；**它证明不了 Rust 侧的保存语义、进程行为或原生窗口**——那些由第 1 层与本文件别处的原生
轮次负责。浏览器预览在无宿主时会走 fixture 工作区，那里的「添加应用」按设计只提示需要后端，
不发命令（与 `新建 PowerShell` 同一条预览规则）。

**第 2 层（原生窗口）：本片未运行。** H09 与 H10 需要真实窗口里真的保存一次、真的启动一份
进程、并在停止过程中再点一次，本轮没有驱动原生窗口，因此**不把 H09/H10 记为通过**，留给
#69 的组合原生轮次（与 #61/#62 留下的 H14/H15/H16 同一批次）。相应地也**没有**动过用户真实的
`%APPDATA%\LocalConsoleHub\config.yaml`：本片所有写盘都发生在临时目录里。

**未执行 / 留待。** H09（真实窗口里添加应用、重开 Hub 复用、制造保存失败或冲突）、H10 的
Hub 内显示部分（已在运行/启动中/停止中点击配置应用）、以及 `openApplication` 启动请求在真实
快捷方式下的端到端链路，都未在原生环境执行。外部入口（`#63`）与独立窗口（`#66`）本就不是
本片范围。

**清理。** 临时 harness 页已删除（`git status` 里不存在），`npm run dev` 的预览服务已停止，
测试用的临时目录由各用例自行删除。

---

## 7. 当前已知缺口与验收边界

- **托盘验收已有通过记录。** 2026-09-29 的独立 Windows 桌面轮次确认 R-1 至 R-8 通过，见 §6。
  这项证据只覆盖托盘清单，不代表 §4 的其他原生窗口与交互条目也已通过。历史上首次 agent 运行
  未执行第 2 层的记录仍保留在 §6，不被后来的人工结果覆盖。
- **终端 T-11 已获用户确认。** D-027 / #38 已允许两种会话类型配置 `purpose` / `close_impact`；
  2026-09-30 随 #57 获用户整体验收确认通过。本轮未新增原生截图或 agent 逐区域视觉比对，证据限制见 §6。
- **#61 的终端进程树行为已按第 1 层与界面渲染证据记录。** 原生窗口里的那一次「点停止 +
  任务管理器」未由 agent 执行，与 H05/H15 的生命周期部分一并归 #69 的组合原生验收；
  见 §6 的 2026-09-30 #61 记录。
- **#62 的临时终端已按第 1 层与原生窗口证据记录，输入通道要说清。** 2026-09-30 那一轮 agent
  通过 WebView2 的 DevTools 通道驱动了真实窗口（真实 IPC 与真实 ConPTY），因此 H03 与
  H05/H07/H08 的主体有了窗口内证据；但输入不是 OS 级 `SendInput`，托盘与任务栏未参与，
  H14/H15/H16 仍归 #69 的组合原生验收，且该轮出现过两个无法归因的额外临时终端（见 §6）。
- **#64 的保存与激活已有第 1 层与前端桩宿主证据，原生窗口部分未运行。** 2026-10-01 那一轮
  把安全追加、两半事务、id 派生、四种状态下的打开语义钉在自动用例里，并在带桩宿主的浏览器页
  里走通了「保存 → 列表出现 → 被选中」与两种失败呈现；但 H09/H10 需要在真实窗口里真的保存与
  真的启停，本轮未做，连同 `openApplication` 请求的真实快捷方式链路一并归 #69（见 §6）。
  用户真实的 `config.yaml` 本片从未写过。
- **标题栏与窗口尚无原生结果。** #68 换了窗口形态（无系统装饰）与图标身份，§4 的 W-1…W-6
  一条都没在真实窗口上跑过：带桩宿主的前端驱动只到命令名，fixture 截图只到浏览器里的布局。
  DWM 阴影/圆角、贴靠、四边缩放与任务栏/托盘/快捷方式图标必须由人在桌面上看（#69）。
- **其他原生手工结果按条目记录。** §4 的 L-11 及其他尚无结果的项目不能由协议测试、浏览器预览或自动化通过代替；
  应在真实 Windows 桌面执行后逐项记录通过、失败或未执行。工具可用性与用户确认范围按每轮实际记录判断（§5/§6）。
- **安装包验收仍有未完成部分。** 详见 [`RELEASE.md`](RELEASE.md) §4 / §5：I-7 图标外观、I-8 MSI 实际安装未完成，I-15 尚未执行；
  I-16 / I-17 中需要真实窗口和交互的部分仍未验证。已通过的 NSIS 文件与用户数据检查
  不能替代这些验收。
- **MVP 修复集成完成。** 2026-09-29 的 #52–#55 未完成状态仍是历史事实；2026-09-30 #52–#56
  已完成并进入同一版本，#57 自动检查通过且获用户整体验收确认。其完成不等同于全部正式发布门槛通过。
- 手工冒烟二进制不进 CI（会起真实进程、需要人敲回车）。要纳入 CI，需先解决 `supervise_smoke` 的交互步骤。
