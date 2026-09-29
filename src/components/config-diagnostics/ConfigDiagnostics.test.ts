import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ConfigReportDto } from "../../types/config";
import ConfigDiagnostics from "./ConfigDiagnostics";

const mixedReport: ConfigReportDto = {
  fileStatus: "loaded",
  configPath: "D:/Users/example/AppData/Roaming/LocalConsoleHub/config.yaml",
  sessions: [],
  errors: [
    {
      index: 2,
      sessionId: "broken-api",
      field: "port",
      message: "port 0 is invalid; set a port from 1 to 65535",
    },
  ],
};

describe("ConfigDiagnostics", () => {
  it("identifies a bad entry while confirming other valid sessions remain usable", () => {
    const markup = renderToStaticMarkup(
      createElement(ConfigDiagnostics, { report: mixedReport, error: null, sessionCount: 2 }),
    );

    expect(markup).toContain('aria-label="配置诊断"');
    expect(markup).toContain("其余 2 个有效会话仍可使用");
    expect(markup).toContain("第 2 项 · broken-api · port");
    expect(markup).toContain("port 0 is invalid");
    expect(markup).toContain("config.yaml");
  });

  it("distinguishes a first run from a valid but empty config", () => {
    const missingMarkup = renderToStaticMarkup(
      createElement(ConfigDiagnostics, {
        report: { ...mixedReport, fileStatus: "missing", sessions: [], errors: [] },
        error: null,
        sessionCount: 0,
        empty: true,
      }),
    );
    const emptyMarkup = renderToStaticMarkup(
      createElement(ConfigDiagnostics, {
        report: { ...mixedReport, sessions: [], errors: [] },
        error: null,
        sessionCount: 0,
        empty: true,
      }),
    );

    expect(missingMarkup).toContain("首次运行");
    expect(emptyMarkup).toContain("配置文件中还没有定义会话");
    expect(emptyMarkup).not.toContain("首次运行");
  });

  it("shows report transport failures as errors", () => {
    const markup = renderToStaticMarkup(
      createElement(ConfigDiagnostics, {
        report: null,
        error: "get_config_report failed",
        sessionCount: 0,
        empty: true,
      }),
    );

    expect(markup).toContain("无法读取配置诊断");
    expect(markup).toContain("get_config_report failed");
  });
});
