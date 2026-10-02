# Local Console Hub

面向 Windows 的本地应用、服务与交互终端统一控制台。保存常用启动入口，查看输出、启停受管进程，关闭主窗口后留在托盘继续工作。

**当前版本：v0.1.1，Windows x64。** 修正首次使用界面：没有会话也直接显示完整侧栏和空工作区，无需先添加应用。验证范围见 [修正版说明](docs/RELEASE_NOTES_v0.1.1.md)；初版交付记录保留历史身份。

## 下载和开始使用

从 [GitHub Releases](https://github.com/q956085398-netizen/Local-Console-Hub/releases) 下载 EXE（当前用户安装）或 MSI（需要管理员权限）。两个包是同一版本，选择其中一个即可。

1. 安装后从开始菜单打开 **Local Console Hub**。
2. 主界面默认没有会话，可以保持为空；需要临时终端时点左下方 **新建 PowerShell**，输入 `Get-Date`。
3. 需要保存常用入口时点左下方 **添加应用**，选择应用目录与启动文件，确认命令和展示方式后保存；下次启动仍会保留。
4. 选中应用并启动；有网页地址时可直接打开网页。
5. 关闭主窗口会隐藏到托盘。真正结束 Hub 请使用托盘菜单的 **退出**。

[安装与使用教程](docs/USER_GUIDE.md) · [发布说明](docs/RELEASE_NOTES_v0.1.1.md) · [下一版：端口监视](docs/NEXT_VERSION.md)

## 初版功能

- 真实 PowerShell / CMD 交互终端：输入、Ctrl+C、调整尺寸，切换会话保留输出。
- 保存应用启动配置，选择目录/程序/日志文件，有限范围识别启动候选并预填。
- Hub 内部显示与应用独立窗口；独立应用默认自行管理生命周期，可显式交给 Hub 管理。
- 启动、停止、重启；受管停止只作用于所属进程树。
- 识别已在 Hub 外运行的应用，按确认结果关联或启动，避免盲目重复启动。
- 单实例：再次打开恢复已有窗口；支持新建终端与打开已保存应用的快捷方式。
- 临时终端可保存启动配置；已停止应用可从名单移除，应用文件和日志保留。
- 按需持久日志、关联应用自有日志、运行历史与托盘常驻。
- 配置端口的本机 TCP 可达性检查。系统监听端口列表与占用进程诊断列入下一版。

## 数据与使用边界

配置：`%APPDATA%\LocalConsoleHub\config.yaml`。日志和运行历史：`%LOCALAPPDATA%\LocalConsoleHub\`。用户数据与安装目录分开，升级前建议备份这两个目录。

普通交互终端默认不写磁盘日志，用户输入不持久化。Hub 只管理自己创建或用户明确选择管理的会话，不全局接管 Windows 终端。

安装包未签名；请核对发布页校验文件。缺少 WebView2 的机器在安装时需要联网获取运行时。安装向导仍为英文，应用主要界面为中文。

## 开发

Tauri 2 + React + TypeScript + Vite + Rust。需要 Windows、Node.js 22+、Rust stable（MSVC）、C++ 构建工具与 WebView2。

```powershell
npm ci
npm run tauri dev
npm run check
npm test
npm run lint
npm run format:check
npm run build
cargo fmt --all --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --manifest-path src-tauri/Cargo.toml -- -D warnings
```

完整 Rust 测试含真实 Windows 子进程、ConPTY 与进程树控制，应在正常 Windows 用户环境运行：

```powershell
cd src-tauri
cargo test -- --test-threads=1
```

正式安装包：`npm run tauri -- build`，输出 MSI 和 NSIS；正式包不启用 `acceptance`。原生自动验收使用单独测试构建，见 [原生验收](docs/NATIVE_AUTOMATION_ACCEPTANCE.md)。日常使用安装版，开发版可能带启动控制台。

## 开发文档

[产品规范](docs/PRODUCT_SPEC.md) · [MVP 规范](docs/MVP_IMPLEMENTATION_SPEC.md) · [UI 规范](docs/UI_STYLE_GUIDE.md) · [日志规范](docs/LOGGING.md) · [路线图](docs/ROADMAP.md) · [开发规范](docs/DEVELOPMENT.md) · [设计决策](docs/DECISIONS.md) · [验收记录](docs/VERIFICATION.md) · [打包与安装记录](docs/RELEASE.md)
