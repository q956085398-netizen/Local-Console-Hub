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
| **托盘隐藏/恢复不销毁 PTY** | **手工**（§4 托盘段）。自动侧只有它的两半：隐藏路径不碰 Session Core（`tray::tests::only_the_main_window_hides_on_close`），以及「没有视图挂着时终端照跑」（上一条） |

### 服务

| 矩阵行 | 自动覆盖 |
| --- | --- |
| 长驻服务能起来 | `session::core::tests::a_service_session_maps_to_a_supervised_process`、集成 `three_concurrent_sessions_hold_distinct_runs_and_processes` |
| stdout/stderr 可见 | `session::core::tests::logging::a_run_file_carries_both_streams_tagged` |
| 配置的端口 / URL 可见 | `a_running_service_reads_its_configured_port`、`a_running_service_whose_port_is_closed_reports_both_facts`、`a_service_with_no_port_is_never_probed`、`a_session_url_comes_from_the_session_that_owns_it` |
| 重启会等前一个进程退出 | `process::tests::restart_leaves_exactly_one_run_alive`、`terminal_tests::restarting_a_terminal_replaces_the_run_and_leaves_one_shell`、集成 `a_restart_starts_a_new_run_and_keeps_the_scrollback` |
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
| R-8 | 看任务栏与托盘图标 | 与应用图标一致（D-013） |

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

2. 逐区域比对（布局 / 组件 / 颜色语义 / 间距 / 字号）：标题栏、侧栏分组与行、
   选中会话头部（名称 / 类型 / 状态 / 动作 / 用途 / 关闭影响 / 元数据行）、页签、
   终端面板、状态栏。**MAJOR 级差异 = 拦住发布。**

3. 同时对照 `docs/UI_STYLE_GUIDE.md` §13 的发布验收条，尤其是：
   终端是主体区域、没有常驻右侧仪表盘、没有装饰性会话图标、没有重复的全局 Logs 入口、
   关闭影响在破坏性动作之前可见、Terminal / Logs / Details 不互相重复。

### 与参考图的有意偏差（**不是缺陷**）

`docs/DESIGN_SPEC_EXTRACTED.md` §5 记录了 7 条，比对时按「预期」处理，逐条如下：
不实现 `Ctrl K` / 全局设置 / 自定义窗口控制（保留原生装饰）；`UI 预览` 徽标只在检测不到
Rust 后端时出现；行内日志标签显示 `External` 而不是参考图的 `Auto`（`auto` 在到达前端前
已被解析）；详情面板不重复头部元数据；fixture 计时是相对的，字面值不同属正常；
`on_error` 拼作 `on error`；服务终端的连接条是 `PTY 未连接 · 只读缓冲` 而不是参考图的
`PTY attached · stdin 可用`（受管服务没有可输入的 stdin，UI_STYLE_GUIDE §7 禁止声称
快照没有报告的连接）。

### 本环境无法完成的部分，要说出来而不是跳过

原生窗口（真实 `tauri dev` 出来的那个窗口）在本仓库的 agent 会话里**没有截图能力**：
`preview_*` 工具只到浏览器面板，而面板在本会话被 worktree 隔离挡住。所以：

- **可以**用上面的脚本对 `dist/` 做完整的视觉比对（§6 记录的就是这么做的）；
- **不能**截图原生窗口本身。因此「原生窗口的窗口装饰、任务栏/托盘图标外观、
  托盘菜单的实际排版」这三项必须由人在桌面上看，归属 §4 的 R-8 与 R-3。

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
| **交互终端无法携带 `purpose` / `close_impact`**，因此真实窗口里的终端头部永远对不上 `ui-v2-terminal.png`（fixture 工作区能对上，因为它在编造这两个字段） | 单开 #38。这是 config semantics 变更（spec §18 禁止静默改），需要独立决策记录，不在 T11 内改 |
| T07 验收条件「UI 不得用只读/假终端路径顶替」在发版路径上不成立（一次失败的 ping 会把窗口永久留在预览工作区） | 本次会话内修复（#29），决策记录为 D-025 |

---

## 7. 已知缺口

- **#38** — 终端的 `purpose` / `close_impact`：见上表。
- **需要人眼的项**：§4 整份清单都要人在 Windows 桌面上执行。其中 agent 侧**完全无法**
  替代的是 R-3 / R-4 / R-8（托盘菜单、显示主窗口、图标）与 L-11（手动记录按钮）：
  它们依赖真实托盘与真实点击，而 agent 会话既点不到托盘也截不到原生窗口（§5 末节）。
- **#30** — `session/core.rs` 的 `terminal_tests` 没有像 T02 的套件那样加
  `#[cfg(all(test, windows))]`，所以在非 Windows 机器上 `cargo test` 编不过。CI 只在
  `windows-latest` 上跑，因此不阻塞发布，但套件对自己需要什么应当诚实。
- **#27** — CI 还没有拒绝被提交的冲突标记。
- 手工冒烟二进制不进 CI（会起真实进程、需要人敲回车）。要进 CI 的话，先解决
  `supervise_smoke` 的交互步骤。
