# 议题追踪（Issue Tracker）

本仓库的议题（issues）与规格（specs）以 GitHub issues 形式存在。所有操作使用 `gh` CLI。

当前仓库：`q956085398-netizen/Local-Console-Hub`（从 `git remote -v` 推断；在克隆内运行时 `gh` 会自动识别）。

## 约定（Conventions）

- **创建议题**：`gh issue create --title "..." --body "..."`。多行正文使用 heredoc。
- **读取议题**：`gh issue view <number> --comments`，用 `jq` 过滤评论并同时获取标签。
- **列出议题**：`gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`，配合适当的 `--label` 与 `--state` 过滤器。
- **评论议题**：`gh issue comment <number> --body "..."`
- **添加 / 移除标签**：`gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **关闭**：`gh issue close <number> --comment "..."`

## Pull requests 作为分诊面（request surface）

**PRs as a request surface: no.**（如果本仓库将外部 PR 视为功能请求，把 `no` 改为 `yes`；`/triage` 会读取此标志。）

设为 `yes` 时，PR 与议题使用相同的标签和状态，命令换成对应的 `gh pr ...`。GitHub 的议题与 PR 共享同一编号空间，所以裸的 `#42` 可能是二者之一：先用 `gh pr view 42` 解析，失败再回退到 `gh issue view 42`。

## 当技能说「发布到议题追踪器」（publish to the issue tracker）

创建一个 GitHub issue。

## 当技能说「获取相关工单」（fetch the relevant ticket）

运行 `gh issue view <number> --comments`。

## Wayfinder 操作

供 `/wayfinder` 使用。**地图（map）**是单个 issue，**子（child）** issues 作为工单。

- **Map（地图）**：单个带 `wayfinder:map` 标签的 issue，承载 Notes / Decisions-so-far / Fog 正文。`gh issue create --label wayfinder:map`。
- **子工单**：作为 GitHub sub-issue 链接到地图（对 sub-issues 端点调用 `gh api`）。在未启用 sub-issues 的地方，把子项加入地图正文的任务列表（task list），并在子工单正文顶部写 `Part of #<map>`。标签：`wayfinder:<type>`（`research`/`prototype`/`grilling`/`task`）。一旦被认领，工单分配给驱动的开发者。
- **Blocking（阻塞）**：GitHub 的**原生议题依赖**（native issue dependencies），这是规范且在 UI 中可见的表示。添加一条边：`gh api --method POST repos/<owner>/<repo>/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`，其中 `<blocker-db-id>` 是阻塞方的数字 **database id**（`gh api repos/<owner>/<repo>/issues/<n> --jq .id`），_而不是_ `#number` 或 `node_id`。GitHub 通过 `issue_dependencies_summary.blocked_by` 报告（仅统计开放状态的阻塞者，即实时门槛）。在依赖功能不可用时，回退为在子工单正文顶部写 `Blocked by: #<n>, #<n>`。所有阻塞者都关闭后，工单即解除阻塞。
- **Frontier query（前沿查询）**：列出地图的开放子工单，剔除仍有开放阻塞者或已有 assignee 的；按地图顺序取第一个。
- **Claim（认领）**：`gh issue edit <n> --add-assignee @me`，这是会话的第一次写入。
- **Resolve（解决）**：`gh issue comment <n> --body "<answer>"`，然后 `gh issue close <n>`，最后把上下文指针（gist + 链接）追加到地图的 Decisions-so-far。
