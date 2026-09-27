# Local Console Hub — Agent 工作守则

本项目是 Windows 优先的本地服务/终端统一控制台（Tauri）。产品语义以 docs/ 下的文档为准：
PRODUCT_SPEC / MVP_IMPLEMENTATION_SPEC / EXECUTION_PLAN / UI_STYLE_GUIDE / LOGGING / DECISIONS / ROADMAP / DEVELOPMENT。

## 视觉能力规则（最高优先级）

主力模型（GLM-5.3）**没有视觉能力**，无法直接查看任何图片。所有"看图"需求必须通过
`ui-vision` MCP 服务器（由 GLM-5.3-Flash 视觉模型驱动）完成，禁止凭想象处理图像内容。

### R1 — UI 工单开工前
任何涉及 UI 的任务（新建/修改界面、组件、样式、布局）：

1. 若 `docs/DESIGN_SPEC_EXTRACTED.md` 不存在：依次调用 `analyze_design`
   （`image_path=assets/ui/ui-v2-service.png` 与 `assets/ui/ui-v2-terminal.png`），
   把两份结构化结果整理写入 `docs/DESIGN_SPEC_EXTRACTED.md` 并提交，
   作为可复用的设计规格缓存（之后的项目会话直接读它，不重复转译）；
2. 通读 `docs/DESIGN_SPEC_EXTRACTED.md` 与 `docs/UI_STYLE_GUIDE.md` 之后，才能开始写 UI 代码。

### R2 — UI 变更完成后
完成一个界面的实现或修改后：对运行中的应用截图，调用 `compare_ui`
（`screenshot_path=截图路径`，`reference_path=对应参考图`），按差异清单迭代，
直到结论不是 `VERDICT: MAJOR`。若当前环境确实无法截图，向用户明确说明原因，
不得静默跳过验收。

### R3 — 任意图像
用户提到或提供任何图片（报错截图、图标、参考素材）时，先用 `describe_image`
查看再回答，禁止猜测图片内容。

### R4 — 故障处理
`ui-vision` 调用失败（缺 Key / 模型名不对 / 网络问题）时，把具体错误报告给用户并等待处理；
严禁静默降级为"不看图直接写 UI"。

## 开发环境注意事项

- 开发服务器端口使用 **24120**（或同段不常用端口）。禁止使用 Tauri 默认端口 1420：
  它落在 Windows 的保留端口范围内，可能随机导致启动失败。
  修改端口时 `vite.config.ts` 与 `src-tauri/tauri.conf.json` 必须一起改。

## ui-vision 服务器配置

- 位置：`tools/vision-mcp/server.mjs`（零依赖，Node ≥ 18，已通过项目 `.mcp.json` 注册）
- 必需环境变量：`ZHIPU_API_KEY`（或 `UI_VISION_API_KEY`）
- 可选：`UI_VISION_MODEL`（默认 `glm-5.3-flash`）、`UI_VISION_BASE_URL`
  （默认智谱开放平台 `https://open.bigmodel.cn/api/paas/v4`；可指向
  `https://api.z.ai/api/paas/v4` 或本地 vLLM/Ollama 的 OpenAI 兼容端点）
- 排障与冒烟测试见 `tools/vision-mcp/README.md`
