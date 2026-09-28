import { describe, expect, it } from "vitest";

import type { EffectiveLoggingDto, SessionConfigDto } from "../types/config";
import type { RunRecordDto, RuntimeEffectiveLoggingDto, SessionRuntimeDto } from "../types/runtime";
import {
  availableActions,
  bufferDiscardNotice,
  dependenciesOf,
  filterSessions,
  formatDuration,
  groupSessions,
  headerCallout,
  initialSelectedSessionId,
  isLive,
  liveCounts,
  logModeLabel,
  logPolicyBadge,
  logSourceLabel,
  loggingHeadline,
  metadataPairs,
  ptyChromeLabel,
  runOutcome,
  runOutcomeBadge,
  sidebarRowMeta,
  sidebarSummaryText,
  statusLabel,
  statusTone,
  titlebarSummaryText,
  typeLabel,
} from "./derivations";
import { FIXTURE_GROUPS, type FixtureSession } from "./fixtures";

function config(overrides: Partial<SessionConfigDto> = {}): SessionConfigDto {
  return {
    id: "sillytavern",
    name: "SillyTavern",
    sessionType: "service",
    cwd: "D:/Tools/SillyTavern",
    command: "node server.js",
    url: "http://127.0.0.1:8000",
    port: 8000,
    purpose: "聊天前端",
    closeImpact: "可停止；打开的聊天页会失联。",
    logging: { mode: "always", source: "captured" },
    ...overrides,
  };
}

function runtime(overrides: Partial<SessionRuntimeDto> = {}): SessionRuntimeDto {
  return {
    sessionId: "sillytavern",
    status: "running",
    pid: 18420,
    runId: "a91c",
    startedAt: "2026-09-28T05:00:00Z",
    ptyAttached: true,
    logging: { mode: "always", source: "captured", external_path: undefined },
    buffer: { bytes: 4096, lines: 8, droppedBytes: 0 },
    ...overrides,
  };
}

/** Config-side logging block in the runtime's wire casing (see runtime.ts). */
function toRuntimeLogging(logging: EffectiveLoggingDto): RuntimeEffectiveLoggingDto {
  return { mode: logging.mode, source: logging.source, external_path: logging.externalPath };
}

/**
 * A fixture whose snapshot logging is seeded from its config — mirroring the
 * backend, where `SessionRuntime::stopped` takes the validated effective
 * logging. Tests that need the two to diverge (a live run overriding the
 * config) pass an explicit `runtime` override.
 */
function fixture(overrides: Partial<FixtureSession> = {}): FixtureSession {
  const resolved = overrides.config ?? config();
  return {
    config: resolved,
    runtime: runtime({
      logging: toRuntimeLogging(resolved.logging),
      ...overrides.runtime,
    }),
    runs: overrides.runs ?? [],
    group: overrides.group ?? "ai",
    busy: overrides.busy,
    ready: overrides.ready,
    dependsOn: overrides.dependsOn,
    lines: overrides.lines,
  };
}

const NOW = new Date("2026-09-28T08:14:30Z");

describe("statusTone / statusLabel / typeLabel / isLive", () => {
  it("maps lifecycle states to the reference's tones and labels", () => {
    expect(statusTone("running")).toBe("run");
    expect(statusTone("running", true)).toBe("busy");
    expect(statusTone("starting")).toBe("warn");
    expect(statusTone("stopping")).toBe("warn");
    expect(statusTone("error")).toBe("err");
    expect(statusTone("stopped")).toBe("idle");
    expect(statusTone("exited")).toBe("idle");
    expect(statusLabel("running")).toBe("Running");
    expect(statusLabel("running", true)).toBe("Busy");
    expect(statusLabel("running", false, true)).toBe("Ready");
    expect(statusLabel("stopped")).toBe("Stopped");
    expect(statusLabel("starting")).toBe("Starting");
    expect(typeLabel("service")).toBe("Service");
    expect(typeLabel("terminal")).toBe("Terminal");
    expect(isLive("stopping")).toBe(true);
    expect(isLive("exited")).toBe(false);
  });
});

describe("formatDuration", () => {
  it("formats like the reference: hours, unpadded minutes/seconds", () => {
    expect(formatDuration("2026-09-28T08:13:45Z", NOW)).toBe("45s");
    expect(formatDuration("2026-09-28T08:02:25Z", NOW)).toBe("12m 5s");
    expect(formatDuration("2026-09-28T05:00:00Z", NOW)).toBe("3h 14m");
  });
});

describe("liveCounts / summaries", () => {
  const sessions = [
    fixture(),
    fixture({
      config: config({ id: "comfyui" }),
      runtime: runtime({ sessionId: "comfyui" }),
      busy: true,
    }),
    fixture({
      config: config({ id: "pwsh", sessionType: "terminal" }),
      runtime: runtime({ sessionId: "pwsh", status: "stopped" }),
    }),
  ];

  it("counts running and busy sessions separately", () => {
    expect(liveCounts(sessions)).toEqual({ total: 3, running: 2, busy: 1 });
  });

  it("words the sidebar and title-bar summaries", () => {
    const counts = liveCounts(sessions);
    expect(sidebarSummaryText(counts)).toBe("2 运行 · 1 忙碌 · 3 会话");
    expect(titlebarSummaryText(counts)).toBe("2 运行 · 1 busy");
    const calm = liveCounts([fixture()]);
    expect(sidebarSummaryText(calm)).toBe("1 运行 · 1 会话");
    expect(titlebarSummaryText(calm)).toBe("1 运行");
  });
});

describe("availableActions", () => {
  it("offers Start only when idle, Stop only when live", () => {
    const running = availableActions(config(), runtime());
    expect(running.start).toBe(false);
    expect(running.stop).toBe(true);
    const stopped = availableActions(config(), runtime({ status: "stopped" }));
    expect(stopped.start).toBe(true);
    expect(stopped.stop).toBe(false);
    expect(availableActions(config(), runtime({ status: "stopping" })).start).toBe(false);
  });

  it("keeps Stop visible but inert while already stopping", () => {
    const stopping = availableActions(config(), runtime({ status: "stopping" }));
    expect(stopping.stop).toBe(true);
    expect(stopping.stopDisabled).toBe(true);
    expect(availableActions(config(), runtime()).stopDisabled).toBe(false);
  });

  it("withholds Restart until the previous run is settled (spec §5 rule 2)", () => {
    expect(availableActions(config(), runtime()).restart).toBe(true);
    expect(availableActions(config(), runtime({ status: "stopped" })).restart).toBe(true);
    expect(availableActions(config(), runtime({ status: "starting" })).restart).toBe(false);
    expect(availableActions(config(), runtime({ status: "stopping" })).restart).toBe(false);
  });

  it("keeps directory and the service URL available", () => {
    const actions = availableActions(config(), runtime());
    expect(actions.directory).toBe(true);
    expect(actions.openUrl).toBe("http://127.0.0.1:8000");
    expect(availableActions(config({ url: undefined }), runtime()).openUrl).toBeUndefined();
    const terminal = config({
      sessionType: "terminal",
      url: undefined,
      port: undefined,
      shell: "pwsh",
    });
    expect(availableActions(terminal, runtime()).openUrl).toBeUndefined();
  });

  it("scopes force stop to the live window, including Stopping", () => {
    expect(availableActions(config(), runtime()).forceStop).toBe(true);
    expect(availableActions(config(), runtime({ status: "stopping" })).forceStop).toBe(true);
    expect(availableActions(config(), runtime({ status: "stopped" })).forceStop).toBe(false);
  });
});

describe("ptyChromeLabel", () => {
  it("reports the real attachment state, not the session type", () => {
    expect(
      ptyChromeLabel(config({ sessionType: "terminal" }), runtime({ ptyAttached: true })),
    ).toBe("ConPTY · interactive");
    expect(
      ptyChromeLabel(config({ sessionType: "terminal" }), runtime({ ptyAttached: false })),
    ).toBe("ConPTY · 未连接");
    expect(ptyChromeLabel(config(), runtime({ ptyAttached: true }))).toBe(
      "PTY attached · stdin 可用",
    );
    expect(ptyChromeLabel(config(), runtime({ ptyAttached: false }))).toBe("PTY 未连接 · 只读缓冲");
  });
});

describe("bufferDiscardNotice", () => {
  it("stays silent until output is actually lost, then counts the loss", () => {
    expect(bufferDiscardNotice(runtime())).toBeNull();
    expect(
      bufferDiscardNotice(runtime({ buffer: { bytes: 1, lines: 1, droppedBytes: 2048 } })),
    ).toBe("更早的输出已被丢弃（2048 B）");
  });
});

describe("runOutcomeBadge / logPolicyBadge", () => {
  const live: RunRecordDto = {
    runId: "c8aa",
    sessionId: "comfyui",
    startedAt: "2026-09-28T05:00:00Z",
    logMode: "always",
    logSource: "captured",
  };

  /// The wire spells "this run is still going" as `endedAt: null` as often as
  /// as a missing key, and reading only the missing key files every live run
  /// under "error" (see the note in `types/runtime.ts`).
  it("reads both spellings of an unfinished run as running", () => {
    expect(runOutcome({ ...live, endedAt: null, exitCode: null })).toBe("running");
    expect(runOutcomeBadge({ ...live, endedAt: null, exitCode: null })).toEqual({
      label: "running",
      tone: "run",
    });
  });

  it("labels a run outcome with its tone", () => {
    expect(runOutcomeBadge(live)).toEqual({ label: "running", tone: "run" });
    expect(runOutcomeBadge({ ...live, endedAt: "2026-09-28T06:00:00Z", exitCode: 0 })).toEqual({
      label: "ok",
      tone: "idle",
    });
    expect(runOutcomeBadge({ ...live, endedAt: "2026-09-28T06:00:00Z", exitCode: 1 })).toEqual({
      label: "error",
      tone: "err",
    });
  });

  it("says Capturing only while a captured run is actually writing", () => {
    expect(logPolicyBadge({ mode: "always", source: "captured" }, "running")).toEqual({
      label: "Capturing",
      tone: "run",
    });
    expect(logPolicyBadge({ mode: "always", source: "captured" }, "stopped")).toEqual({
      label: "Always",
      tone: "warn",
    });
    expect(logPolicyBadge({ mode: "off", source: "none" }, "running")).toEqual({
      label: "Off",
      tone: "idle",
    });
  });
});

describe("headerCallout", () => {
  it("shows the configured close impact verbatim while live", () => {
    const callout = headerCallout(config(), runtime());
    expect(callout).toEqual({
      kind: "impact",
      title: "关闭影响",
      text: "可停止；打开的聊天页会失联。",
    });
  });

  it("shows the last error instead once stopped", () => {
    const callout = headerCallout(
      config(),
      runtime({
        status: "stopped",
        lastError: { operation: "start", message: "端口 7788 已被占用" },
      }),
    );
    expect(callout).toEqual({ kind: "error", title: "上次错误", text: "端口 7788 已被占用" });
  });

  it("shows nothing for a clean stopped session", () => {
    expect(headerCallout(config(), runtime({ status: "stopped" }))).toBeNull();
  });
});

describe("logging labels", () => {
  it("labels modes and sources", () => {
    expect(logModeLabel("on_error")).toBe("On error");
    expect(logSourceLabel("captured")).toBe("Hub captured");
  });

  it("answers 'is this being logged?' in one line", () => {
    expect(loggingHeadline({ mode: "off", source: "none" })).toBe("仅内存缓冲，不写磁盘");
    expect(loggingHeadline({ mode: "always", source: "captured" })).toBe(
      "每次运行都保存 stdout/stderr",
    );
    expect(loggingHeadline({ mode: "on_error", source: "captured" })).toBe("异常退出时才落盘");
    expect(loggingHeadline({ mode: "manual", source: "captured" })).toBe("需手动开始记录");
    expect(loggingHeadline({ mode: "off", source: "external", external_path: "D:/a.log" })).toBe(
      "关联应用日志，不重复捕获",
    );
  });
});

describe("metadataPairs", () => {
  it("renders pid, port, uptime, cwd and log value for a running service", () => {
    expect(metadataPairs(config(), runtime(), NOW)).toEqual([
      { label: "PID", value: "18420" },
      { label: "port", value: ":8000" },
      { label: "up", value: "3h 14m" },
      { label: "cwd", value: "D:/Tools/SillyTavern" },
      { label: "log", value: "Always" },
    ]);
  });

  it("says buffer only for a terminal without persistence", () => {
    const terminal = config({
      sessionType: "terminal",
      url: undefined,
      port: undefined,
      shell: "pwsh",
    });
    const pairs = metadataPairs(
      terminal,
      runtime({
        sessionId: "pwsh",
        startedAt: "2026-09-28T08:02:25Z",
        logging: { mode: "off", source: "none", external_path: undefined },
      }),
      NOW,
    );
    expect(pairs).toEqual([
      { label: "PID", value: "18420" },
      { label: "up", value: "12m 5s" },
      { label: "cwd", value: "D:/Tools/SillyTavern" },
      { label: "log", value: "buffer only" },
    ]);
  });
});

describe("sidebarRowMeta", () => {
  it("describes services as port · svc, terminals as interactive · tty", () => {
    expect(sidebarRowMeta(fixture())).toEqual([
      { text: ":8000" },
      { text: "svc" },
      { text: "Always" },
    ]);
    expect(
      sidebarRowMeta(
        fixture({
          config: config({
            sessionType: "terminal",
            port: undefined,
            url: undefined,
            shell: "pwsh",
            logging: { mode: "off", source: "none" },
          }),
        }),
      ),
    ).toEqual([{ text: "interactive" }, { text: "tty" }]);
  });

  it("lets the live run override the config once one is running (LOGGING.md §1.4)", () => {
    expect(
      sidebarRowMeta(
        fixture({
          config: config({ logging: { mode: "off", source: "none" } }),
          runtime: runtime({
            logging: { mode: "manual", source: "captured", external_path: undefined },
          }),
        }),
      ),
    ).toContainEqual({ text: "Manual" });
  });

  it("reports an application-owned log as External when no mode is in effect", () => {
    expect(
      sidebarRowMeta(
        fixture({
          config: config({
            logging: { mode: "off", source: "external", externalPath: "D:/a.log" },
          }),
        }),
      ),
    ).toEqual([{ text: ":8000" }, { text: "svc" }, { text: "External" }]);
  });

  it("flags a busy running session and a failed one with a tone", () => {
    expect(sidebarRowMeta(fixture({ busy: true }))).toEqual([
      { text: ":8000" },
      { text: "svc" },
      { text: "busy", tone: "busy" },
      { text: "Always" },
    ]);
    expect(sidebarRowMeta(fixture({ runtime: runtime({ status: "error" }) }))).toContainEqual({
      text: "error",
      tone: "err",
    });
  });
});

describe("dependenciesOf", () => {
  it("resolves declared dependencies and ignores unknown ids", () => {
    const dep = fixture({ config: config({ id: "koboldcpp" }) });
    const subject = fixture({ dependsOn: ["koboldcpp", "missing"] });
    expect(dependenciesOf(subject, [subject, dep]).map((s) => s.config.id)).toEqual(["koboldcpp"]);
    expect(dependenciesOf(fixture(), [])).toEqual([]);
  });
});

describe("runOutcome", () => {
  it("derives the outcome from end and exit code", () => {
    const live: RunRecordDto = {
      runId: "c8aa",
      sessionId: "comfyui",
      startedAt: "2026-09-28T05:00:00Z",
      logMode: "always",
      logSource: "captured",
    };
    expect(runOutcome(live)).toBe("running");
    expect(runOutcome({ ...live, endedAt: "2026-09-28T06:00:00Z", exitCode: 0 })).toBe("ok");
    expect(runOutcome({ ...live, endedAt: "2026-09-28T06:00:00Z", exitCode: 1 })).toBe("error");
  });
});

describe("filterSessions / groupSessions", () => {
  const sessions = [
    fixture(),
    fixture({ config: config({ id: "comfyui", name: "ComfyUI", port: 8188 }), group: "ai" }),
    fixture({
      config: config({
        id: "test-api",
        name: "Test API",
        port: 7788,
        purpose: "本地调试用的临时 HTTP 接口。",
      }),
      group: "debug",
    }),
    fixture({
      config: config({
        id: "pwsh",
        name: "PowerShell",
        sessionType: "terminal",
        url: undefined,
        port: undefined,
        shell: "pwsh",
        logging: { mode: "off", source: "none" },
      }),
      group: "debug",
    }),
  ];

  it("filters by name, port, purpose, id and type", () => {
    expect(filterSessions(sessions, "comfy").map((s) => s.config.id)).toEqual(["comfyui"]);
    expect(filterSessions(sessions, "8188").map((s) => s.config.id)).toEqual(["comfyui"]);
    expect(filterSessions(sessions, "调试").map((s) => s.config.id)).toEqual(["test-api"]);
    expect(filterSessions(sessions, "  SillyTavern ").map((s) => s.config.id)).toEqual([
      "sillytavern",
    ]);
    expect(filterSessions(sessions, "terminal").map((s) => s.config.id)).toEqual(["pwsh"]);
    expect(filterSessions(sessions, "").length).toBe(4);
  });

  it("groups by workload in declared order and drops empty groups", () => {
    const groups = groupSessions(sessions, FIXTURE_GROUPS);
    expect(groups.map((g) => g.group.label)).toEqual(["AI Apps", "Debug / Test"]);
    expect(groups[0].items.map((s) => s.config.id)).toEqual(["sillytavern", "comfyui"]);
    expect(groups[1].items.map((s) => s.config.id)).toEqual(["test-api", "pwsh"]);
  });
});

describe("initialSelectedSessionId", () => {
  const ids = ["sillytavern", "pwsh"];

  it("honours a matching #session deep link", () => {
    expect(initialSelectedSessionId("#session=pwsh", ids)).toBe("pwsh");
  });

  it("falls back to the first session for unknown, absent or malformed hashes", () => {
    expect(initialSelectedSessionId("#session=nope", ids)).toBe("sillytavern");
    expect(initialSelectedSessionId("", ids)).toBe("sillytavern");
    expect(initialSelectedSessionId(null, ids)).toBe("sillytavern");
    expect(initialSelectedSessionId("#session=bad id", ids)).toBe("sillytavern");
  });
});
