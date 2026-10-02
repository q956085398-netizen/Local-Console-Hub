# 从受管名单移除应用

2026-10-02，在正常 Windows 用户环境，使用真实 Tauri/WebView2 和隔离配置完成验证；没有改动用户实际配置。

入口：选中已保存的应用 → 更多操作 → 从受管名单移除 → 确认移除。

只移除启动配置及当前列表记录，不删除应用文件或磁盘日志。运行中的应用必须先停止；不能确认进程树已结束的失败状态仍拒绝移除。配置无法安全修改或写入失败时保留名单记录。保留其他配置及注释；不支持安全编辑的 YAML 形态明确拒绝。

| 验证 | 结果 |
| --- | --- |
| 取消后名单和配置保留 | PASS |
| 运行中拒绝移除且配置不变 | PASS |
| 已结束的应用可以从真实窗口移除 | PASS |
| 其他配置及注释保持原样 | PASS |
| 重新启动后移除项仍不存在 | PASS |
| 用户实际配置未修改；测试进程已清理 | PASS |

[原生报告](acceptance/2026-10-02-application-removal/report.json) · [确认框](acceptance/2026-10-02-application-removal/confirmation.png) · [重启后](acceptance/2026-10-02-application-removal/after-restart.png)

复验：先用 `npm run tauri -- build --debug --no-bundle --features acceptance` 构建，再在正常 Windows 用户环境运行 `node scripts/native-acceptance.mjs src-tauri/target/debug/local-console-hub.exe --removal-only`。

前端 291 项测试通过；后端完整 Windows 套件当时 551 项通过、1 个测试辅助项忽略。最终进程句柄修复后专项 5 项通过，真实窗口流程通过。类型检查、构建、本次修改文件的 ESLint 检查通过。完整 ESLint 因既有 `.scratch` 嵌套工作目录触发多个 tsconfig 根而失败，未修改其全局配置。未发布安装包，未进行已安装版本验收。
