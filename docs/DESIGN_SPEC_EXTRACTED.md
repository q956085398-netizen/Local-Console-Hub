# V2 UI 设计规格转译缓存（DESIGN_SPEC_EXTRACTED）

> **用途**：`assets/ui/ui-v2-service.png` 与 `assets/ui/ui-v2-terminal.png` 的结构化文字
> 转译，供后续会话直接读取，避免重复调用视觉模型（CLAUDE.md R1 缓存）。
> **权威性**：以两张参考图与 `docs/UI_STYLE_GUIDE.md` 为准；本文档是它们的忠实转译，
> 冲突时以上游为准并回改本文档。
> **生成方式**：视觉模型逐图转译（两图各一次）。首次生成时 ui-vision 服务器返回
> HTTP 429（智谱余额不足，error code 1113），改由会话内另一视觉模型（4.5v）经 MCP
> 完成同等转译；充值 ui-vision 后可用 `tools/vision-mcp` 的 `analyze_design` 重新生成
> 校对。两图转译在同一窗口布局上高度一致，本文档已合并去重；个别两图估计值不同的
> token 以「(两图取…)」注明。

## 1. 共享规格（两图一致的窗口骨架）

### 1.1 布局树

~~~text
窗口（深色，约 1280×~720，2 栏）
├── 标题栏 TitleBar（全宽，高 ~44–48px，底部 1px 边框）
│   ├── 左：应用图标（20px 圆角方，#4C8DFF 终端提示符造型）+ "Local Console Hub"（13px/700）+ "UI 预览" 徽标
│   ├── 左末：会话数摘要（11px，次要色）
│   └── 右：运行摘要（等宽 11px）+ Ctrl K + 设置/日志 + 退出（红字）+ 窗口控制 — □ ✕
├── 主体（水平两栏）
│   ├── 侧栏 Sidebar（宽 ~280px，右边界 1px）
│   │   ├── 搜索框（"搜索名称、端口、用途…"）
│   │   ├── 分组 "SERVICES" / "TERMINALS"（大写小字标题 + 会话行）
│   │   └── 添加会话入口（虚线边框整宽按钮）
│   └── 工作区 Workspace（纵向）
│       ├── 会话头部 SessionHeader（~112px：名称行 / 用途 / 停止影响 callout / 元数据行 + 右侧按钮组）
│       ├── Tabs（Terminal / Logs / Details，底部 2px 蓝色激活下划线）
│       └── 内容区（终端/输出为主，占满剩余全部高度）
└── 底部状态栏 StatusBar（全宽，高 ~24–28px，顶部 1px 边框）
~~~

### 1.2 颜色 token（估计 hex，深色主题）

| Token | 值 | 用途 |
| --- | --- | --- |
| `--bg` | `#0B0E14` | 窗口/工作区背景 |
| `--bg-sidebar` | `#0E1117` | 侧栏背景 |
| `--bg-raised` | `#12161F` | 徽标/悬停行/输入框类表面 |
| `--bg-inset` | `#05070C` | 终端面板与 stdout 块（近黑） |
| `--bg-selected` | `#18243B` | 选中会话行（两图取 #18243B） |
| `--border` | `#1F2633` | 分区分隔线（标题栏底、侧栏右、Tabs 底、面板边） |
| `--border-strong` | `#2A3040` | 徽标/次级按钮/虚线按钮边框 |
| `--border-input` | `#232B3A` | 搜索框边框 |
| `--text-primary` | `#E5EAF3` | 主文字 |
| `--text-secondary` | `#8B93A7` | 次要文字、徽标文字 |
| `--text-muted` | `#6B7280` | 更弱文字（用途行、非激活 Tab） |
| `--text-faint` | `#4B5563` | 时间戳前缀等最弱文字 |
| `--accent` | `#3B82F6` | 选中态左侧竖条、Tabs 激活下划线、状态点蓝 |
| `--accent-strong` | `#2563EB` | 主按钮背景（hover `#1D4ED8`） |
| `--accent-icon` | `#4C8DFF` | 标题栏图标 |
| `--ok` | `#22C55E` | Running 状态点 |
| `--ok-text` | `#86EFAC` | RUNNING 徽标文字（bg `rgba(34,197,94,.15)`，border `rgba(34,197,94,.3)`） |
| `--warn` | `#F59E0B` | busy/Starting 状态点（两图取 `#F59E0B`；另一估计 `#FBBF24`） |
| `--warn-callout-border` | `#3A2E10` | 停止影响 callout 边框 |
| `--warn-callout-bg` | `#191307` | callout 背景 |
| `--warn-callout-text` | `#D97706` | callout 文字 |
| `--danger-border` | `#DC2626` | Stop 按钮边框 |
| `--danger-text` | `#F87171` | Stop 按钮/错误文字（另一估计 `#EF4444`） |
| `--stopped` | `#6B7280` | Stopped 灰点与徽标（bg `rgba(139,147,167,.12)`，border `rgba(139,147,167,.3)`） |

### 1.3 字体与排版

- UI 字体：系统栈（Segoe UI / Microsoft YaHei）。
- 等宽（Consolas / Cascadia Mono 栈）：端口、类型徽标、头部元数据行、状态栏、终端/stdout 内容、快捷键提示。
- 字号层级（估计）：会话名 15px/600；侧栏行名 12px；Tabs 12px/500；正文/按钮 11px；徽标/搜索/状态栏 10–10.5px；分组标题 10px 大写、字间距 ~0.08em。

### 1.4 形状与间距

- 圆角：徽标 4px；输入框/按钮/会话行/callout 6px；终端面板与 stdout 块 8px。
- 边框一律 1px（除 Tabs 激活下划线 2px）。
- 会话行高 ~40px，内边距 ~8px；选中行左侧 3px `--accent` 竖条 + `--bg-selected` 背景。
- 头部区域内边距约 20px×24px；元数据行与 callout 之间留 ~10px。
- 终端面板内边距 ~14px；状态点 8px 圆。

### 1.5 状态语义

- 状态点：绿 = Running；琥珀 = busy/Starting；灰 = Stopped；红 = Error（UI_STYLE_GUIDE §10）。
- 类型徽标：`SERVICE` / `TERMINAL`（等宽大写、中性配色）；生命周期徽标：`RUNNING`（绿）/ `STARTING`（琥珀）/ `STOPPED`（灰）。
- 悬停（推测）：行背景 → `--bg-raised`；按钮亮度略升。

## 2. Service 视图（ui-v2-service.png 差异部分）

- 侧栏分组：SERVICES（SillyTavern · 8000 绿点 / ComfyUI · 8188 琥珀点），TERMINALS（PowerShell / CMD 灰点）；每行第二行为 10px 用途文字。
- 头部（选中 SillyTavern）：
  - 名称行：`SillyTavern` + `SERVICE` + `RUNNING`；
  - 用途：`聊天前端`；
  - 停止影响 callout：⚠ `停止影响：SillyTavern 正在 8000 端口服务 — 网页会失联`；
  - 元数据行（等宽 10.5px）：`PID 12384 · 端口 8000 · 运行 2h 14m · 日志 captured/auto`；
  - 按钮组：`Open http://localhost:8000`（主按钮蓝）、`Restart`（次级）、`Stop`（红描边）、`⋯`（More）。
- Tabs 下内容区：整块 stdout 面板（`--bg-inset`、8px 圆角、1px 边框、内滚动），每行前缀灰色时间戳（`--text-faint`）+ 正文（约 `#9CA3AF`）。
- 底部状态栏：左 `● 端口 8000 正常`（绿点）`· 日志 captured → %LOCALAPPDATA%\LocalConsoleHub\logs\...`（等宽、截断）；右 `就绪`。

## 3. Terminal 视图（ui-v2-terminal.png 差异部分）

- 侧栏选中 `PowerShell`（TERMINALS 组，绿点）。
- 头部（选中 PowerShell）：
  - 名称行：`PowerShell` + `TERMINAL` + `RUNNING`；
  - 用途：`日常终端`；
  - 停止影响 callout：⚠ `停止影响：关闭将结束 PowerShell 会话 — 未保存的 shell 状态将丢失`；
  - 元数据行：`PID 15234 · 运行 12m 05s · 日志 off/none（交互会话默认不持久化）`；
  - 按钮组：`Restart`（次级）、`Stop`（红描边）、`⋯`（More）——**无 Open 按钮**（terminal 无 URL，context-appropriate）。
- 内容区：全高终端面板（`--bg-inset`、8px 圆角、1px 边框）；面板头部左上有三个 12px 圆点（`#FF5F57` / `#FEBC2E` / `#28C840`）+ 右侧 `已连接 · PTY`（10px 次要色）；内容为等宽终端文本（prompt `PS C:\Users\q9560>` 亮色，输出约 `#9CA3AF`）。
- 底部状态栏：左 `缓冲 512 行 · 丢弃 0 行 · stdin 未记录`（等宽 10px）；右 `就绪`。

## 4. 实现取舍（T06 #7 范围裁定）

参考图含以下元素，按工单 out-of-scope 与 UI_STYLE_GUIDE 裁定如下：

1. **顶部 设置 / 日志 / 退出 / Ctrl K**：T06 不实现 —— 工单明确禁止重复的全局 Logs 导航；
   全局设置与退出入口归后续工单（退出语义依赖 T09 托盘生命周期）。标题栏只保留
   应用身份 + 运行摘要。
2. **窗口控制 — □ ✕**：T06 保留操作系统原生标题栏（tauri 默认 decorations），不自绘
   窗口控制。X → 隐藏到托盘（D-006）是 T09 的交付物，届时再评估是否切 custom chrome。
3. **终端面板左上三个圆点**：纯装饰元素（无功能语义），为贴近 normative 参考图保留
   为 CSS 装饰；不承载任何交互。
4. **"UI 预览" 徽标**：参考图的预览标记，产品界面不携带。
5. 两图摘要文字不一致（"2 running · 1 busy" vs "4 运行 · 1 busy"）：以 AppSummary
   真实计数为准（`{running} 运行 · {busy} busy`），不复制任何一图的字面值。
