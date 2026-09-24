# Local Console Hub

> 一个面向 Windows 本地应用、长期运行服务与交互式终端的统一控制台。

Local Console Hub 的目标不是再造一个普通终端模拟器，也不是只做一个“进程查看器”。它希望把 **dekit/mprocs 式的多会话管理**、**Launcher 式的按钮操作**、**可交互终端**、**服务状态与用途说明**、**有策略的日志管理** 和 **系统托盘常驻** 合并到一个轻量工具中。

当前仓库处于 **设计与 MVP 落地阶段**。

## 为什么需要它

在本地运行 SillyTavern、ComfyUI、KoboldCpp、各种 WebUI、测试 API、临时调试服务时，经常会出现大量 PowerShell / CMD / Terminal 窗口：

- 很难一眼看出“这个终端属于什么”；
- 不知道某个窗口能不能关、关闭后会影响什么；
- 服务型程序启动后其实只需要偶尔看日志，却长期占着任务栏；
- 有些终端只是输出日志，有些又必须允许用户输入命令；
- 日志容易散落，之后想找时不知道在哪里；
- 但也不应该为了“统一管理”而把每一次普通终端会话都强制落盘记录。

Local Console Hub 希望把这些问题收口到一个窗口中。

## 总体目标

一个 Hub 管理多个 **受管会话（Managed Session）**：

```text
Local Console Hub
├─ AI Apps
│  ├─ ● SillyTavern   :8000
│  ├─ ● ComfyUI       :8188 · Busy
│  └─ ● KoboldCpp     :5001
├─ Debug / Test
│  ├─ ○ Test API
│  └─ ● PowerShell · interactive
└─ Temporary
   └─ ○ Scratch
```

选中某个会话后，主区域可以展示：

- 状态、PID、运行时长、端口与 URL；
- 用途说明与“关闭影响”；
- 启动 / 停止 / 重启 / 打开网页 / 打开目录等操作；
- **可交互终端**：不仅能看输出，也能输入；
- 日志与历史运行记录（仅在策略允许时持久化）。

点窗口关闭按钮时默认收进系统托盘，而不是直接结束受管服务。

## 核心原则

### 1. 终端必须是可交互的

Local Console Hub 不是日志查看器。

对于需要交互的会话，终端必须支持：

- 键盘输入；
- Ctrl+C 等控制序列；
- PTY/ConPTY 语义；
- 终端尺寸变化；
- Unicode；
- 交互式 CLI、REPL、测试工具等。

“只读输出面板”只能作为日志视图存在，不能替代真正的交互式终端。

### 2. 服务与终端是不同的会话类型

长期运行的 WebUI / 本地服务和临时 shell 不应被当成完全相同的对象。

初期至少区分：

- **Service Session**：长期运行，重点是状态、端口、健康检查、日志与启停；
- **Interactive Terminal Session**：需要用户输入，重点是完整终端交互；
- 后续可扩展一次性任务、外部进程观察等类型。

### 3. 只管理“受管会话”，不全局劫持系统终端

目标是让 **由 Hub 创建或明确交给 Hub 管理的终端** 自动进入同一个窗口。

不计划在初期全局拦截所有 `cmd.exe` / `powershell.exe` / `pwsh.exe`：

- 系统、安装器和第三方程序可能会创建自己的控制台；
- AI Agent 拉起的各种后台终端通常不需要进入 Hub；
- 全局劫持会增加兼容性问题和不可预测行为。

因此默认边界是：**受管的进入 Hub，不受管的保持原样。**

### 4. 日志采用“按需持久化”，不是“开终端就写日志”

终端缓冲区、实时输出和持久化日志是三件不同的事。

默认策略应避免无意义的磁盘写入和隐私泄露：

- 普通交互式 shell：默认只保留内存滚动缓冲，不自动生成日志文件；
- 长期服务：可以按配置持久化 stdout/stderr；
- 已经自己写日志文件的应用：优先索引/关联原日志，而不是再复制一份；
- 调试/测试会话：可手动开启、按错误保存，或按会话保存；
- 用户输入默认不作为持久化日志记录。

详细规则见 [日志规范](docs/LOGGING.md)。

### 5. 信息必须能回答“它是谁、在干嘛、能不能关”

每个受管会话应尽可能拥有语义信息：

- 名称；
- 用途；
- 类型；
- 当前状态；
- 关闭影响；
- 端口 / URL；
- 依赖关系；
- 工作目录与启动命令；
- 日志位置。

目标不是让用户去猜 PID，而是直接看到“ComfyUI 正在生成任务，此时停止会中断当前任务”。

### 6. 轻量与低干扰

- Windows 优先；
- 空闲时低 CPU 占用；
- 不为了 UI 长期高频轮询；
- 支持系统托盘常驻；
- 关闭主窗口默认隐藏到托盘；
- 真正退出与“隐藏窗口”明确区分；
- 不要求所有受管程序都一直显示终端画面。

## 初期落地阶段（MVP）

MVP 的目标是先把最痛的“多个控制台散落在任务栏”问题解决，同时保证终端是真的可用。

### P0：基础骨架

- 读取本地会话配置；
- 左侧会话列表与状态；
- Service / Interactive Terminal 两类会话；
- 基本启动、停止、重启；
- 记录 PID 与进程树；
- 窗口关闭时收进托盘；
- 明确的真正退出入口。

### P1：可交互终端

- Windows ConPTY/等效 PTY 接入；
- stdout/stderr 实时显示；
- stdin 输入；
- resize；
- Ctrl+C 等控制操作；
- 会话切换时保留各自终端状态；
- 普通交互终端默认不落盘。

### P2：服务体验

- URL / 端口信息；
- “打开网页”“打开目录”等按钮；
- Starting / Running / Stopped / Error 等基础状态；
- 可选健康检查；
- 用途与关闭影响展示；
- 优雅停止优先，必要时再提供“强制结束进程树”。

### P3：日志与可追溯性

- 按策略决定是否生成持久化日志；
- 明确区分“终端缓冲”“Hub 捕获日志”“应用自身日志”；
- 有序目录与可搜索元数据；
- 日志轮转与保留策略；
- UI 中始终能看到某会话当前是否在记录、记录到哪里。

MVP 完成标准与后续阶段见 [Roadmap](docs/ROADMAP.md)。

## 后续规划

在 MVP 稳定后再逐步考虑：

- 会话依赖关系与启动顺序；
- Busy / Degraded / Unhealthy 等更丰富的状态；
- 启动组 / Workspace；
- 自动重启策略；
- CPU / 内存 / GPU 等资源信息；
- 日志全文搜索与过滤；
- 配置导入导出；
- 受管终端的快捷创建；
- 插件化健康检查与应用适配；
- 更完善的任务栏 / 托盘快捷控制；
- 可能的跨平台支持。

后续内容应坚持一个原则：**不要为了“功能全”牺牲轻量、可理解性和稳定性。**

## 配置方向（草案）

最终格式尚未冻结，但预期会类似：

```yaml
sessions:
  - id: sillytavern
    name: SillyTavern
    type: service
    cwd: D:/Tools/SillyTavern
    command: node server.js
    url: http://127.0.0.1:8000
    purpose: 聊天前端
    close_impact: 可停止；网页会失联
    logging:
      mode: auto

  - id: scratch
    name: Scratch PowerShell
    type: terminal
    shell: pwsh
    cwd: D:/Work
    purpose: 临时交互终端
    logging:
      mode: off
```

配置语义会优先于具体文件格式；格式在 MVP 实现验证后再冻结。

## 文档

- [产品与行为规范](docs/PRODUCT_SPEC.md)
- [MVP 实现规范](docs/MVP_IMPLEMENTATION_SPEC.md)
- [任务依赖与执行计划](docs/EXECUTION_PLAN.md)
- [V1 UI 制作规范](docs/UI_STYLE_GUIDE.md)
- [日志规范](docs/LOGGING.md)
- [Roadmap](docs/ROADMAP.md)
- [开发与贡献规范](docs/DEVELOPMENT.md)
- [关键设计决策](docs/DECISIONS.md)

## 项目边界

初期 **不以** 以下目标为重点：

- 替代 Windows Terminal 成为通用终端模拟器；
- 全局捕获所有系统新终端；
- 变成完整的系统进程管理器；
- 变成容器编排平台；
- 默认记录所有终端输入与输出；
- 为 AI Agent 的内部后台子进程提供强制可视化。

Local Console Hub 更关注的是：

> **让用户主动管理的本地服务和终端集中、可辨认、可交互、可控制、可追溯。**

## 状态

🚧 设计 / MVP 准备中。
