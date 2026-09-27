# ui-vision MCP 服务器

给没有视觉能力的主力模型（GLM-5.3）外接视觉模型（GLM-5.3-Flash）的零依赖 MCP stdio 服务器。
主力模型遇到"需要看图"的场景时，通过工具调用把图片交给视觉模型，拿回结构化文字结果 ——
整个过程由 CLAUDE.md 中的强制规则驱动，无需人工介入。

## 工具

| 工具 | 用途 |
| --- | --- |
| `analyze_design` | 把 UI 设计参考图转写成结构化设计规格（布局树 / 组件清单 / 颜色 hex / 字号 / 间距 / 状态语义） |
| `compare_ui` | 对比"实现截图 vs 设计参考图"，输出按严重程度排序的差异清单 + VERDICT |
| `describe_image` | 通用图像问答（报错截图 / 图标 / 任意图片） |

## 配置

环境变量：

| 变量 | 必填 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `ZHIPU_API_KEY`（或 `UI_VISION_API_KEY` / `ZAI_API_KEY`） | 是 | — | 视觉模型 API Key |
| `UI_VISION_MODEL` | 否 | `glm-5.3-flash` | 视觉模型 ID；若你的供应商模型名不同（如 ModelScope 带前缀 ID、本地推理自定义名），改这里 |
| `UI_VISION_BASE_URL` | 否 | `https://open.bigmodel.cn/api/paas/v4` | OpenAI 兼容端点；可指向 `https://api.z.ai/api/paas/v4` 或本地 vLLM / Ollama |

Windows 永久设置 Key（PowerShell，设置后需重启终端/Agent 才生效）：

```powershell
setx ZHIPU_API_KEY "你的key"
```

## 注册方式

项目根的 `.mcp.json` 已注册本服务器，Claude Code 打开本仓库时自动加载。
首次使用会请求一次项目级 MCP 服务器批准——选择"始终允许"，或预先在
`~/.claude/settings.json` 加入：

```json
{ "enabledMcpjsonServers": ["ui-vision"] }
```

之后从加载到调用完全无人工介入。

其他支持 MCP 的 agent（Cursor / Cline / 自研框架等）按各自方式把
`node tools/vision-mcp/server.mjs` 注册为 stdio MCP 服务器即可，
并把 CLAUDE.md 中的视觉规则抄进对应的项目指令文件（如 AGENTS.md）。

## 手动冒烟测试

只测协议握手（不消耗 token）：

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | node tools/vision-mcp/server.mjs
```

应返回一行包含 `"serverInfo":{"name":"ui-vision"` 的 JSON。

带图完整测试（真实调用视觉模型，消耗少量 token）：

```bash
( printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe_image","arguments":{"image_path":"assets/ui/ui-v2-service.png"}}}' ; sleep 60 ) \
  | node tools/vision-mcp/server.mjs
```

## 常见问题

- 报"模型不存在 / model not found"：你的账户或端点下视觉模型 ID 与默认值不同，
  设置 `UI_VISION_MODEL` 为正确 ID。
- 报 HTTP 401：Key 不对，或没设置 `ZHIPU_API_KEY`。
- Claude Code 里看不到 ui-vision 工具：运行 `/mcp` 检查服务器状态，确认项目级
  `.mcp.json` 已批准；服务器启动配置会打到 MCP 日志（stderr）里，可查 `[ui-vision]` 开头的行。
