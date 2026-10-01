// Real Tauri/WebView2 + real IPC. Run outside the Windows sandbox.
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, writeFile, mkdtemp, stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { createServer, createConnection } from "node:net";
import { release } from "node:os";
import { chromium } from "playwright-core";

assert.equal(process.platform, "win32", "Windows native acceptance requires Windows");
const repository = resolve(import.meta.dirname, "..");
const binary = resolve(
  process.argv[2] ?? join(repository, "src-tauri/target/issue-57/debug/local-console-hub.exe"),
);
const outputBase = join(repository, ".scratch", "native-acceptance");
await mkdir(outputBase, { recursive: true });
const output = await mkdtemp(join(outputBase, "run-"));
const report = {
  startedAt: new Date().toISOString(),
  sourceCommit: execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: repository,
    encoding: "utf8",
  }).trim(),
  sourceStatus: execFileSync("git", ["status", "--short"], {
    cwd: repository,
    encoding: "utf8",
  }).trim(),
  binary,
  binarySha256: await hashFile(binary),
  platform: process.platform,
  node: process.version,
  windowsRelease: release(),
  architecture: process.arch,
  scriptSha256: await hashFile(join(repository, "scripts", "native-acceptance.mjs")),
  backendPathsSha256: await hashFile(join(repository, "src-tauri/src/config/paths.rs")),
  cargoManifestSha256: await hashFile(join(repository, "src-tauri/Cargo.toml")),
  execution: "signed-in Windows user; serial; real Tauri WebView2 and IPC",
  results: [],
  exclusions: [
    "Native tray menu clicks",
    "Installation/MSI and installed-version acceptance",
    "Whole-window decorations and taskbar/icon appearance",
  ],
  cleanup: [],
};
const originalConfig = process.env.APPDATA
  ? join(process.env.APPDATA, "LocalConsoleHub", "config.yaml")
  : undefined;
const originalHash = originalConfig ? await optionalHash(originalConfig) : undefined;

async function hashFile(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}
async function optionalHash(path) {
  try {
    return await hashFile(path);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}
function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    throw error;
  }
}
async function unusedPort() {
  const server = createServer();
  await new Promise((done, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", done);
  });
  const port = server.address().port;
  await new Promise((done) => server.close(done));
  return port;
}
async function until(read, matches, message, timeout = 20_000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await read();
    if (matches(value)) return value;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error(message);
}
async function check(name, execute) {
  const started = Date.now();
  try {
    const evidence = await execute();
    report.results.push({ name, status: "passed", milliseconds: Date.now() - started, evidence });
    console.log(`PASS ${name}`);
  } catch (error) {
    report.results.push({
      name,
      status: "failed",
      milliseconds: Date.now() - started,
      error: String(error.stack ?? error),
    });
    console.log(`FAIL ${name}: ${error.message}`);
    throw error;
  } finally {
    await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2));
  }
}
const fixture = `sessions:
  - id: powershell
    name: PowerShell
    type: terminal
    shell: powershell
    cwd: '${repository.replaceAll("'", "''")}'
    purpose: 日常交互终端，跑一次性命令与 REPL。
    close_impact: 仅结束本终端；不会停止其它受管服务。
    logging: {mode: off, source: none}
  - id: manual
    name: Manual Terminal
    type: terminal
    shell: powershell
    cwd: .
    logging: {mode: manual, source: captured}
  - id: write-error
    name: Write Error Terminal
    type: terminal
    shell: powershell
    cwd: .
    logging: {mode: manual, source: captured}
  - id: service
    name: Save Service
    type: service
    command: cmd.exe /c ping -n 120 127.0.0.1
    cwd: .
    logging: {mode: on_error, source: captured}
`;

async function launch(name, config, applicationId) {
  const root = join(output, name);
  const configPath = join(root, "roaming", "LocalConsoleHub", "config.yaml");
  await mkdir(join(root, "roaming", "LocalConsoleHub"), { recursive: true });
  if (config?.directory === true) await mkdir(configPath);
  else if (config !== null) await writeFile(configPath, config, "utf8");
  const port = await unusedPort();
  const entry = applicationId ? join(root, "Application entry.lnk") : undefined;
  if (entry) {
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-File",
        join(repository, "scripts/create-application-shortcut.ps1"),
        "-Shortcut",
        entry,
        "-Hub",
        binary,
        "-ApplicationId",
        applicationId,
      ],
      { windowsHide: true },
    );
  }
  let spawnError;
  const child = spawn(
    entry ? "powershell.exe" : binary,
    entry
      ? [
          "-NoProfile",
          "-Command",
          `$app = Start-Process -FilePath '${entry.replaceAll("'", "''")}' -PassThru; $app.WaitForExit(); exit $app.ExitCode`,
        ]
      : [],
    {
      cwd: repository,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        LCH_ACCEPTANCE_ROOT: root,
        WEBVIEW2_USER_DATA_FOLDER: join(root, "webview2"),
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port}`,
      },
    },
  );
  child.on("error", (error) => {
    spawnError = error;
  });
  let logs = "";
  child.stdout.on("data", (data) => {
    logs += data;
  });
  child.stderr.on("data", (data) => {
    logs += data;
  });
  let browser;
  let page;
  const hubPids = [];
  const stop = async () => {
    let sessionPids = [];
    if (page && !page.isClosed()) {
      try {
        const sessions = await ipc(page, "list_sessions");
        sessionPids = sessions.map((session) => session.pid).filter((pid) => pid != null);
        for (const session of sessions) {
          if (session.status === "running")
            await ipc(page, "force_stop_session", { sessionId: session.sessionId });
        }
      } catch (error) {
        report.results.push({
          name: `${name}: session cleanup`,
          status: "failed",
          error: String(error),
        });
      }
    }
    if (browser) await browser.close().catch(() => {});
    if (child.pid && isAlive(child.pid)) {
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        windowsHide: true,
        stdio: "ignore",
      });
    }
    if (child.pid)
      await until(
        async () => isAlive(child.pid),
        (alive) => !alive,
        "Owned app survived cleanup",
      );
    for (const pid of sessionPids) {
      await until(
        async () => isAlive(pid),
        (alive) => !alive,
        `Owned session pid ${pid} survived cleanup`,
      );
    }
    for (const pid of hubPids) {
      await until(
        async () => isAlive(pid),
        (alive) => !alive,
        `Hub pid ${pid} survived cleanup`,
      );
    }
    report.cleanup.push({
      scenario: name,
      launcherPid: child.pid,
      hubPids,
      sessionPids,
      status: "passed",
    });
    await writeFile(join(root, "application.log"), logs);
  };
  try {
    await until(
      async () => {
        if (spawnError) throw spawnError;
        if (child.exitCode !== null) throw new Error(`App exited with ${child.exitCode}: ${logs}`);
        try {
          return (await fetch(`http://127.0.0.1:${port}/json/version`)).ok;
        } catch {
          return false;
        }
      },
      Boolean,
      "WebView2 CDP startup timed out",
      40_000,
    );
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    page = await until(
      async () => browser.contexts().flatMap((context) => context.pages())[0],
      Boolean,
      "No native WebView2 page",
    );
    await page.waitForFunction(() => Boolean(globalThis.__TAURI_INTERNALS__?.invoke));
    const configReport = await ipc(page, "get_config_report");
    assert.equal(
      configReport.configPath.toLowerCase(),
      configPath.toLowerCase(),
      "Refuse testing a binary that reads user configuration",
    );
    await until(
      () => page.locator('[aria-label="会话同步状态"]').count(),
      (count) => count === 0,
      "Session initialization did not complete",
    );
    return { page, root, configPath, configReport, stop, entry, hubPids };
  } catch (error) {
    await stop();
    throw error;
  }
}
async function ipc(page, command, args = {}) {
  return page.evaluate(
    ({ command, args }) => globalThis.__TAURI_INTERNALS__.invoke(command, args),
    { command, args },
  );
}
// The public launch-request transport, with the real Hub's answer. Unlike an
// invalid CLI launch this can assert the failure without leaving a native
// message box awaiting a user.
function applicationRequest(id) {
  return new Promise((done, reject) => {
    const pipe = createConnection(String.raw`\\.\pipe\LocalConsoleHub.Instance.v1`);
    let frame = "";
    pipe.setEncoding("utf8");
    pipe.setTimeout(20000, () => pipe.destroy(new Error("Launch request timed out")));
    pipe.on("error", reject);
    pipe.on("connect", () => pipe.write(JSON.stringify({ request: "openApplication", id }) + "\n"));
    pipe.on("data", (data) => {
      frame += data;
      if (frame.includes("\n")) {
        try {
          done(JSON.parse(frame.trim()));
        } catch (error) {
          reject(error);
        }
        pipe.destroy();
      }
    });
    pipe.on("end", () => {
      if (!frame.includes("\n")) reject(new Error("Incomplete launch response"));
    });
  });
}
async function select(page, name) {
  await page.locator(".session-row").filter({ hasText: name }).click();
  await until(
    () => page.locator("h1").innerText(),
    (text) => text === name,
    "Wrong selected session",
  );
}
async function start(page) {
  await page.locator(".session-header").getByRole("button", { name: "启动", exact: true }).click();
  const stop = page.locator(".session-header").getByRole("button", { name: "停止", exact: true });
  await stop.waitFor();
  await until(() => stop.isEnabled(), Boolean, "Session did not finish starting");
}
async function screenshot(page, name) {
  await page.screenshot({ path: join(output, `${name}.png`) });
}

try {
  await check("configured application shortcut: cold launch and running reuse", async () => {
    const app = await launch("application-entry", fixture, "service");
    try {
      const current = () => ipc(app.page, "get_session", { sessionId: "service" });
      const first = await until(
        current,
        (value) => value.status === "running",
        "Cold shortcut did not activate service",
      );
      assert.ok(first.pid);
      const before = await optionalHash(app.configPath);
      assert.equal(before, createHash("sha256").update(fixture).digest("hex"));
      await until(
        () => app.page.locator("h1").innerText(),
        (text) => text === "Save Service",
        "Cold shortcut did not select its application",
      );
      const metadata = execFileSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-Command",
          `$link = (New-Object -ComObject WScript.Shell).CreateShortcut('${app.entry.replaceAll("'", "''")}'); @($link.TargetPath,$link.Arguments,$link.IconLocation) | ConvertTo-Json -Compress`,
        ],
        { encoding: "utf8", windowsHide: true },
      );
      const [target, arguments_, icon] = JSON.parse(metadata);
      assert.equal(target.toLowerCase(), binary.toLowerCase());
      assert.equal(arguments_, "--open-app service");
      assert.equal(icon.toLowerCase(), `${binary},0`.toLowerCase());
      for (let index = 0; index < 2; index++) {
        const exit = execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-Command",
            `$env:LCH_ACCEPTANCE_ROOT='${app.root.replaceAll("'", "''")}'; $p=Start-Process -FilePath '${app.entry.replaceAll("'", "''")}' -PassThru; if (-not $p.WaitForExit(20000)) { throw 'Shortcut handoff timed out' }; $p.ExitCode`,
          ],
          { encoding: "utf8", windowsHide: true, timeout: 25000 },
        );
        assert.equal(exit.trim(), "0");
        assert.equal((await current()).pid, first.pid);
      }
      const hubs = JSON.parse(
        execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-Command",
            `ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq '${binary.replaceAll("'", "''")}' } | Select-Object ProcessId,ExecutablePath)`,
          ],
          { encoding: "utf8", windowsHide: true },
        ),
      );
      assert.equal(hubs.length, 1, "The actual binary must have exactly one Hub process");
      app.hubPids.push(hubs[0].ProcessId);
      const invalid = await applicationRequest("missing-saved-application");
      assert.equal(invalid.delivered, false);
      assert.match(invalid.message, /missing-saved-application/);
      assert.equal((await current()).pid, first.pid);
      assert.equal(await optionalHash(app.configPath), before);
      await screenshot(app.page, "application-shortcut");
      return {
        entry: app.entry,
        target,
        arguments: arguments_,
        icon,
        hubs,
        servicePid: first.pid,
        configurationUnchanged: true,
        unknownIdResponse: invalid,
      };
    } finally {
      await app.stop();
    }
  });
  let app;
  try {
    await check("isolated real backend connection", async () => {
      app = await launch("valid", fixture);
      assert.equal(app.configReport.errors.length, 0);
      assert.equal((await ipc(app.page, "list_sessions")).length, 4);
      assert.equal(await app.page.getByText("UI 预览", { exact: true }).count(), 0);
      return { configPath: app.configPath, binarySha256: report.binarySha256 };
    });
    const { page, root } = app;
    await check("T-11 terminal purpose and close impact", async () => {
      await select(page, "PowerShell");
      await start(page);
      assert.equal(
        await page.locator(".session-header__purpose").innerText(),
        "日常交互终端，跑一次性命令与 REPL。",
      );
      assert.equal(
        await page.locator(".callout__text").innerText(),
        "仅结束本终端；不会停止其它受管服务。",
      );
      await screenshot(page, "terminal-fields");
      await select(page, "Manual Terminal");
      await start(page);
      assert.equal(
        await page.locator(".callout__text").innerText(),
        "停止该终端会同时结束它启动的子进程。",
      );
    });
    await check("manual recording starts and stops in the real window", async () => {
      await page.getByRole("tab", { name: "日志", exact: true }).click();
      await page.getByRole("button", { name: "开始记录", exact: true }).click();
      await page.getByRole("button", { name: "停止记录", exact: true }).waitFor();
      assert.match(await page.locator(".logs-panel__badges").innerText(), /Capturing/);
      const info = await ipc(page, "get_log_info", { sessionId: "manual" });
      await page.getByRole("button", { name: "停止记录", exact: true }).click();
      await page.getByRole("button", { name: "开始记录", exact: true }).waitFor();
      assert.match(await page.locator(".logs-panel__badges").innerText(), /Off/);
      await screenshot(page, "recording-stopped");
      return info;
    });
    await check("real terminal keyboard input reaches ConPTY", async () => {
      await page.getByRole("tab", { name: "终端", exact: true }).click();
      await page.locator(".xterm-helper-textarea").focus();
      // Assemble the output marker so its appearance cannot be just input echo.
      await page.keyboard.type("Write-Output ('LCH_NATIVE_' + 'ROUNDTRIP_57')");
      await page.keyboard.press("Enter");
      const marker = "LCH_NATIVE_ROUNDTRIP_57";
      const attachment = await until(
        () => ipc(page, "attach_terminal", { sessionId: "manual" }),
        (value) =>
          Buffer.concat(value.chunks.map((chunk) => Buffer.from(chunk.data, "base64")))
            .toString("utf8")
            .includes(marker),
        "The real shell did not execute keyboard input",
      );
      await screenshot(page, "terminal-roundtrip");
      return { sessionId: "manual", marker, generation: attachment.generation };
    });
    await check("save reports an existing log and enables its file action", async () => {
      await select(page, "Save Service");
      await start(page);
      await page.getByRole("tab", { name: "日志", exact: true }).click();
      await until(
        () => ipc(page, "get_log_info", { sessionId: "service" }),
        (info) => info.buffer.bytes > 0,
        "Service produced no buffered output",
      );
      await page.getByRole("button", { name: "保存本次日志", exact: true }).click();
      await until(
        () => page.locator(".status-bar__right").innerText(),
        (text) => text.includes("本次运行的缓冲已写入日志文件"),
        "No confirmed save notice",
      );
      const info = await ipc(page, "get_log_info", { sessionId: "service" });
      const paths = await page.locator(".logs-panel__paths").innerText();
      const file = paths.match(/[A-Za-z]:\\[^\r\n]*\.log/)?.[0];
      assert.ok(file && file.startsWith(root), "Saved log must belong to this isolated run");
      assert.ok((await stat(file)).isFile());
      assert.equal(
        await page.getByRole("button", { name: "打开日志", exact: true }).first().isEnabled(),
        true,
      );
      await screenshot(page, "saved-log");
      return info;
    });
    await check("real log write failure remains visible and never says success", async () => {
      const logs = join(root, "local", "LocalConsoleHub", "logs");
      await mkdir(logs, { recursive: true });
      await writeFile(join(logs, "write-error"), "blocks the directory");
      await select(page, "Write Error Terminal");
      await start(page);
      await page.getByRole("tab", { name: "日志", exact: true }).click();
      await page.getByRole("button", { name: "开始记录", exact: true }).click();
      await page.locator(".logs-panel__warning").waitFor();
      assert.doesNotMatch(await page.locator(".status-bar__right").innerText(), /已开始记录/);
      const runtime = await ipc(page, "get_session", { sessionId: "write-error" });
      assert.equal(runtime.status, "running");
      await screenshot(page, "write-error");
      const info = await ipc(page, "get_log_info", { sessionId: "write-error" });
      assert.ok(info.lastError?.message);
      assert.equal(info.state, "off");
      return info;
    });
    await check("frontend reload preserves latest backend running sessions", async () => {
      const before = await ipc(page, "list_sessions");
      await page.reload();
      await page.locator(".session-row").first().waitFor();
      const after = await ipc(page, "list_sessions");
      assert.deepEqual(
        after.map((row) => [row.sessionId, row.status, row.pid]),
        before.map((row) => [row.sessionId, row.status, row.pid]),
      );
      assert.match(await page.locator(".status-bar__left").innerText(), /4\/4 运行/);
    });
  } finally {
    if (app) await app.stop();
  }

  const scenarios = [
    [
      "mixed-config",
      `${fixture}\n  - id: invalid\n    name: Invalid\n    type: nonsense\n`,
      /invalid.*type/s,
      4,
    ],
    ["bad-yaml", "sessions: [\n", /YAML|yaml/, 0],
    ["missing-config", null, /首次运行/, 0],
    ["empty-config", "", /配置文件中还没有定义会话/, 0],
    ["unreadable-config", { directory: true }, /无法读取配置文件/, 0],
  ];
  for (const [name, config, expected, sessionCount] of scenarios) {
    await check(`native configuration diagnostics: ${name}`, async () => {
      const app = await launch(name, config);
      try {
        assert.match(await app.page.locator('[aria-label="配置诊断"]').innerText(), expected);
        assert.equal((await ipc(app.page, "list_sessions")).length, sessionCount);
        await screenshot(app.page, name);
        return app.configReport;
      } finally {
        await app.stop();
      }
    });
  }
} catch {
  process.exitCode = 1;
} finally {
  if (originalConfig) {
    const unchanged = originalHash === (await optionalHash(originalConfig));
    report.userConfigurationUnchanged = unchanged;
    if (!unchanged) process.exitCode = 1;
  }
  report.finishedAt = new Date().toISOString();
  await writeFile(join(output, "report.json"), JSON.stringify(report, null, 2));
  if (report.results.some((result) => result.status === "failed")) process.exitCode = 1;
  console.log(`Report: ${join(output, "report.json")}`);
}
