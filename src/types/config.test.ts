import { describe, expect, it } from "vitest";
import {
  isConfigReportDto,
  isEffectiveLoggingDto,
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
      logging: { mode: "on_error", source: "captured" },
    },
    {
      id: "extsvc",
      name: "App with own log",
      sessionType: "service",
      command: "run",
      logging: { mode: "always", source: "external", externalPath: "D:/app/data/app.log" },
    },
    {
      id: "devshell",
      name: "PowerShell",
      sessionType: "terminal",
      shell: "powershell",
      initialCommand: "Get-ChildItem",
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
        logging: { mode: "always", source: "none" },
      }),
    ).toBe(true);
  });

  it("reads the temporary flag as a boolean, and its absence as configured", () => {
    const terminal = {
      id: "terminal-1a2b",
      name: "PowerShell 1",
      sessionType: "terminal",
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
