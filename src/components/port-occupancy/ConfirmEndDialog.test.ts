import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import ConfirmEndDialog from "./ConfirmEndDialog";

function buttonText(markup: string): string[] {
  return [...markup.matchAll(/<button\b[^>]*>(.*?)<\/button>/g)].map((match) => match[1]);
}

describe("ConfirmEndDialog", () => {
  it("names the confirmed identity and does not end on cancel", () => {
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    const markup = renderToStaticMarkup(
      createElement(ConfirmEndDialog, {
        target: { pid: 42, createdAt: "132" },
        busy: false,
        message: null,
        onCancel,
        onConfirm,
      }),
    );

    expect(markup).toContain("PID 42");
    expect(markup).toContain("132");
    expect(markup).toContain("会话状态不变");
    expect(markup).toContain("继续运行");
    expect(markup).not.toContain("仍然启动");
    expect(buttonText(markup)).toEqual(["取消", "确认结束"]);
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("shows that nothing was ended", () => {
    const markup = renderToStaticMarkup(
      createElement(ConfirmEndDialog, {
        target: { pid: 42, createdAt: "132" },
        busy: false,
        message: "没有结束。进程已经退出，或这个 PID 已经是另一个进程。",
        onCancel: vi.fn(),
        onConfirm: vi.fn(),
      }),
    );

    expect(markup).toContain("没有结束");
  });
});
