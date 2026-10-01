# #89 配置应用启动入口与 Windows 验收

日期：2026-10-02。配置应用入口使用 `local-console-hub.exe --open-app <id>`。
`id` 是 `config.yaml` 中的稳定配置身份，不是显示名称或命令；沿用配置 id 校验
（1–64 个 ASCII 字母、数字、下划线或连字符，字母或数字开头）。缺值、重复 id 参数、
未知参数以及与 `--new-terminal` / `--directory` 混用都会明确拒绝。

命令行转换为现有 `Request::OpenApplication`，冷启动及运行中转交都进入
`app::activation::open` / `SessionCore::activate`。Running 与 Starting 复用，
Stopping 拒绝，不存在的配置返回具体 id；不拼接为 shell 命令。
成功请求通过已有待处理选中机制保留冷启动时的目标，再通知窗口选中该应用。
普通打开及新建 PowerShell 的语义保持现有协议。

## 创建一个入口

使用已安装的新版 Hub 可执行文件；先创建存放快捷方式的目录，再执行：

```powershell
powershell -NoProfile -File scripts\create-application-shortcut.ps1 `
  -Shortcut "D:\Entries\ComfyUI.lnk" `
  -Hub "E:\Local Console Hub\local-console-hub.exe" `
  -ApplicationId comfyui
```

脚本创建点名的 `.lnk`，目标为 Hub，参数为 `--open-app comfyui`，图标引用
Hub 可执行文件的图标 0。已存在的入口拒绝覆盖，目录或可执行文件不存在时拒绝创建。
移除创建的 `.lnk` 即可撤销；现有普通 Hub / PowerShell 入口无需修改。

## 验证证据

基线 `f71bd4147647f1459419051557761ba9baaca3b9` 与已合并 #88 的
`5042c433b7444c2ebe437041e2082d2bf8438626` 文件树一致；本次证据针对该基线上的
#89 工作树修改，报告保留当时的提交身份与修改状态。

构建方式：`npm run tauri -- build --no-bundle --features acceptance`。
测试二进制 SHA-256：
`43bc8006951336f87ea45397f8deca4bad5483d1c648b50ae1f059319f6e2bc0`。
这是显式隔离数据根的验收版本，不是安装包或已安装版本验证。

正常 Windows 用户 `newname\q9560`，Windows 10.0.26300 x64，真实 Tauri / WebView2 / IPC：
`node scripts/native-acceptance.mjs src-tauri/target/release/local-console-hub.exe`。

| 项目 | 结果与证据 |
| --- | --- |
| 冷启动实际 `.lnk` | 通过：脚本生成入口启动 service，界面标题为 Save Service |
| 运行中两次点击同一入口 | 通过：两个请求退出码为 0，service PID 始终为 63248 |
| Hub 数量 | 通过：实际二进制进程仅 1 个，PID 63788 |
| 入口参数和图标 | 通过：`--open-app service`，图标引用本次 Hub exe 的 `,0` |
| 不存在的配置 | 通过：真实命名管道返回 `delivered: false`，消息包含 missing-saved-application，既有进程不变 |
| 配置保护 | 通过：隔离配置与原始 fixture 哈希一致，用户配置前后哈希不变 |
| 清理 | 通过：启动包装进程、实际 Hub PID 及 service PID 全部确认退出；其它原生场景清理也通过 |
| 原生回归 | 通过：共 13 项，包括真实终端输入、日志写入失败及五种配置诊断 |
| Starting / Stopping | Session Core 公共 activate 边界回归验证复用 / 拒绝；没有用 GUI 伪造状态来冒充实际快捷方式测试 |

完整报告：[report.json](acceptance/2026-10-02-application-shortcut/report.json)。
截图：[application-shortcut.png](acceptance/2026-10-02-application-shortcut/application-shortcut.png)。
最终 Rust 全量输出：[rust-tests.log](acceptance/2026-10-02-application-shortcut/rust-tests.log)。
最终 Windows Rust 全量回归：544 项单元测试及 17 项集成测试通过，1 项子进程 fixture 按既有定义忽略。`cargo clippy --all-targets -- -D warnings` 通过。
协议回归 25 项、启动焦点回归 8 项、前端 288 项通过；类型检查、正式源码及脚本 lint、
格式检查、前端和 Tauri 构建通过。`npm run lint` 全仓入口会受到现存 `.scratch` 中
另一份 tsconfig / WebView 缓存的影响；使用 `npx eslint src scripts --ignore-pattern '.scratch/**'` 验证正式代码。

首次直接 `cargo build --release --features acceptance` 的产物没有正确嵌入发布页面，
原生 IPC Origin 检查失败，不算通过证据；之后用 Tauri 构建入口解决。
原生脚本对默认终端关闭影响的旧 `—` 预期同步为现行的子进程树提示。

未运行：安装、任务栏及快捷方式图标视觉、托盘点击、#69 H01–H17 同一构建的组合验收。
#89 交付解除配置应用入口缺口，仍由 #69 补完整组合验收；不关闭 #59 或 #69，不发布安装包。
