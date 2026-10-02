import { describe, expect, it } from "vitest";
import { applicationUrl, selectedProgram, isDirectoryScan } from "./discovery";

describe("application discovery input", () => {
  it("keeps optional URLs absent and preserves pasted complete addresses", () => {
    expect(applicationUrl("http://", " ")).toBeUndefined();
    expect(applicationUrl("http://", "127.0.0.1:8189")).toBe("http://127.0.0.1:8189");
    expect(applicationUrl("http://", " https://example.com/path ")).toBe(
      "https://example.com/path",
    );
  });

  it("quotes selected programs and keeps drive-root working directories absolute", () => {
    expect(selectedProgram("C:\\Start Here.cmd")).toEqual({
      path: "C:\\Start Here.cmd",
      command: '"C:\\Start Here.cmd"',
      cwd: "C:\\",
      port: null,
    });
    expect(selectedProgram("D:\\My App\\start.ps1").command).toBe(
      'powershell.exe -NoProfile -File "D:\\My App\\start.ps1"',
    );
  });

  it("rejects malformed scan data before it can prefill the form", () => {
    const scan = {
      name: "app",
      cwd: "C:\\app",
      logs: [],
      warnings: [],
      candidates: [selectedProgram("C:\\app\\start.cmd")],
    };
    expect(isDirectoryScan(scan)).toBe(true);
    expect(isDirectoryScan({ ...scan, candidates: [{ ...scan.candidates[0], port: 65536 }] })).toBe(
      false,
    );
    expect(isDirectoryScan({ ...scan, logs: [null] })).toBe(false);
  });
});
