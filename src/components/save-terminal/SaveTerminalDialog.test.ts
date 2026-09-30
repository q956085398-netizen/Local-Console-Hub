import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { FIXTURE_SESSIONS } from "../../state/fixtures";
import type { SessionConfigDto } from "../../types/config";
import SaveTerminalDialog, { errorField, readableShell } from "./SaveTerminalDialog";

/** The terminal being saved: the fixture's temporary row (#62). */
function terminal(overrides: Partial<SessionConfigDto> = {}): SessionConfigDto {
  return {
    ...FIXTURE_SESSIONS[5].config,
    id: "terminal-1a2b",
    name: "PowerShell 1",
    sessionType: "terminal",
    cwd: "C:\\Users\\example",
    shell: "pwsh",
    temporary: true,
    ...overrides,
  };
}

function render(config: SessionConfigDto = terminal()): string {
  return renderToStaticMarkup(
    createElement(SaveTerminalDialog, { config, onSubmit: vi.fn(), onClose: vi.fn() }),
  );
}

describe("SaveTerminalDialog", () => {
  it("states the launch method it is saving instead of asking for it", () => {
    const markup = render(terminal({ shell: "pwsh", cwd: "D:\\Work\\proj" }));

    expect(markup).toContain("工作目录");
    expect(markup).toContain("D:\\Work\\proj");
    expect(markup).toContain("pwsh");
    // The launch method is what the terminal already runs, so there is no box
    // to type it into — only the name and the two optional words have inputs.
    expect(markup.match(/dialog__input/g)).toHaveLength(3);
    expect(markup.match(/dialog__required/g)).toHaveLength(1);
  });

  it("prefills the row's current name, so saving is one gesture", () => {
    expect(render(terminal({ name: "PowerShell 3" }))).toContain('value="PowerShell 3"');
  });

  it("says what is not saved, in the same place the user decides to save", () => {
    const markup = render();

    // Story 25: a saved terminal must not look like it is recording the
    // session it was saved from.
    expect(markup).toContain("不保存、不重放你输入过的命令");
    expect(markup).toContain("终端不会重启，也不会被复制");
  });

  it("is a form with somewhere for a refusal to land", () => {
    const markup = render();

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain("保存配置");
    expect(markup).toContain("取消");
    expect(markup).toContain("关闭");
  });
});

describe("errorField", () => {
  it("puts a refusal on the input the config layer named", () => {
    for (const field of ["name", "purpose", "close_impact"]) {
      expect(errorField({ field, message: "no" })).toBe(field);
    }
  });

  it("falls back to the banner for a field this form does not render", () => {
    // `cwd` and `shell` are shown but not editable, so a refusal about one has
    // no box to sit beside — it must still be visible.
    expect(errorField({ field: "cwd", message: "no" })).toBeNull();
    expect(errorField({ field: "shell", message: "no" })).toBeNull();
    expect(errorField({ field: "initial_command", message: "no" })).toBeNull();
    expect(errorField({ message: "没有配置文件位置" })).toBeNull();
    expect(errorField(null)).toBeNull();
  });
});

describe("readableShell", () => {
  it("shows a quoted command line as the path it names", () => {
    // The config quotes a path with a space so it survives the split into
    // program and arguments; the quotes are not part of the program.
    expect(readableShell('"C:\\Program Files\\PowerShell\\7\\pwsh.exe"')).toBe(
      "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
    );
    expect(readableShell("powershell")).toBe("powershell");
  });

  it("says nothing rather than something wrong when there is no shell", () => {
    expect(readableShell(undefined)).toBe("—");
    expect(readableShell("  ")).toBe("—");
  });
});
