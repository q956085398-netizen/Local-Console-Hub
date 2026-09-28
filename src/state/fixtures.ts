/**
 * Fixture workspace data for the V2 UI shell (T06 #7).
 *
 * This is the workspace the app renders when no backend is answering — the
 * browser preview, and the moment before the first `list_session_configs`
 * returns. When a backend *is* answering, T07 (#8) renders its sessions
 * instead (`src/app/useSessionRegistry.ts`), and the terminal attaches to the
 * real PTY rather than showing these preview lines.
 *
 * Every config/runtime/run value here passes the landed DTO guards, so the two
 * sources are the same model and the shell renders either one: what changes is
 * where the data came from, not what it is. The cast, wording and buffer
 * content mirror the approved V2 prototype's demo data
 * (E:/Grok-UI-Design/LocalConsoleHub, the source of the reference screenshots)
 * so the shell can be compared against them visually.
 *
 * One deliberate departure from the reference prototype's demo data: its
 * service rows are labelled "PTY attached · stdin 可用", which no supervised
 * service can be — Session Core sets `pty_attached` only for a terminal run it
 * actually attached, and refuses input for a service outright. Those fixtures
 * therefore report `ptyAttached: false` (T07 #8), so the preview does not
 * repeat a claim the product cannot produce.
 *
 * Timestamps are minted relative to module load so uptimes stay plausible
 * (3h 12m, 2h 14m, 12m …) without becoming clock fixtures.
 */

import type { SessionView, WorkloadGroup } from "./session-view";

function iso(relativeMs: number): string {
  return new Date(Date.now() - relativeMs).toISOString().replace(/\.\d{3}Z$/, "Z");
}

const H = 3600 * 1000;
const M = 60 * 1000;
const DAY = 24 * H;

export const FIXTURE_GROUPS: readonly WorkloadGroup[] = [
  { id: "ai", label: "AI Apps", hint: "长期本地模型与 WebUI" },
  { id: "debug", label: "Debug / Test", hint: "临时接口与调试壳" },
  { id: "temp", label: "Temporary", hint: "用完即走的终端" },
];

export const FIXTURE_SESSIONS: SessionView[] = [
  {
    group: "ai",
    config: {
      id: "sillytavern",
      name: "SillyTavern",
      sessionType: "service",
      cwd: "D:\\Tools\\SillyTavern",
      command: "node server.js",
      url: "http://127.0.0.1:8000",
      port: 8000,
      purpose: "聊天前端，连接本地模型后端。",
      closeImpact: "可停止；打开的聊天页会失联，进行中的对话不会写入。",
      // auto resolved: the app owns its log, so the Hub captures nothing.
      logging: {
        mode: "off",
        source: "external",
        externalPath: "D:\\Tools\\SillyTavern\\data\\access.log",
      },
    },
    runtime: {
      sessionId: "sillytavern",
      status: "running",
      pid: 18420,
      runId: "a91c",
      startedAt: iso(3 * H + 12 * M),
      ptyAttached: false,
      logging: {
        mode: "off",
        source: "external",
        external_path: "D:\\Tools\\SillyTavern\\data\\access.log",
      },
      buffer: { bytes: 3120, lines: 8, droppedBytes: 0 },
    },
    runs: [
      {
        runId: "a91c",
        sessionId: "sillytavern",
        startedAt: iso(3 * H + 12 * M),
        pid: 18420,
        logMode: "off",
        logSource: "external",
        logFile: "D:\\Tools\\SillyTavern\\data\\access.log",
      },
    ],
    ready: true,
    dependsOn: ["koboldcpp"],
    lines: [
      { kind: "sys", text: "Hub 启动会话 sillytavern · run a91c" },
      { kind: "out", text: "SillyTavern 1.12.14" },
      { kind: "out", text: "Node v22.11.0" },
      { kind: "out", text: "Using config: D:\\Tools\\SillyTavern\\config.yaml" },
      { kind: "out", text: "Listening on http://127.0.0.1:8000" },
      { kind: "out", text: "Auto-connect: koboldcpp @ :5001  ready" },
      { kind: "out", text: "Extensions loaded: smart-context, websearch" },
      { kind: "sys", text: "端口就绪 · 进程存活。服务可用，不等于模型空闲。" },
    ],
  },
  {
    group: "ai",
    config: {
      id: "comfyui",
      name: "ComfyUI",
      sessionType: "service",
      cwd: "D:\\Tools\\ComfyUI",
      command: "python main.py --listen 127.0.0.1 --port 8188",
      url: "http://127.0.0.1:8188",
      port: 8188,
      purpose: "图像生成后端，队列里经常会有长时间任务。",
      closeImpact: "会中断当前生成；队列中的 prompt 将丢失。",
      logging: { mode: "always", source: "captured" },
    },
    runtime: {
      sessionId: "comfyui",
      status: "running",
      pid: 19002,
      runId: "c8aa",
      startedAt: iso(2 * H + 14 * M),
      ptyAttached: false,
      logging: { mode: "always", source: "captured", external_path: undefined },
      buffer: { bytes: 24576, lines: 14, droppedBytes: 0 },
    },
    runs: [
      {
        runId: "c711",
        sessionId: "comfyui",
        startedAt: iso(26 * H),
        endedAt: iso(18 * H),
        exitCode: 0,
        pid: 15402,
        logMode: "always",
        logSource: "captured",
        logFile:
          "%LOCALAPPDATA%\\LocalConsoleHub\\logs\\comfyui\\2026-09\\2026-09-26_02-21-00__run-c711.log",
        // The row state retention creates and the tab has to render: the run
        // is in the history, the log it names was swept (D-022). The fixture
        // carries it because there is otherwise no way to look at this row
        // without a live backend and a real sweep.
        logFilePresent: false,
      },
      {
        runId: "c8aa",
        sessionId: "comfyui",
        startedAt: iso(2 * H + 14 * M),
        pid: 19002,
        logMode: "always",
        logSource: "captured",
        logFile:
          "%LOCALAPPDATA%\\LocalConsoleHub\\logs\\comfyui\\2026-09\\2026-09-27_02-07-00__run-c8aa.log",
      },
    ],
    busy: true,
    ready: true,
    lines: [
      { kind: "sys", text: "Hub 启动会话 comfyui · run c8aa" },
      { kind: "out", text: "Total VRAM 24564 MB, total RAM 65241 MB" },
      { kind: "out", text: "pytorch version: 2.5.1+cu124" },
      { kind: "out", text: "Starting server" },
      { kind: "out", text: "To see the GUI go to: http://127.0.0.1:8188" },
      { kind: "out", text: "got prompt" },
      { kind: "out", text: "  20%|████                | 4/20  0.41s/it" },
      { kind: "out", text: "  55%|███████████         | 11/20  0.39s/it" },
      { kind: "out", text: " 100%|████████████████████| 20/20  8.14s" },
      { kind: "out", text: "Prompt executed in 8.14 seconds" },
      { kind: "out", text: "got prompt" },
      { kind: "out", text: "  40%|████████            | 8/20  0.38s/it" },
      { kind: "sys", text: "当前 Busy · 停止会丢掉这个 prompt。" },
    ],
  },
  {
    group: "ai",
    config: {
      id: "koboldcpp",
      name: "KoboldCpp",
      sessionType: "service",
      cwd: "D:\\Tools\\KoboldCpp",
      command: "koboldcpp.exe --model Qwen2.5-32B.gguf --port 5001",
      url: "http://127.0.0.1:5001",
      port: 5001,
      purpose: "本地 LLM 推理服务，给 SillyTavern 提供 API。",
      closeImpact: "SillyTavern 将无法生成回复，直到本服务再次就绪。",
      logging: { mode: "on_error", source: "captured" },
    },
    runtime: {
      sessionId: "koboldcpp",
      status: "running",
      pid: 17611,
      runId: "k02e",
      startedAt: iso(H + 4 * M),
      ptyAttached: false,
      logging: { mode: "on_error", source: "captured", external_path: undefined },
      buffer: { bytes: 7340, lines: 7, droppedBytes: 0 },
    },
    runs: [
      {
        runId: "k019",
        sessionId: "koboldcpp",
        startedAt: iso(2 * DAY),
        endedAt: iso(2 * DAY - 40 * M),
        exitCode: 1,
        pid: 14110,
        logMode: "on_error",
        logSource: "captured",
        logFile:
          "%LOCALAPPDATA%\\LocalConsoleHub\\logs\\koboldcpp\\2026-09\\2026-09-25_03-40-00__run-k019.log",
      },
      {
        runId: "k02e",
        sessionId: "koboldcpp",
        startedAt: iso(H + 4 * M),
        pid: 17611,
        logMode: "on_error",
        logSource: "captured",
      },
    ],
    ready: true,
    lines: [
      { kind: "sys", text: "Hub 启动会话 koboldcpp · run k02e" },
      { kind: "out", text: "KoboldCpp v1.82.4" },
      { kind: "out", text: "Loading model: Qwen2.5-32B-Instruct-Q4_K_M.gguf" },
      { kind: "out", text: "llama_model_load: n_ctx = 16384" },
      { kind: "out", text: "Load time: 4120 ms" },
      { kind: "out", text: "Embedded Kobold Lite UI: http://127.0.0.1:5001" },
      { kind: "out", text: "API ready. Idle." },
    ],
  },
  {
    group: "debug",
    config: {
      id: "test-api",
      name: "Test API",
      sessionType: "service",
      cwd: "D:\\Work\\sandbox\\test-api",
      command: "uv run uvicorn app:app --port 7788",
      url: "http://127.0.0.1:7788",
      port: 7788,
      purpose: "本地调试用的临时 HTTP 接口。",
      closeImpact: "只影响当前调试客户端，无持久数据。",
      logging: { mode: "on_error", source: "captured" },
    },
    runtime: {
      sessionId: "test-api",
      status: "stopped",
      ptyAttached: false,
      logging: { mode: "on_error", source: "captured", external_path: undefined },
      buffer: { bytes: 1720, lines: 4, droppedBytes: 0 },
      lastError: {
        operation: "start",
        message: "上次退出码 1 · 端口 7788 已被占用",
      },
    },
    runs: [
      {
        runId: "e3f0",
        sessionId: "test-api",
        startedAt: iso(5 * H),
        endedAt: iso(5 * H - 4000),
        exitCode: 1,
        pid: 20881,
        logMode: "on_error",
        logSource: "captured",
        logFile:
          "%LOCALAPPDATA%\\LocalConsoleHub\\logs\\test-api\\2026-09\\2026-09-26_23-21-00__run-e3f0.log",
      },
    ],
    lines: [
      { kind: "sys", text: "上次运行失败 · run e3f0" },
      {
        kind: "err",
        text: "ERROR:    [Errno 10048] error while attempting to bind on address ('127.0.0.1', 7788)",
      },
      { kind: "err", text: "OSError: 端口 7788 已被占用" },
      { kind: "sys", text: "会话已停止。启动前请确认端口空闲。" },
    ],
  },
  {
    group: "debug",
    config: {
      id: "pwsh",
      name: "PowerShell",
      sessionType: "terminal",
      cwd: "D:\\Work",
      shell: "pwsh",
      purpose: "日常交互终端，跑一次性命令与 REPL。",
      closeImpact: "仅结束本终端；不会停止其它受管服务。",
      logging: { mode: "off", source: "none" },
    },
    runtime: {
      sessionId: "pwsh",
      status: "running",
      pid: 22104,
      runId: "t7b1",
      startedAt: iso(12 * M),
      ptyAttached: true,
      logging: { mode: "off", source: "none", external_path: undefined },
      buffer: { bytes: 2048, lines: 9, droppedBytes: 0 },
    },
    runs: [
      {
        runId: "t7b1",
        sessionId: "pwsh",
        startedAt: iso(12 * M),
        pid: 22104,
        logMode: "off",
        logSource: "none",
      },
    ],
    ready: true,
    lines: [
      { kind: "sys", text: "Interactive terminal · logging off · stdin 不落盘" },
      { kind: "out", text: "PowerShell 7.4.6" },
      { kind: "in", text: "Get-ChildItem .\\projects -Name" },
      { kind: "out", text: "comfy-workflows" },
      { kind: "out", text: "st-characters" },
      { kind: "out", text: "local-api" },
      { kind: "in", text: 'curl.exe -s -o NUL -w "%{http_code}" http://127.0.0.1:8000' },
      { kind: "out", text: "200" },
      { kind: "in", text: "help" },
      { kind: "out", text: "Local Console Hub 终端。这是可交互输入，不是只读日志。" },
      { kind: "out", text: "常用： dir  cd  echo  curl  Get-Process  clear  nvidia-smi" },
      { kind: "out", text: "Ctrl+C 中断当前命令。普通交互终端默认不落盘。" },
    ],
  },
  {
    group: "temp",
    config: {
      id: "scratch",
      name: "Scratch",
      sessionType: "terminal",
      cwd: "D:\\Work",
      shell: "pwsh",
      purpose: "用完即弃的草稿终端。",
      closeImpact: "丢弃内存缓冲。默认不写日志文件。",
      logging: { mode: "off", source: "none" },
    },
    runtime: {
      sessionId: "scratch",
      status: "stopped",
      ptyAttached: false,
      logging: { mode: "off", source: "none", external_path: undefined },
      buffer: { bytes: 0, lines: 0, droppedBytes: 0 },
    },
    runs: [],
    lines: [{ kind: "sys", text: "会话未启动。启动后这是一个真正的可交互终端，不是只读日志。" }],
  },
];

/** The session the shell selects on load — the reference's running service. */
export const DEFAULT_SELECTED_SESSION_ID = "sillytavern";
