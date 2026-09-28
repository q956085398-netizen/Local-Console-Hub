# Local Console Hub — Agent 工作守则

本项目是 Windows 优先的本地服务/终端统一控制台（Tauri）。产品语义以 docs/ 下的文档为准：
PRODUCT_SPEC / MVP_IMPLEMENTATION_SPEC / EXECUTION_PLAN / UI_STYLE_GUIDE / LOGGING / DECISIONS / ROADMAP / DEVELOPMENT。

## UI 参考图

主力模型自带原生视觉，直接读图即可，不要绕任何视觉转发服务。

- 服务会话参考图：`assets/ui/ui-v2-service.png`；交互终端参考图：`assets/ui/ui-v2-terminal.png`。
- `docs/UI_STYLE_GUIDE.md` 是规范文本；图文若有冲突，以它的产品语义为准。
- 开工前先看图再写 UI 代码，完工后核对实现与参考图的一致性。
- 用户提到或提供任何图片（报错截图、图标、参考素材）时直接查看，不要猜测图片内容。

## 开发环境注意事项

- 开发服务器端口使用 **24120**（或同段不常用端口）。禁止使用 Tauri 默认端口 1420：
  它落在 Windows 的保留端口范围内，可能随机导致启动失败。
  修改端口时 `vite.config.ts` 与 `src-tauri/tauri.conf.json` 必须一起改。

## Agent skills

本仓库的议题以 GitHub Issues 形式追踪（`q956085398-netizen/Local-Console-Hub`），使用 `gh` CLI。

- **Issue tracker**：见 `docs/agents/issue-tracker.md`。
- **Triage labels**：使用默认分诊标签，原样沿用：`needs-triage`、`needs-info`、`ready-for-agent`、`ready-for-human`、`wontfix`。见 `docs/agents/triage-labels.md`。
- **Domain docs**：单一上下文（single-context）：仓库根目录一个 `CONTEXT.md`，加上根目录 `docs/adr/`。见 `docs/agents/domain.md`。
