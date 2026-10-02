# 发布前 GitHub CI 复验

首轮运行：[37000932772](https://github.com/q956085398-netizen/Local-Console-Hub/actions/runs/37000932772)，源码 `a74efaf`。

- 前端及静态检查通过，Rust 552 单元通过、1 辅助项忽略。
- 集成测试 16/17 通过；`the_quick_entry_adds_a_terminal_on_top_of_a_loaded_workspace` 等待临时 shell 退出超过 40 秒。
- 正常 Windows 用户环境单独复跑原测试通过；给打印命令加入 500 ms 可控延迟也通过，未在本机稳定复现 CI 失败。没有用这两次本机通过掩盖 CI 失败。

代码确认测试在观察到输出标记后，立即从另一次输入发送 `exit`，未同步到 PowerShell 的新提示符。输出标记早于下一次输入就绪是可能的时序原因；CI 旧日志没有 shell 转录，不能证明具体是哪一层丢失输入。

修复仅改测试命令为“打印标记；exit”，由真实 shell 顺序执行。保留 stdin 执行、自然退出、结束后输出可读、临时项移除及配置未写入全部断言；没有调大超时、跳过测试或改动产品源码。完整 17 项集成测试在正常 Windows 用户环境复验通过，随后提交 GitHub Windows CI 复验。最终状态以 [PR #91 检查](https://github.com/q956085398-netizen/Local-Console-Hub/pull/91/checks) 为准。

产品源码及安装包与首轮交付相同，不因这次测试与文档修改重新打包。正式包哈希保持原记录。
