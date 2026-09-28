# Local Console Hub — Agent 工作守则

本项目是 Windows 优先的本地服务/终端统一控制台（Tauri）。产品语义以 docs/ 下的文档为准：
PRODUCT_SPEC / MVP_IMPLEMENTATION_SPEC / EXECUTION_PLAN / UI_STYLE_GUIDE / LOGGING / DECISIONS / ROADMAP / DEVELOPMENT。

## 视觉能力

主力模型自带多模态视觉，可以**直接查看图片**，不需要任何外接视觉服务。

- **UI 开工前**：直接读取 `assets/ui/ui-v2-service.png` 与 `assets/ui/ui-v2-terminal.png`，
  再读 `docs/UI_STYLE_GUIDE.md`，然后开始写 UI 代码。设计细节以图为准。
- **UI 变更完成后**：对运行中的应用截图，自行与对应参考图逐区域比对
  （布局 / 组件 / 颜色语义 / 间距 / 字号），按差异清单迭代，直到没有 MAJOR 级差异。
  若当前环境确实无法截图，向用户明确说明原因，不得静默跳过验收。
- **任意图像**：用户提到或提供任何图片（报错截图、图标、参考素材）时，先看图再回答，
  禁止猜测图片内容。

> 2026-09-28：原 `ui-vision` MCP 外接视觉方案（glm-5.3-flash）已停用——主力模型已具备原生视觉，
> 那层转译是多余的。`tools/vision-mcp/` 保留仅为历史参考，已不在 `.mcp.json` 中注册。
> 若将来主力模型换回无视觉能力的模型，在 `.mcp.json` 重新注册该 server 即可恢复。

## 开发环境注意事项

- 开发服务器端口使用 **24120**（或同段不常用端口）。禁止使用 Tauri 默认端口 1420：
  它落在 Windows 的保留端口范围内，可能随机导致启动失败。
  修改端口时 `vite.config.ts` 与 `src-tauri/tauri.conf.json` 必须一起改。

## Agent skills

本仓库的议题以 GitHub Issues 形式追踪（`q956085398-netizen/Local-Console-Hub`）。

- **Issue tracker**：本地 CLI 环境用 `gh` CLI；在 Claude Code 的沙箱 / VM 会话里 `gh` 不存在且无网络出口，
  此时改用 GitHub MCP 工具读写议题与 PR。见 `docs/agents/issue-tracker.md`。
- **Triage labels**：使用默认分诊标签，原样沿用：`needs-triage`、`needs-info`、`ready-for-agent`、`ready-for-human`、`wontfix`。见 `docs/agents/triage-labels.md`。
- **Domain docs**：单一上下文（single-context）：仓库根目录一个 `CONTEXT.md`，加上根目录 `docs/adr/`。见 `docs/agents/domain.md`。
