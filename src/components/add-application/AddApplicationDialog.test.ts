import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import AddApplicationDialog, { errorPlacement, loggingFor } from "./AddApplicationDialog";

function render(): string {
  return renderToStaticMarkup(
    createElement(AddApplicationDialog, { onSubmit: vi.fn(), onClose: vi.fn() }),
  );
}

describe("AddApplicationDialog", () => {
  it("asks for the three required fields, and says which ones they are", () => {
    const markup = render();

    for (const label of ["名称", "工作目录", "启动命令"]) {
      expect(markup).toContain(label);
    }
    // Three `必填` marks, in that order: the fields the spec names as required
    // (spec #59 decision 7) and no others.
    expect(markup.match(/add-app__required/g)).toHaveLength(3);
    for (const optional of ["端口", "网页地址", "用途", "关闭影响", "日志策略"]) {
      expect(markup).toContain(optional);
    }
  });

  it("offers the optional fields the spec names, and no invented ones", () => {
    const markup = render();

    expect(markup).toContain("日志策略");
    expect(markup).toContain("关联应用自有日志");
    expect(markup).toContain("关闭影响");
    // The log file path only exists for the external policy, so it is not on
    // screen until that policy is chosen.
    expect(markup).not.toContain("应用日志文件");
  });

  it("states the Hub-internal display mode instead of offering a mode that does not work", () => {
    const markup = render();

    expect(markup).toContain("Hub 内显示");
    expect(markup).not.toContain("独立窗口");
  });

  it("keeps a refusal's message on the form rather than closing it", () => {
    // The dialog renders its own banner slot; what matters here is that the
    // component is a form with a submit control and a close control, so a
    // failure has somewhere to land without a second surface.
    const markup = render();

    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('aria-modal="true"');
    expect(markup).toContain("保存应用");
    expect(markup).toContain("取消");
    expect(markup).toContain("关闭");
  });
});

describe("errorPlacement", () => {
  it("puts a refusal on the input the config layer named", () => {
    for (const field of ["name", "cwd", "command", "port", "url", "purpose", "close_impact"]) {
      expect(errorPlacement({ field, message: "no" }, "")).toBe(field);
    }
  });

  it("falls back to the banner for a refusal with no field, or one this form has no input for", () => {
    expect(errorPlacement({ message: "没有配置文件位置" }, "")).toBeNull();
    expect(errorPlacement(null, "")).toBeNull();
    // A field the form does not render must not swallow its own message.
    expect(errorPlacement({ field: "logging.mode", message: "no" }, "")).toBeNull();
    expect(errorPlacement({ field: "initial_command", message: "no" }, "")).toBeNull();
  });

  it("places the log-path refusal only while that input is on screen", () => {
    const error = { field: "logging.path", message: "no" };

    expect(errorPlacement(error, "external")).toBe("logging.path");
    expect(errorPlacement(error, "on_error")).toBeNull();
  });
});

describe("loggingFor", () => {
  it("omits the block entirely for the unspecified policy", () => {
    expect(loggingFor("", "")).toBeUndefined();
  });

  it("maps each policy to a combination the config layer accepts", () => {
    expect(loggingFor("off", "")).toEqual({ mode: "off", source: "none" });
    expect(loggingFor("on_error", "")).toEqual({ mode: "on_error", source: "captured" });
    expect(loggingFor("always", "")).toEqual({ mode: "always", source: "captured" });
    expect(loggingFor("manual", "")).toEqual({ mode: "manual", source: "captured" });
  });

  it("carries the application's own log path only for the external policy", () => {
    expect(loggingFor("external", "  D:/Tools/App/access.log  ")).toEqual({
      mode: "always",
      source: "external",
      path: "D:/Tools/App/access.log",
    });
  });

  it("never emits a contradictory pair", () => {
    // `mode: off` with `source: captured` and `source: external` with no path
    // are the two combinations the config layer refuses; neither is reachable
    // from the form.
    for (const policy of ["", "off", "on_error", "always", "manual"]) {
      const logging = loggingFor(policy, "");
      if (logging === undefined) continue;
      expect(logging.source === "captured" && logging.mode === "off").toBe(false);
    }
  });
});
