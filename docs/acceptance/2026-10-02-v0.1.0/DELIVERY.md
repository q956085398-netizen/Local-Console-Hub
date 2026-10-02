# v0.1.0 初版交付记录

日期：2026-10-02（香港时间）。源码以本记录所在初版发布提交为准。

## 交付内容

- Windows x64 当前用户 EXE 安装包和所有用户 MSI 安装包。
- [安装与使用教程](../../USER_GUIDE.md)、[初版发布说明](../../RELEASE_NOTES_v0.1.0.md)、[下一版端口监视计划](../../NEXT_VERSION.md)。
- 本次纳入已验收的添加/识别应用、保存与移除应用及日常验收脚本，保留历史证据。
- 清除 README 的过期草案与只读配置描述；临时目录、本机设置、安装包副本明确排除出版本控制和代码检查。

## 本轮检查

| 检查 | 执行环境 | 结果 |
| --- | --- | --- |
| TypeScript、Lint、Prettier、前端生产构建 | Windows 配置沙箱 | PASS |
| 前端测试 | Windows 配置沙箱 | 291 PASS |
| Rust 格式、Clippy 全目标 | Windows 配置沙箱 | PASS |
| Rust 完整串行测试 | 正常 Windows 用户 NEWNAME\q9560，沙箱外逐命令执行 | 552 单元 + 17 集成 PASS；1 测试辅助项忽略 |
| Tauri/WebView2/IPC/ConPTY 原生验收 | 正常 Windows 用户；私有配置；Debug acceptance 构建 | 27/27 PASS |
| 用户配置前后哈希与自有测试进程清理 | 原生报告 | 配置不变；15 场景清理记录 |
| 正式 release 构建，不启用 acceptance | Windows 配置沙箱 | PASS |
| WiX / NSIS 打包 | 正常 Windows 用户，沙箱外逐命令执行 | MSI + EXE PASS |
| 正式 EXE 的 PE 子系统 | 静态读取文件 | Windows GUI（2） |
| 本次新包实际安装/升级/卸载 | 本轮未执行 | NOT RUN |
| MSI 实际安装及安装版任务栏/托盘视觉 | 本轮未执行 | NOT RUN |

原生构建 SHA-256：`b5a5672c8d4d48689f564a0e2c3b73a3175019a8c692f83d80e17f3f8f876162`。
正式 EXE（完成 NSIS 打包标记后）SHA-256：`40bc0a5577ea8d078d857be2aa0b5e1a30e73a5d6afcc7a8895a1835aae09f73`。

原生验收执行在发布提交前的工作树上；[报告](native-report.json)记录基线提交、当时改动、脚本及二进制哈希。它是独立测试构建，不冒充安装包验收。正式包由同一产品源码无 acceptance 构建后打包。后续文档与 Git 整理不改变产品源码。历史人工确认保持原构建范围。

证据：[原生报告](native-report.json)、[原生日志](native-tests.log)、[打包日志](bundle.log)、[安装包清单](artifacts.json)、[SHA-256](SHA256SUMS.txt)。

构建有 Vite 主资源超过 500 kB 的提示及 MSVC 库创建提示，均未阻止构建，不据此声称已完成性能优化。安装包未签名。

## 同步与清理范围

GitHub 正常 Windows 用户在线认证成功，沙箱 401 属凭据上下文差异，无需用户重新登录。同步通过发布 PR、Windows CI 和 v0.1.0 Release 进行，不直接绕过 main 保护。

旧远端分支均对应已合并 PR；收尾删除旧分支。本地两份旧工作目录和旧分支已整理；未提交实验代码、scratch 取证、本机设置及本地 main 独有的旧验收记录先保存到 `.scratch/release-v0.1.0/` 的 ZIP 与完整 Git bundle，并验证关键文件哈希及 bundle。

唯一残留：`.claude/worktrees/cleanup-branches-worktrees-7c69b4` 已为空且解除 Git 注册，但被其它进程占用，暂不能删除。未修改访问权限，也未结束用户进程。备份仅本地保存，不发布本机设置；用户配置、日志和现有人工验收目录保留。长期设计规范及验收证据不作为“垃圾”删除。

## 下一版

优先端口监视：本机监听端口、占用进程、受管会话关联、启动冲突提示、低频刷新与查找。只是规划，本次不实现。
