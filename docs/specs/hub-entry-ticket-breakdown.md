# Hub 启动与会话归集实施工单

日期：2026-09-30。来源：[完整规格 #59](https://github.com/q956085398-netizen/Local-Console-Hub/issues/59)。用户已确认拆分，10 张工单已发布；12 条直接依赖已设置为 GitHub 原生 blocking links。

所有工单均为 OPEN、ready-for-agent、未分配。已核对正文与本地发布文件一致，依赖与批准拆分一致；父议题 #59 的正文、标题、状态、标签及分配未改变。

每张功能工单交付可独立演示或验证的完整行为，包含对应回归与真实 Windows 验收。必要局部整理随行为实施，不另建纯底层接口工单。最终组合验收不代替各功能工单的验收。

| 原草案 | 发布工单 | 直接阻塞者 |
| --- | --- | --- |
| T01 | [#60：直接启动并复用同一个 Hub](https://github.com/q956085398-netizen/Local-Console-Hub/issues/60) | 无 |
| T02 | [#61：关闭终端时安全结束所属进程树](https://github.com/q956085398-netizen/Local-Console-Hub/issues/61) | 无 |
| T03 | [#62：一键新建可交互的临时 PowerShell](https://github.com/q956085398-netizen/Local-Console-Hub/issues/62) | [#61](https://github.com/q956085398-netizen/Local-Console-Hub/issues/61) |
| T04 | [#63：让日常 PowerShell 快捷方式进入 Hub](https://github.com/q956085398-netizen/Local-Console-Hub/issues/63) | [#60](https://github.com/q956085398-netizen/Local-Console-Hub/issues/60)、[#62](https://github.com/q956085398-netizen/Local-Console-Hub/issues/62) |
| T05 | [#64：添加并复用 Hub 内显示的应用](https://github.com/q956085398-netizen/Local-Console-Hub/issues/64) | [#60](https://github.com/q956085398-netizen/Local-Console-Hub/issues/60)、[#62](https://github.com/q956085398-netizen/Local-Console-Hub/issues/62) |
| T06 | [#65：保存并复用终端启动配置](https://github.com/q956085398-netizen/Local-Console-Hub/issues/65) | [#64](https://github.com/q956085398-netizen/Local-Console-Hub/issues/64) |
| T07 | [#66：支持自带控制台的独立窗口应用](https://github.com/q956085398-netizen/Local-Console-Hub/issues/66) | [#64](https://github.com/q956085398-netizen/Local-Console-Hub/issues/64) |
| T08 | [#67：关联并唤起 Hub 外已运行的应用](https://github.com/q956085398-netizen/Local-Console-Hub/issues/67) | [#66](https://github.com/q956085398-netizen/Local-Console-Hub/issues/66) |
| T09 | [#68：合并标题栏并统一 Hub 图标](https://github.com/q956085398-netizen/Local-Console-Hub/issues/68) | 无 |
| T10 | [#69：完成完整日常流程的 Windows 原生验收](https://github.com/q956085398-netizen/Local-Console-Hub/issues/69) | [#63](https://github.com/q956085398-netizen/Local-Console-Hub/issues/63)、[#65](https://github.com/q956085398-netizen/Local-Console-Hub/issues/65)、[#67](https://github.com/q956085398-netizen/Local-Console-Hub/issues/67)、[#68](https://github.com/q956085398-netizen/Local-Console-Hub/issues/68) |

## 可以立即开展

- [#60：直接启动并复用同一个 Hub](https://github.com/q956085398-netizen/Local-Console-Hub/issues/60)
- [#61：关闭终端时安全结束所属进程树](https://github.com/q956085398-netizen/Local-Console-Hub/issues/61)
- [#68：合并标题栏并统一 Hub 图标](https://github.com/q956085398-netizen/Local-Console-Hub/issues/68)

其它工单等待其原生阻塞者完成；ready-for-agent 表示规格完整，不表示阻塞条件已解除。认领前仍须检查实时状态、分配和依赖。

## 依赖理由

- #62 等 #61：新建终端先具备可靠归属和结束整个树的能力。
- #63 等 #60、#62：快捷方式需要单实例请求转交与真实临时终端创建。
- #64 等 #60、#62：复用请求协调、动态后台注册表和即时列表，配置应用使用同一操作边界。
- #65 等 #64：复用安全配置保存；#62 已是传递依赖，无需重复列出。
- #66 等 #64：在已可用添加应用链路上扩展展示方式和独立生命周期。
- #67 等 #66：先有已持有实例的激活边界，再扩展外部身份识别与选择。
- #68 无前置：现有会话即可验证标题栏和图标。
- #69 只列 #63、#65、#67、#68：覆盖其余传递依赖，不人为串行无关功能。

## 验收覆盖

- #60：H01、H02、H15 的普通恢复部分。
- #61：H05 的 Ctrl+C、H14 及 H15 的生命周期部分。
- #62：H03、H05、应用内 H06、H07 及 H08 的临时项部分。
- #63：H04、外部目录 H06、H17。
- #64：H09、Hub 内显示应用的 H10。
- #65：H08 保存配置及保存失败保护。
- #66：Hub 持有独立实例的 H10、H12、H13。
- #67：H11。
- #68：H16。
- #69：同一实际构建上完整 H01–H17 组合验证。

## 发布与实施边界

正文逐工单保存，保留原批准草案和发布核对记录，未将所有工单合并为一份正文。工单以 Parent 链接引用 #59，彼此依赖使用原生阻塞关系，未更改父议题。

本次完成工单发布与核对；未认领工单、未修改应用实现、未执行新增验收、未发布安装包、未进行 Git 提交。
