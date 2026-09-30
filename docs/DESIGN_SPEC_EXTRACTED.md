# V2 UI 设计规格转译缓存（DESIGN_SPEC_EXTRACTED）

> **用途**：`assets/ui/ui-v2-service.png`、`assets/ui/ui-v2-terminal.png` 的结构化文字
> 规格，让后续会话/协作者不必反复读图就能对齐 token 与结构。
> **权威性**：`docs/UI_STYLE_GUIDE.md` 的产品语义优先；本文档的 token 与结构
> 取自参考图的**上游源码**（见下），因此是精确值而非目测估值。
> **上游来源**：`E:\Grok-UI-Design\LocalConsoleHub` —— V2 预览图的真实来源工程
> （Tailwind v4 + shadcn 风格原型）。两张参考图即该工程 `screenshots/qa-home.png`、
> `qa-terminal.png` 的同源产物；下方 token 取自其构建后的样式表
> (`.vercel/output/static/assets/styles-*.css` 的 `:root`/`@theme` 段) 与组件源码
> (`src/components/hub/*`、`src/lib/hub/*`)。**该工程是设计来源，不是应用架构** ——
> 其 auth / database / deployment / preview-runtime 子系统一律不引入
> （UI_STYLE_GUIDE §12、T06 out-of-scope）。
> **生成记录**：首版转译由模型读图完成，含目测误差（把中性灰主按钮报成蓝色、并虚构了
> 图中不存在的 macOS 三点窗口按钮）。拿到上游源码后已按源码**重写**本文档 ——
> 颜色/字体/结构一律以源码为准；后续如需复核，直接看图并与上游源码交叉验证。

## 1. 设计 token（源码精确值）

### 1.1 表面与线条

| Token | 值 | 用途 |
| --- | --- | --- |
| `--color-background` | `#0b0c0f` | 窗口背景 |
| `--color-card` | `#121318` | 侧栏、详情卡片 |
| `--color-popover` | `#181a21` | 菜单、浮层卡片 |
| `--color-secondary` | `#1c1e26` | 选中行、次级按钮 |
| `--color-accent` | `#22242d` | 次级按钮 hover |
| `--color-terminal` | `#090a0d` | 终端面板 |
| `--color-border` | `rgba(236,236,232,.1)` | 分区分隔线 |
| `--color-input` | `rgba(236,236,232,.12)` | 输入框边框 |
| `--shadow-border` | `0 0 0 1px rgba(255,255,255,.08)` | 卡片 1px 描边阴影 |

### 1.2 文字与交互

| Token | 值 | 用途 |
| --- | --- | --- |
| `--color-foreground` | `#ecece8` | 主文字 |
| `--color-muted-foreground` | `#8f929c` | 次要文字 |
| `--color-primary` / `-foreground` | `#c5ccd6` / `#0b0c0f` | 主按钮（**中性灰，不是蓝色**） |
| `--color-destructive` | `#b85c5c` | 强制结束等破坏性动作 |
| 终端正文 | `#d7d8d4` | 输出行 |
| 终端输入行 | `#c5ccd6` | 回显的用户输入 |
| 终端提示符 | `#8fa4c7` | `PS D:\Work>` |

> **重点**：V2 是中性色交互面。**蓝色不用于按钮**；颜色一律留给生命周期语义
> （UI_STYLE_GUIDE §10）。

### 1.3 生命周期状态色

| Token | 值 | 语义 |
| --- | --- | --- |
| `--color-status-run` | `#6fba8a` | Running / Ready |
| `--color-status-busy` / `-warn` | `#c9b07a` | Busy / Starting / Stopping |
| `--color-status-err` | `#d27878` | Error |
| `--color-status-idle` | `#6d717b` | Stopped / Exited |
| `--color-impact` | `rgba(201,176,122,.14)` | 关闭影响 callout 底色 |

### 1.4 字体与排版

- `--font-sans`: `"IBM Plex Sans", "Segoe UI", ui-sans-serif, system-ui, sans-serif`
- `--font-mono`: `"IBM Plex Mono", "Cascadia Mono", ui-monospace, "SF Mono", Menlo, Consolas, monospace`
- 实现用 `@fontsource/ibm-plex-*` **本地打包**（OFL），不依赖字体 CDN。
- 字号阶梯（Tailwind 名 → px）：`text-lg` 18（会话名）/ `text-sm` 14（正文、按钮、Tab）/
  `text-xs` 12（侧栏计数、表格）/ `text-[11px]`（等宽元数据、分组标题、状态栏）/
  `text-[10px]`（行尾时长、徽标）。字重 500（medium）为主。
- 等宽用于：端口、类型徽标、头部元数据、状态栏、终端、运行历史。

### 1.5 形状与间距

- 圆角：卡片/终端面板/菜单 `12px`（`rounded-md`/`rounded-xl`）；按钮、行 `8px`；
  徽标 `999px`（全圆）。
- 侧栏宽 `17.5rem`（280px）；选中行无左竖条，仅背景 `--color-secondary`。
- 状态点 `8px` 圆；busy/starting/stopping 时带脉冲动画（2s）。
- 标题栏高 `44px`；Tabs 高 `40px`，激活态下划线 `1px` `--color-primary`（**不是 2px 蓝线**）；
  状态栏高 `32px`。

## 2. 结构

~~~text
窗口（#0b0c0f）
├── 标题栏（44px，底部 1px 边框）
│   ├── 左：HubMark 图标(30px，参考图是 20px——见 §5 第 9 条) + "Local Console Hub"(14/500)
│   └── 右：等宽 "N 运行 · M busy" + 窗口控制（最小化 / 最大化·还原 / 关闭）
├── 主体（水平两栏）
│   ├── 侧栏（280px，#121318，右 1px 边框）
│   │   ├── "受管会话"(14/500) + "N 运行 · M 忙碌 · T 会话"(12，次要色) + 新建按钮(32px 方)
│   │   ├── 搜索框（36px，放大镜图标，占位符 "搜索名称、端口、用途"）
│   │   ├── 分组（标题 11/大写/字距 .12em + 右侧 hint）
│   │   │   ├── AI Apps — 长期本地模型与 WebUI
│   │   │   ├── Debug / Test — 临时接口与调试壳
│   │   │   └── Temporary — 用完即走的终端
│   │   └── 底部："新建 PowerShell / 服务"
│   └── 工作区
│       ├── 头部（会话名 18/500 + 类型徽标 + 状态徽标 / 用途 / callout / 元数据行 / 按钮组）
│       ├── Tabs：终端 · 日志 · 详情
│       └── 内容区（padding 8px）；终端面板占满
└── 状态栏（32px，等宽 11）
    左 "Hub 常驻 · N/T 运行 · 输入默认不记录"；右 "关闭窗口 ≠ 停止服务"
~~~

## 3. 组件清单（源码文案）

- **会话行**：状态点 + 名称(14/500) + 右侧「运行时长」或「状态词」+ 元数据行
  `<:端口> · svc|tty · [busy] · [error] · [Always|On error|Manual]`；
  terminal 行为 `interactive · tty`。busy/error 词着色；**无装饰性应用图标**。
- **头部徽标**：类型徽标 `Service` / `Terminal`（描边）；状态徽标 = 状态点 + 词
  （`Ready` / `Busy` / `Running` / `Starting` / `Stopping` / `Stopped` / `Exited` / `Error`）。
  `busy` 优先于 `ready`。
- **头部按钮**：`启动`(主，中性灰) **或** `停止`(次级，二选一)；`重启`；`打开网页`
  （仅 service 且有 URL）；`目录`；`⋯` 更多菜单（打开网页/打开目录/聚焦终端/
  查看日志策略/复制路径/——/强制结束进程树）。
- **关闭影响 callout**：`关闭影响`(11/大写/字距 .12em/琥珀) + 配置里的 `close_impact` 原文；
  左 2px 琥珀竖线 + `--color-impact` 底。停止态改为 `上次错误`(红) 显示 `lastError`。
- **元数据行**：`PID <n>` · `port :<n>` · `up <时长>` · `cwd <路径>` · `log <值>`；
  等宽 11，label 用次要色 70% 透明，value 用主文字 80%。`up` 仅 running 时出现。
  `log` 值**全小写**：`buffer only`（source none）/ `external` / 小写模式词
  （`off` / `always` / `on error` / `manual`）。**与侧栏 chip 的句首大写刻意不同**
  （chip 为 `Always` / `On error` / `Manual`）：这一行是策略 token 条，不是标签条；
  参考图两张均可印证（`log always`、`log buffer only`），上游源码该行直接渲染
  `session.logging.mode` 原始值。
- **终端面板**：`#090a0d` + 1px 描边阴影；顶部条左 `ConPTY · interactive`（terminal）
  或 `PTY attached · stdin 可用`（service），右 `connected` / 状态词；正文等宽 12.5/行高 1.55；
  行类型着色：sys 次要色、in `#c5ccd6`、err 红、out `#d7d8d4`；
  running 时末尾一行提示符（terminal `PS <cwd>>`，service `<id> $`）+ 闪烁光标；
  **未运行时**覆盖一层 dim + 居中卡片「会话未运行 / 交互终端必须先启动进程。这不是只读
  日志面板。」+ `启动 <name>` 按钮。
  **注意**：源码中**没有** macOS 三点窗口按钮（早先视觉转译虚构了这一元素，已删除）。
- **日志面板**：策略卡片（徽标 `Capturing`|模式词 + 来源词 + `stdin 不记录`；
  一句话结论；当前日志文件路径；`打开日志`/`打开目录`/`复制路径`）+ 运行历史列表
  （`run-<id>` + 状态徽标 running|ok|error + 起始时间 + 路径/PID/exit）。off 会话显示
  「交互终端默认不产生磁盘日志，因此没有伪造的空记录。」
- **详情面板**：三段卡片 —— 「它是谁」（名称/用途）、「能不能关」（close_impact +
  「停止会尝试优雅结束；强制结束是单独动作，且只作用于本会话进程树。」）、
  「依赖」（依赖会话名 + 状态徽标）；末尾 label/value 行：类型、状态、PID、Run、端口、
  URL、工作目录、启动命令、日志模式、运行时长。

## 4. 状态语义

- 状态点：绿 Running / 琥珀 Busy·Starting·Stopping / 红 Error / 灰 Stopped·Exited。
- 类型徽标中性描边；生命周期徽标用状态色 15% 底 + 状态色文字。
- 悬停：行 → `rgba(28,30,38,.7)`；次级按钮 → `--color-accent`；幽灵按钮 → `--color-secondary`。
- 选中：行背景 `--color-secondary`（无左竖条）。

## 5. 与参考图的**有意偏差**（T06 #7 范围裁定）

1. **窗口控制已实现，`Ctrl K` / 设置 / 日志 / `退出` 仍不实现**：窗口控制 `— □ ✕`
   自 #68 起就在标题栏里——窗口改为无系统装饰（`tauri.conf.json` 的
   `decorations: false`），这三个按钮是窗口唯一的控制，位置与分量按参考图（D-029）。
   其余几项仍然不做：`Ctrl K` 需要一套全局搜索、全局设置/日志入口与 T06 明确禁止的
   重复全局 Logs 导航冲突、`退出` 的语义只有托盘一条路（D-006）。参考图里排在 `✕`
   后面的 `退出` 因此也不画。
2. **"UI 预览" 徽标**：参考图的预览标记，产品界面不携带；实现仅在检测不到 Rust 后端时
   于状态栏右侧显示一个等宽 `UI 预览` 提示（开发态）。
3. **行内日志标签 `Auto` → `External`**：`auto` 在到达前端前已被解析
   （`src/types/config.ts`、T05 契约），前端拿不到 `Auto`。对「应用自带日志」的会话，
   行内改显示 `External`，与头部 `log external` 一致。这是唯一一处与参考图文案的
   实际差异，属契约约束而非实现取舍。
4. **详情面板不重复头部元数据**：参考原型在「详情」里再列一遍 PID/端口/URL/工作目录/
   日志模式/运行时长，与头部的元数据行逐条重复；UI_STYLE_GUIDE §13 明确禁止
   Terminal / Logs / Details 之间过度重复，故实现只保留头部不承载的低频字段
   （启动命令、Run、退出码、PTY、内存缓冲），身份与路径并入「它是谁」卡片。
5. 参考图两处的计时字面值（`3h 23m` vs `3h 12m`、`23m 47s` vs `12m 5s`）在实现中由
   fixture 相对时间生成，时间点不同属正常。
6. **元数据行的 `on_error` 拼作 `on error`**：上游原型把原始枚举整值塞进该行，
   `log on_error` 会带着下划线出现在界面上（参考图未出现该会话，故无图可证）。实现改为
   小写模式词 `on error`，与同行手写值 `buffer only`、`external` 同级，不泄漏线上枚举值。
   其余模式（`off` / `always` / `manual`）两种写法本就一致。
7. **服务终端的连接条拼作 `PTY 未连接 · 只读缓冲`**（T07 #8 引入，T11 #12 记录）：
   服务参考图（`assets/ui/ui-v2-service.png`）的终端面板顶部写着 `PTY attached · stdin 可用`，
   但一个受管服务按定义没有可输入的 stdin（`session::core` 的 `terminal_write` 对服务直接
   拒绝），而 UI_STYLE_GUIDE §7 要求这个面「不得声称快照没有报告的连接」。`ptyChromeLabel`
   因此读运行快照的 `ptyAttached` 而不是会话类型：服务运行中显示 `PTY 未连接 · 只读缓冲`，
   交互终端显示 `ConPTY · interactive`（终端参考图即此文案，逐字一致）。
   参考图这一处是上游原型的示意值，不是可实现状态。
8. **活动中的交互终端，「关闭影响」callout 有两行**（#61 引入，D-028）：第一行仍是配置里的
   `close_impact` 原文（与终端参考图逐字一致），第二行是 Hub 自己的一句
   `停止该终端会同时结束它启动的子进程。`（会话类型为 terminal 时固定出现，服务没有）。
   上游原型只画了配置文字，但 UI_STYLE_GUIDE §5 要求这个 callout「说明停止该会话的实际
   后果」，而终端关闭会结束它启动的整棵进程树（D-028）恰恰是配置作者写不出来、用户在按下
   停止前必须知道的那一条。第二行的字号与颜色都比第一行轻（12/muted），保持参考图里
   「配置原文是主体」的层次。同一轮还改掉了「详情 → 能不能关」卡片底下那句固定的
   `停止会尝试优雅结束；…`：它对终端本来就是错的（终端没有优雅停止阶梯，D-018），现在
   按会话类型取 `closeMechanics`，终端显示与 callout 同一句，服务保持原句。该卡片不在
   参考图里（参考图只有服务态与终端态两个主视图），所以这不是与图的偏差，是措辞纠正。

9. **标题栏的 Hub 图标是 30px，参考图是 20px**（#68 收尾，D-029 补记）：用户在产品上过目
   之后要求放大到 1.5 倍，让它在应用名旁边读起来是**身份**而不是项目符号。参考图那颗是
   20px（`docs/DESIGN_SPEC_EXTRACTED.md` §2 原值），比对时按 30px 处理，**不要**当成缺陷。
   30px 仍留在 44px 标题栏里（上下各 7px 余量），且它是 `flex: none`：最窄的布局里也不会
   被窗口按钮挤到、不会盖住会话列表按钮（实测 960 / 720 / 480 / 375 四个宽度无重叠，
   `docs/VERIFICATION.md` §6）。
