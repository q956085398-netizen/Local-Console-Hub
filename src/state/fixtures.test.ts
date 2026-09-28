import { describe, expect, it } from "vitest";

import { isSessionConfigDto } from "../types/config";
import { isRunRecordDto, isSessionRuntimeDto } from "../types/runtime";
import { liveCounts, runOutcome, sidebarRowMeta } from "./derivations";
import { DEFAULT_SELECTED_SESSION_ID, FIXTURE_GROUPS, FIXTURE_SESSIONS } from "./fixtures";

describe("FIXTURE_SESSIONS", () => {
  it("consists entirely of values that pass the landed DTO guards", () => {
    for (const fixture of FIXTURE_SESSIONS) {
      expect(isSessionConfigDto(fixture.config)).toBe(true);
      expect(isSessionRuntimeDto(fixture.runtime)).toBe(true);
      for (const run of fixture.runs) {
        expect(isRunRecordDto(run)).toBe(true);
      }
    }
  });

  it("matches the V2 reference cast across the three workload groups", () => {
    expect(FIXTURE_SESSIONS.map((s) => [s.group, s.config.id])).toEqual([
      ["ai", "sillytavern"],
      ["ai", "comfyui"],
      ["ai", "koboldcpp"],
      ["debug", "test-api"],
      ["debug", "pwsh"],
      ["temp", "scratch"],
    ]);
    expect(liveCounts(FIXTURE_SESSIONS)).toEqual({ total: 6, running: 4, busy: 1 });
  });

  it("keeps every fixture runtime aligned with its config id and logging", () => {
    for (const fixture of FIXTURE_SESSIONS) {
      expect(fixture.runtime.sessionId).toBe(fixture.config.id);
      expect(fixture.runtime.logging.mode).toBe(fixture.config.logging.mode);
      expect(fixture.runtime.logging.source).toBe(fixture.config.logging.source);
    }
  });

  it("never attaches a persisted log file to an off/none session", () => {
    for (const fixture of FIXTURE_SESSIONS) {
      if (fixture.config.logging.mode === "off" && fixture.config.logging.source === "none") {
        for (const run of fixture.runs) {
          expect(run.logFile).toBeUndefined();
        }
      }
    }
  });

  it("gives every live run a run record, and stopped history exit codes", () => {
    const byId = new Map(FIXTURE_SESSIONS.map((s) => [s.config.id, s]));
    const running = byId.get("sillytavern");
    expect(running?.runs.at(-1)?.endedAt).toBeUndefined();
    expect(running?.runs.at(-1)?.logFile).toBeDefined();
    expect(runOutcome(byId.get("comfyui")!.runs[0])).toBe("ok");
    expect(runOutcome(byId.get("koboldcpp")!.runs[0])).toBe("error");
    expect(byId.get("scratch")?.runs).toEqual([]);
  });

  it("selects the running service from the reference by default", () => {
    expect(DEFAULT_SELECTED_SESSION_ID).toBe("sillytavern");
  });

  it("provides preview lines for every session and groups only known ids", () => {
    const groupIds = new Set(FIXTURE_GROUPS.map((g) => g.id));
    for (const fixture of FIXTURE_SESSIONS) {
      expect(fixture.lines?.length).toBeGreaterThan(0);
      expect(groupIds.has(fixture.group)).toBe(true);
      expect(sidebarRowMeta(fixture).length).toBeGreaterThan(0);
    }
  });
});
