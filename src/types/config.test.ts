import { describe, expect, it } from "vitest";
import {
  isConfigReportDto,
  isEffectiveLoggingDto,
  isFormErrorDto,
  isSessionConfigDto,
  type ConfigReportDto,
} from "./config";

/**
 * Seam under test (pre-agreed): the typed frontend ↔ backend config DTO
 * boundary.
 *
 * The fixtures below mirror what the Rust config layer serializes
 * (src-tauri/src/config/dto.rs, camelCase). They are an independent source
 * of truth for this test — if the backend contract changes, these fixtures
 * must change with it, which is exactly what the guards are supposed to
 * catch.
 */
const backendReport: ConfigReportDto = {
  fileStatus: "loaded",
  configPath: "D:/Hub/config.yaml",
  sessions: [
    {
      id: "sillytavern",
      name: "SillyTavern",
      sessionType: "service",
      cwd: "D:/Tools/SillyTavern",
      command: "node server.js",
      url: "http://127.0.0.1:8000/",
      port: 8000,
      purpose: "聊天前端",
      closeImpact: "可停止；网页会失联",
      display: "internal",
      lifecycle: "managed",
      logging: { mode: "on_error", source: "captured" },
    },
    {
      id: "extsvc",
      name: "App with own log",
      sessionType: "service",
      command: "run",
      display: "internal",
      lifecycle: "managed",
      logging: { mode: "always", source: "external", externalPath: "D:/app/data/app.log" },
    },
    {
      id: "devshell",
      name: "PowerShell",
      sessionType: "terminal",
      shell: "powershell",
      initialCommand: "Get-ChildItem",
      display: "internal",
      lifecycle: "managed",
      logging: { mode: "off", source: "none" },
    },
  ],
  errors: [
    {
      index: 4,
      sessionId: "badport",
      field: "port",
      message: "port 0 is not a usable service port — set 1-65535 or remove `port`",
    },
  ],
};

describe("config DTO guards", () => {
  it("accepts the payload the Rust config layer documents", () => {
    expect(isConfigReportDto(backendReport)).toBe(true);
    for (const session of backendReport.sessions) {
      expect(isSessionConfigDto(session)).toBe(true);
    }
  });

  it("accepts every documented effective logging combination", () => {
    const valid: Array<[string, string]> = [
      ["off", "none"],
      ["always", "captured"],
      ["on_error", "captured"],
      ["manual", "captured"],
      ["always", "external"],
    ];
    for (const [mode, source] of valid) {
      expect(isEffectiveLoggingDto({ mode, source })).toBe(true);
    }
  });

  it("rejects unresolved auto and unknown logging vocabulary", () => {
    expect(isEffectiveLoggingDto({ mode: "auto", source: "captured" })).toBe(false);
    expect(isEffectiveLoggingDto({ mode: "always", source: "undefined-source" })).toBe(false);
    expect(isEffectiveLoggingDto(null)).toBe(false);
  });

  it("rejects sessions with wrong shapes", () => {
    expect(isSessionConfigDto(null)).toBe(false);
    expect(isSessionConfigDto({ id: "x", name: "X", sessionType: "daemon" })).toBe(false);
    expect(
      isSessionConfigDto({
        id: "x",
        name: "X",
        sessionType: "service",
        port: "8000",
        logging: { mode: "off", source: "none" },
      }),
    ).toBe(false);
    expect(
      isSessionConfigDto({
        id: "x",
        name: "X",
        sessionType: "terminal",
        display: "internal",
        lifecycle: "managed",
        logging: { mode: "always", source: "none" },
      }),
    ).toBe(true);
  });

  /// The two #66 dimensions are part of the contract, not optional extras: a
  /// payload without them would leave the UI guessing whether to draw a
  /// terminal — and guessing wrong is the "假内嵌" the mode exists to prevent.
  it("requires a display mode and a lifecycle owner on every session", () => {
    const complete = {
      id: "x",
      name: "X",
      sessionType: "service",
      display: "window",
      lifecycle: "independent",
      logging: { mode: "off", source: "none" },
    };

    expect(isSessionConfigDto(complete)).toBe(true);
    expect(isSessionConfigDto({ ...complete, display: undefined })).toBe(false);
    expect(isSessionConfigDto({ ...complete, lifecycle: undefined })).toBe(false);
    expect(isSessionConfigDto({ ...complete, display: "external" })).toBe(false);
    expect(isSessionConfigDto({ ...complete, lifecycle: "self" })).toBe(false);
  });

  it("reads the temporary flag as a boolean, and its absence as configured", () => {
    const terminal = {
      id: "terminal-1a2b",
      name: "PowerShell 1",
      sessionType: "terminal",
      display: "internal",
      lifecycle: "managed",
      logging: { mode: "off", source: "none" },
    };
    // Absent: a configured session, which is what every payload written before
    // #62 says.
    expect(isSessionConfigDto(terminal)).toBe(true);
    expect(isSessionConfigDto({ ...terminal, temporary: true })).toBe(true);
    expect(isSessionConfigDto({ ...terminal, temporary: false })).toBe(true);
    expect(isSessionConfigDto({ ...terminal, temporary: "yes" })).toBe(false);
  });

  it("rejects reports whose parts are not DTOs", () => {
    expect(isConfigReportDto({ sessions: [{}], errors: [] })).toBe(false);
    expect(isConfigReportDto({ sessions: [], errors: [{ message: 1 }] })).toBe(false);
    expect(isConfigReportDto({ sessions: [] })).toBe(false);
  });

  it("validates config-file status and optional path in startup reports", () => {
    expect(
      isConfigReportDto({
        ...backendReport,
        fileStatus: "missing",
        configPath: "D:/Hub/config.yaml",
      }),
    ).toBe(true);
    expect(isConfigReportDto({ ...backendReport, fileStatus: "corrupt" })).toBe(false);
    expect(isConfigReportDto({ ...backendReport, fileStatus: "loaded", configPath: 42 })).toBe(
      false,
    );
  });
});

describe("the save refusal a dialog places (#64, #65)", () => {
  it("accepts a sentence with or without a field to put it beside", () => {
    expect(isFormErrorDto({ message: "没有配置文件位置" })).toBe(true);
    expect(isFormErrorDto({ field: "name", message: "`name` is empty" })).toBe(true);
    expect(isFormErrorDto({ field: 7, message: "no" })).toBe(false);
    expect(isFormErrorDto({ field: "name" })).toBe(false);
    expect(isFormErrorDto("boom")).toBe(false);
    expect(isFormErrorDto(null)).toBe(false);
  });

  it("does not read a refused lifecycle command as a form refusal", () => {
    // A session error also carries a string message and no field, so without
    // this exclusion a failed command would be shown inside the dialog as if
    // the form had refused it — losing the operation the message names.
    expect(
      isFormErrorDto({
        kind: "unknown_session",
        sessionId: "terminal-1a2b",
        operation: "save_terminal_config",
        message: "no session is registered as `terminal-1a2b`",
      }),
    ).toBe(false);
  });
});
