// Real Tauri/WebView2 + real IPC. Run outside the Windows sandbox.
import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, writeFile, readFile, copyFile, mkdtemp, stat } from "node:fs/promises";
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
  else if (config !== null && config !== undefined) await writeFile(configPath, config, "utf8");
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
      windowsHide: Boolean(entry),
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
        sessionPids = sessions
          .filter((session) => !session.external)
          .map((session) => session.pid)
          .filter((pid) => pid != null);
        for (const session of sessions) {
          if (session.status === "running" && !session.external)
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
    return { page, root, configPath, configReport, stop, entry, hubPids, launcherPid: child.pid };
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

async function terminalText(page, sessionId) {
  const value = await ipc(page, "attach_terminal", { sessionId });
  return Buffer.concat(value.chunks.map((chunk) => Buffer.from(chunk.data, "base64"))).toString(
    "utf8",
  );
}
async function typeCommand(page, command) {
  // The blinking caret is the view's observable ready-for-input signal.
  // Selecting a row can precede stream attachment; wait before typing.
  await page.getByRole("tab", { name: "终端", exact: true }).click();
  await page.locator(".xterm-helper-textarea").focus();
  await until(
    () => page.locator(".xterm-cursor-blink").count(),
    (count) => count > 0,
    "Terminal did not show its ready-for-input caret",
  );
  await page.keyboard.type(command);
  await page.keyboard.press("Enter");
}

async function verifyApplicationRemoval() {
  let app;
  try {
    app = await launch(
      "removal",
      `# preserve header
sessions:
  - id: remove-me
    name: Remove Me
    type: service
    cwd: .
    command: cmd.exe /c ping -n 120 127.0.0.1
  # preserve second
  - id: keep-me
    name: Keep Me
    type: service
    cwd: .
    command: cmd.exe /c ping -n 120 127.0.0.1
`,
    );
    const { page } = app;
    await page.locator(".session-row").filter({ hasText: "Remove Me" }).click();
    await page.getByRole("button", { name: "更多操作" }).click();
    await page.getByRole("menuitem", { name: "从受管名单移除", exact: true }).click();
    await page.getByRole("dialog").waitFor();
    await page.screenshot({ path: join(output, "confirmation.png") });
    await page.getByRole("button", { name: "取消", exact: true }).click();
    assert.equal((await ipc(page, "list_session_configs")).length, 2);
    console.log("PASS cancel preserves configuration");
    await ipc(page, "start_session", { sessionId: "remove-me" });
    const before = await readFile(app.configPath, "utf8");
    await page.getByRole("button", { name: "更多操作" }).click();
    await page.getByRole("menuitem", { name: "从受管名单移除", exact: true }).click();
    await page.getByRole("button", { name: "确认移除", exact: true }).click();
    await page.getByRole("alert").filter({ hasText: "请先停止应用" }).waitFor();
    assert.equal(await readFile(app.configPath, "utf8"), before);
    assert.equal((await ipc(page, "list_session_configs")).length, 2);
    console.log("PASS running removal refused without changes");
    await page.getByRole("button", { name: "取消", exact: true }).click();
    await ipc(page, "force_stop_session", { sessionId: "remove-me" });
    await until(
      () => ipc(page, "list_sessions"),
      (sessions) =>
        ["stopped", "exited", "error"].includes(
          sessions.find((s) => s.sessionId === "remove-me").status,
        ),
      "Stop did not settle",
    );
    await page.getByRole("button", { name: "更多操作" }).click();
    await page.getByRole("menuitem", { name: "从受管名单移除", exact: true }).click();
    await page.getByRole("button", { name: "确认移除", exact: true }).click();
    await until(
      async () => {
        const alert = await page.getByRole("alert").allTextContents();
        if (alert.length) throw new Error(alert.join(" "));
        return page.locator(".session-row").filter({ hasText: "Remove Me" }).count();
      },
      (count) => count === 0,
      "Removed row still visible",
    );
    const saved = await readFile(app.configPath, "utf8");
    assert.ok(saved.includes("# preserve header") && saved.includes("# preserve second"));
    assert.ok(saved.includes("id: keep-me") && !saved.includes("id: remove-me"));
    assert.equal((await ipc(page, "list_session_configs")).length, 1);
    console.log("PASS real UI removal and preserved other configuration");
    await app.stop();
    app = undefined;
    app = await launch("removal");
    const restored = await ipc(app.page, "list_session_configs");
    assert.deepEqual(
      restored.map((s) => s.id),
      ["keep-me"],
    );
    console.log("PASS removed entry stays absent after native restart");
    await app.page.screenshot({ path: join(output, "after-restart.png") });
    return {
      cancellation: true,
      runningRemovalRefused: true,
      otherEntriesPreserved: true,
      absentAfterRestart: true,
    };
  } finally {
    if (app) await app.stop();
  }
}

async function verifyExistingFlows() {
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

  let daily;
  try {
    daily = await launch("daily", "# preserved comment\nsessions:\n");
    let page = daily.page;
    let terminals;
    await check(
      "H03/H07: empty workspace creates distinct focused temporary terminals",
      async () => {
        const before = await readFile(daily.configPath);
        for (let count = 1; count <= 2; count++) {
          await page.getByRole("button", { name: "新建 PowerShell", exact: true }).last().click();
          await until(
            () => ipc(page, "list_sessions"),
            (rows) => rows.length === count && rows.every((row) => row.status === "running"),
            "Quick terminal did not start",
          );
          await until(
            () =>
              page
                .locator(".xterm-helper-textarea")
                .evaluate((node) => node === document.activeElement),
            Boolean,
            "Quick terminal did not receive focus",
          );
        }
        terminals = await ipc(page, "list_sessions");
        assert.notEqual(terminals[0].pid, terminals[1].pid);
        assert.deepEqual(await readFile(daily.configPath), before);
        for (const row of terminals) {
          const info = await ipc(page, "get_log_info", { sessionId: row.sessionId });
          assert.equal(info.state, "off");
        }
        await screenshot(page, "daily-two-terminals");
        return terminals;
      },
    );
    const selected = await page.locator(".session-row--selected").innerText();
    const configs = await ipc(page, "list_session_configs");
    const active = configs.find((config) => selected.includes(config.name));
    assert.ok(active);
    await check("H05: keyboard execution, Ctrl+C and subsequent execution", async () => {
      await typeCommand(page, "Write-Output ('DAILY69_' + 'READY')");
      await until(
        () => terminalText(page, active.id),
        (text) => text.includes("DAILY69_READY"),
        "Shell input failed",
      );
      await typeCommand(page, "Write-Output ('DAILY69_' + 'WAIT'); Start-Sleep 120");
      await until(
        () => terminalText(page, active.id),
        (text) => text.includes("DAILY69_WAIT"),
        "Long command did not begin",
      );
      await page.keyboard.press("Control+c");
      try {
        await until(
          () => terminalText(page, active.id),
          (text) => /PS [^\r\n]*>/.test(text.split("DAILY69_WAIT").at(-1)),
          "Interrupted shell did not return to its prompt",
        );
      } catch (error) {
        await writeFile(
          join(output, "daily-interrupt-failure.txt"),
          await terminalText(page, active.id),
        );
        await screenshot(page, "daily-interrupt-failure");
        throw error;
      }
      await typeCommand(page, "Write-Output ('DAILY69_' + 'INTERRUPTED')");
      await until(
        () => terminalText(page, active.id),
        (text) => text.includes("DAILY69_INTERRUPTED"),
        "Ctrl+C did not release the shell",
      );
      assert.equal((await ipc(page, "get_session", { sessionId: active.id })).status, "running");
      await screenshot(page, "daily-ctrl-c");
    });
    await check(
      "H15: OS window close hides Hub while terminal output continues and entry restores interaction",
      async () => {
        await typeCommand(
          page,
          "1..6 | ForEach-Object { Write-Output ('HIDDEN69_' + $_); Start-Sleep 1 }",
        );
        await until(
          () => terminalText(page, active.id),
          (text) => text.includes("HIDDEN69_1"),
          "Continuous output did not start",
        );
        const hidden = execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-Command",
            `Add-Type -Name Native -Namespace Lch69 -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr h,uint m,System.IntPtr w,System.IntPtr l); [System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);'; $h=(Get-Process -Id ${daily.launcherPid}).MainWindowHandle; if ($h -eq 0) { throw 'No Hub window' }; [Lch69.Native]::PostMessage($h,0x10,[IntPtr]::Zero,[IntPtr]::Zero) | Out-Null; $limit=(Get-Date).AddSeconds(5); while ([Lch69.Native]::IsWindowVisible($h) -and (Get-Date) -lt $limit) { Start-Sleep -Milliseconds 100 }; @{handle=$h.ToInt64();visible=[Lch69.Native]::IsWindowVisible($h)} | ConvertTo-Json -Compress`,
          ],
          { encoding: "utf8", windowsHide: true },
        );
        const state = JSON.parse(hidden);
        assert.equal(state.visible, false);
        await until(
          () => terminalText(page, active.id),
          (text) => text.includes("HIDDEN69_6"),
          "Hidden shell stopped producing output",
        );
        execFileSync(binary, [], {
          env: { ...process.env, LCH_ACCEPTANCE_ROOT: daily.root },
          windowsHide: true,
          timeout: 20000,
        });
        const visible = execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-Command",
            `Add-Type -Name Native -Namespace Lch69 -MemberDefinition '[System.Runtime.InteropServices.DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);'; [Lch69.Native]::IsWindowVisible([IntPtr]${state.handle})`,
          ],
          { encoding: "utf8", windowsHide: true },
        );
        assert.equal(visible.trim(), "True");
        await typeCommand(page, "Write-Output ('HIDDEN69_' + 'RESTORED')");
        await until(
          () => terminalText(page, active.id),
          (text) => text.includes("HIDDEN69_RESTORED"),
          "Restored terminal was not interactive",
        );
        await screenshot(page, "daily-hidden-restored");
        return {
          hubPid: daily.launcherPid,
          windowHandle: state.handle,
          producedWhileHidden: "HIDDEN69_6",
          restoredInteraction: true,
          boundary: "OS WM_CLOSE and executable entry; tray gesture remains manual",
        };
      },
    );
    await check(
      "H08: save terminal launch method without restarting or recording commands",
      async () => {
        const before = await ipc(page, "get_session", { sessionId: active.id });
        await page.getByRole("button", { name: "更多操作" }).click();
        await page.getByRole("menuitem", { name: "保存启动配置" }).click();
        await page.getByRole("dialog").getByLabel(/名称/).fill("Daily Saved Shell");
        await page.getByRole("button", { name: "保存配置", exact: true }).click();
        await page.getByRole("dialog").waitFor({ state: "hidden" });
        assert.equal((await ipc(page, "get_session", { sessionId: active.id })).pid, before.pid);
        const text = await readFile(daily.configPath, "utf8");
        assert.ok(text.startsWith("# preserved comment\nsessions:\n"));
        assert.ok(text.includes("Daily Saved Shell"));
        assert.doesNotMatch(text, /DAILY69|Start-Sleep|Write-Output/);
        await screenshot(page, "daily-saved-terminal");
        return {
          sessionId: active.id,
          pid: before.pid,
          configSha256: await hashFile(daily.configPath),
        };
      },
    );
    await check(
      "H09/H10: add and activate an internal application through the real form",
      async () => {
        await page.getByRole("button", { name: "添加应用", exact: true }).click();
        const dialog = page.getByRole("dialog");
        await dialog.getByLabel(/名称/).fill("Daily Internal App");
        await dialog.getByLabel(/工作目录/).fill(repository);
        await dialog.getByLabel(/启动命令/).fill("cmd.exe /c ping -n 120 127.0.0.1");
        await dialog.getByRole("radio", { name: "Hub 内显示", exact: true }).click();
        await dialog.getByRole("button", { name: "保存应用", exact: true }).click();
        await dialog.waitFor({ state: "hidden" });
        await select(page, "Daily Internal App");
        await start(page);
        const config = (await ipc(page, "list_session_configs")).find(
          (item) => item.name === "Daily Internal App",
        );
        const before = await ipc(page, "get_session", { sessionId: config.id });
        await Promise.all([
          ipc(page, "activate_session", { sessionId: config.id }),
          ipc(page, "activate_session", { sessionId: config.id }),
        ]);
        assert.equal((await ipc(page, "get_session", { sessionId: config.id })).pid, before.pid);
        await screenshot(page, "daily-internal-app");
        return { sessionId: config.id, pid: before.pid };
      },
    );
    await check(
      "H09: read-only config refuses real form save without changing file or registry",
      async () => {
        const before = await hashFile(daily.configPath);
        const entries = await ipc(page, "list_session_configs");
        execFileSync("attrib.exe", ["+R", daily.configPath], { windowsHide: true });
        try {
          await page.getByRole("button", { name: "添加应用", exact: true }).click();
          const dialog = page.getByRole("dialog");
          await dialog.getByLabel(/名称/).fill("Read Only Must Not Save");
          await dialog.getByLabel(/工作目录/).fill(repository);
          await dialog.getByLabel(/启动命令/).fill("cmd.exe /c ping -n 120 127.0.0.1");
          await dialog.getByRole("button", { name: "保存应用", exact: true }).click();
          await dialog.getByRole("alert").waitFor();
          const error = await dialog.getByRole("alert").innerText();
          assert.ok(error.length > 0);
          assert.equal(await hashFile(daily.configPath), before);
          assert.deepEqual(await ipc(page, "list_session_configs"), entries);
          await screenshot(page, "daily-read-only-refused");
          await dialog.getByRole("button", { name: "取消", exact: true }).click();
          return { error, configurationUnchanged: true, registryUnchanged: true };
        } finally {
          execFileSync("attrib.exe", ["-R", daily.configPath], { windowsHide: true });
        }
      },
    );
    await check("H09: malformed external edit refuses save and preserves every byte", async () => {
      const before = await readFile(daily.configPath);
      const broken = "# external edit\nsessions: [\n";
      await writeFile(daily.configPath, broken);
      try {
        await page.getByRole("button", { name: "添加应用", exact: true }).click();
        const dialog = page.getByRole("dialog");
        await dialog.getByLabel(/名称/).fill("Must Not Save");
        await dialog.getByLabel(/工作目录/).fill(repository);
        await dialog.getByLabel(/启动命令/).fill("cmd.exe /c ping -n 120 127.0.0.1");
        await dialog.getByRole("button", { name: "保存应用", exact: true }).click();
        await dialog.getByRole("alert").waitFor();
        assert.match(await dialog.getByRole("alert").innerText(), /YAML/);
        assert.equal(await readFile(daily.configPath, "utf8"), broken);
        assert.ok(
          !(await ipc(page, "list_session_configs")).some(
            (config) => config.name === "Must Not Save",
          ),
        );
        await screenshot(page, "daily-save-refused");
        await dialog.getByRole("button", { name: "取消", exact: true }).click();
      } finally {
        await writeFile(daily.configPath, before);
      }
    });
    await check(
      "H08/H14: closing a terminal reclaims its child and preserves other work",
      async () => {
        const other = configs.find((config) => config.id !== active.id);
        const ownedPid = terminals.find((row) => row.sessionId === other.id).pid;
        const childFile = join(daily.root, "terminal-child.pid");
        const childScript = join(daily.root, "terminal-child.ps1");
        await writeFile(
          childScript,
          `[IO.File]::WriteAllText('${childFile.replaceAll("'", "''")}', [string]$PID); Start-Sleep 120`,
        );
        const sentinel = spawn("powershell.exe", ["-NoProfile", "-Command", "Start-Sleep 120"], {
          windowsHide: true,
          stdio: "ignore",
        });
        let childPid;
        try {
          assert.ok(sentinel.pid);
          await select(page, other.name);
          await typeCommand(
            page,
            `powershell.exe -NoProfile -File '${childScript.replaceAll("'", "''")}'`,
          );
          childPid = await until(
            async () => {
              try {
                return Number(await readFile(childFile, "utf8"));
              } catch (error) {
                if (error.code === "ENOENT") return null;
                throw error;
              }
            },
            (pid) => Number.isInteger(pid) && pid > 0 && isAlive(pid),
            "Terminal child did not acknowledge startup",
          );
          const survivor = await ipc(page, "get_session", { sessionId: active.id });
          await page
            .locator(".session-header")
            .getByRole("button", { name: "停止", exact: true })
            .click();
          await until(
            () => ipc(page, "get_session", { sessionId: other.id }),
            (runtime) => runtime.status === "stopped",
            "Closed terminal did not end",
          );
          assert.equal(isAlive(ownedPid), false);
          assert.equal(isAlive(childPid), false);
          assert.equal(isAlive(sentinel.pid), true);
          assert.equal(
            (await ipc(page, "get_session", { sessionId: active.id })).pid,
            survivor.pid,
          );
          assert.equal(isAlive(survivor.pid), true);
          assert.ok((await terminalText(page, other.id)).length > 0);
          await screenshot(page, "daily-closed-tree");
          await page.getByRole("button", { name: "更多操作" }).click();
          await page.getByRole("menuitem", { name: /移除/ }).click();
          await until(
            () => ipc(page, "list_sessions"),
            (rows) => !rows.some((row) => row.sessionId === other.id),
            "Removed terminal remains registered",
          );
          await page.reload();
          await page.locator(".session-row").first().waitFor();
          assert.ok(
            !(await ipc(page, "list_session_configs")).some((config) => config.id === other.id),
          );
          return {
            shellPid: ownedPid,
            childPid,
            survivorPid: survivor.pid,
            sentinelPid: sentinel.pid,
          };
        } catch (error) {
          await writeFile(
            join(output, "daily-tree-failure.txt"),
            await terminalText(page, other.id),
          );
          await screenshot(page, "daily-tree-failure");
          throw error;
        } finally {
          if (sentinel.pid && isAlive(sentinel.pid)) sentinel.kill();
          if (sentinel.pid)
            await until(
              async () => isAlive(sentinel.pid),
              (alive) => !alive,
              "Sentinel cleanup failed",
            );
          // On assertion failure the app's own job cleanup still owns childPid.
        }
      },
    );
    await check(
      "H08/H09: restart restores saved configurations and discards unsaved terminals",
      async () => {
        await page.getByRole("button", { name: "新建 PowerShell", exact: true }).last().click();
        const withUnsaved = await until(
          () => ipc(page, "list_session_configs"),
          (entries) => entries.some((entry) => entry.temporary),
          "No unsaved terminal exists for the restart check",
        );
        const unsavedId = withUnsaved.find((entry) => entry.temporary).id;
        const unsaved = await until(
          () => ipc(page, "get_session", { sessionId: unsavedId }),
          (runtime) => runtime.status === "running" && runtime.pid !== null && isAlive(runtime.pid),
          "Unsaved terminal did not start before restart",
        );
        await daily.stop();
        assert.equal(isAlive(unsaved.pid), false);
        daily = await launch("daily", undefined);
        page = daily.page;
        const restored = await ipc(page, "list_session_configs");
        assert.deepEqual(restored.map((config) => config.name).sort(), [
          "Daily Internal App",
          "Daily Saved Shell",
        ]);
        assert.ok(restored.every((config) => !config.temporary));
        assert.ok(!restored.some((config) => config.id === unsavedId));
        const restoredShell = restored.find((config) => config.id === active.id);
        assert.equal(restoredShell.shell, active.shell);
        assert.equal(restoredShell.cwd, active.cwd);
        await select(page, "Daily Saved Shell");
        await start(page);
        await until(
          () => terminalText(page, active.id),
          (text) => /PS [^\r\n]*>/.test(text),
          "Restored shell did not finish startup",
        );
        assert.ok(!(await terminalText(page, active.id)).includes("DAILY69_READY"));
        await typeCommand(page, "Write-Output ('DAILY69_' + 'RESTORED')");
        await until(
          () => terminalText(page, active.id),
          (text) => text.includes("DAILY69_RESTORED"),
          "Saved shell did not execute",
        );
        assert.notEqual(
          (await ipc(page, "get_session", { sessionId: active.id })).pid,
          terminals.find((row) => row.sessionId === active.id).pid,
        );
        await screenshot(page, "daily-restored");
        return { restored, discardedUnsavedId: unsavedId, discardedUnsavedPid: unsaved.pid };
      },
    );
  } finally {
    if (daily) await daily.stop();
  }

  await check(
    "H06: real shell executes in Unicode cwd and invalid cwd creates no session",
    async () => {
      const app = await launch("directory", "sessions:\n");
      try {
        const cwd = join(app.root, "work space 工作目录");
        await mkdir(cwd);
        const created = await ipc(app.page, "create_temporary_terminal", { cwd });
        assert.match(created.config.shell, /pwsh\.exe|powershell\.exe/i);
        await select(app.page, created.config.name);
        await typeCommand(
          app.page,
          "Write-Output ('FALLBACK69_' + 'READY'); (Get-Location).Path; $PSVersionTable.PSVersion.Major",
        );
        const text = await until(
          () => terminalText(app.page, created.config.id),
          (value) =>
            value.includes("FALLBACK69_READY") &&
            value.includes(cwd) &&
            /[\r\n][57][\r\n]/.test(value),
          "Fallback shell did not execute in requested directory",
        );
        assert.match(text, /[\r\n][57][\r\n]/);
        const missing = join(app.root, "missing-directory");
        // Capture Tauri's structured rejection inside WebView2; Playwright's
        // exception wrapper otherwise reduces it to "page.evaluate: Object".
        const refused = await app.page.evaluate(async (cwd) => {
          try {
            await globalThis.__TAURI_INTERNALS__.invoke("create_temporary_terminal", { cwd });
            return { ok: true };
          } catch (error) {
            return { ok: false, error };
          }
        }, missing);
        assert.equal(refused.ok, false);
        assert.match(JSON.stringify(refused.error), /missing-directory/);
        assert.equal((await ipc(app.page, "list_sessions")).length, 1);
        await screenshot(app.page, "fallback-unicode-directory");
        return {
          shell: created.config.shell,
          cwd,
          output: text,
          missingDirectoryCreatedNoSession: true,
          invalidDirectoryResponse: refused.error,
          environmentBoundary:
            "Installed shell exercised; no-PowerShell-7 native acceptance remains not run",
        };
      } finally {
        await app.stop();
      }
    },
  );

  await check(
    "H12: actual executable recommendation remains manually changeable in real form",
    async () => {
      const app = await launch("recommendation", "sessions:\n");
      try {
        const dialog = app.page.getByRole("dialog");
        await app.page.getByRole("button", { name: "添加应用", exact: true }).click();
        await dialog.getByLabel(/名称/).fill("ComfyUI Name Is Not Identity");
        await dialog.getByLabel(/工作目录/).fill(repository);
        await dialog.getByLabel(/启动命令/).fill("cmd.exe /c ping -n 120 127.0.0.1");
        const advice = await ipc(app.page, "recommend_display", {
          command: "cmd.exe /c ping -n 120 127.0.0.1",
          cwd: repository,
        });
        assert.equal(advice.recommended, "window");
        await until(
          () =>
            dialog
              .getByRole("radio", { name: "独立窗口", exact: true })
              .getAttribute("aria-checked"),
          (value) => value === "true",
          "Recommended mode not applied",
        );
        await dialog.getByRole("radio", { name: "Hub 内显示", exact: true }).click();
        await dialog.getByRole("button", { name: "保存应用", exact: true }).click();
        await dialog.waitFor({ state: "hidden" });
        const config = (await ipc(app.page, "list_session_configs"))[0];
        assert.equal(config.display, "internal");
        await select(app.page, config.name);
        await start(app.page);
        assert.ok((await ipc(app.page, "get_session", { sessionId: config.id })).pid);
        const unknown = await ipc(app.page, "recommend_display", {
          command: "unknown-launcher-69.exe",
          cwd: repository,
        });
        assert.equal(unknown.recommended, undefined);
        await screenshot(app.page, "recommendation-user-choice");
        return { advice, savedDisplay: config.display, unknownAdvice: unknown };
      } finally {
        await app.stop();
      }
    },
  );

  await check(
    "H11: external identity association does not create or acquire ownership of another process",
    async () => {
      const fixtureDir = join(output, "external-fixture");
      await mkdir(fixtureDir);
      const executable = join(fixtureDir, "lch69-external.exe");
      await copyFile(join(process.env.SystemRoot, "System32", "ping.exe"), executable);
      const command = `"${executable}" -n 120 127.0.0.1`;
      const config = `sessions:\n  - id: external69\n    name: External69\n    type: service\n    command: '${command.replaceAll("'", "''")}'\n    cwd: '${repository.replaceAll("'", "''")}'\n    display: window\n    lifecycle: independent\n    logging: {mode: off, source: none}\n`;
      const outside = spawn(executable, ["-n", "120", "127.0.0.1"], {
        windowsHide: true,
        stdio: "ignore",
      });
      let app;
      try {
        assert.ok(outside.pid);
        app = await launch("external-association", config);
        await select(app.page, "External69");
        const result = await ipc(app.page, "activate_session", { sessionId: "external69" });
        const runtime = await ipc(app.page, "get_session", { sessionId: "external69" });
        assert.equal(runtime.pid, outside.pid);
        assert.equal(runtime.external, true);
        const repeated = await ipc(app.page, "activate_session", { sessionId: "external69" });
        assert.equal(
          (await ipc(app.page, "get_session", { sessionId: "external69" })).pid,
          outside.pid,
        );
        await screenshot(app.page, "external-associated");
        // Stop the Hub first, then verify that association granted no termination ownership.
        await app.stop();
        app = undefined;
        assert.equal(isAlive(outside.pid), true);
        return {
          externalPid: outside.pid,
          result,
          repeated,
          survivedHubCleanup: true,
          boundary:
            "No-window console fixture; actual launcher foreground switching remains manual",
        };
      } finally {
        if (app) await app.stop();
        if (outside.pid && isAlive(outside.pid)) outside.kill();
        if (outside.pid)
          await until(
            async () => isAlive(outside.pid),
            (alive) => !alive,
            "External fixture cleanup failed",
          );
      }
    },
  );

  await check(
    "application form: real directory scan, URL persistence and option colors",
    async () => {
      const app = await launch("form-discovery", "sessions:\n");
      try {
        const directory = join(app.root, "示例应用 空格");
        await mkdir(directory, { recursive: true });
        await writeFile(
          join(directory, "启动.cmd"),
          "@echo off\necho NEVER_EXECUTED\npython main.py --port 8189\n",
        );
        const page = app.page;
        const dialog = page.getByRole("dialog");
        await page.getByRole("button", { name: "添加应用", exact: true }).click();
        const scan = await ipc(page, "scan_application_directory", { cwd: directory });
        assert.equal(scan.candidates.length, 1);
        assert.equal(scan.candidates[0].port, 8189);
        assert.equal(scan.cwd, directory);
        // Native selection and its automatic prefill remain a manual boundary.
        await dialog.getByLabel(/名称/).fill(scan.name);
        await dialog.getByLabel(/工作目录/).fill(scan.cwd);
        await dialog.getByLabel(/启动命令/).fill(scan.candidates[0].command);
        await dialog.getByLabel("端口", { exact: true }).fill(String(scan.candidates[0].port));
        assert.equal(await dialog.getByLabel("端口", { exact: true }).inputValue(), "8189");
        assert.equal(
          await dialog.getByLabel("网页地址", { exact: true }).inputValue(),
          "127.0.0.1:8189",
        );
        await dialog.getByLabel("端口", { exact: true }).fill("8190");
        await until(
          () => dialog.getByLabel("网页地址", { exact: true }).inputValue(),
          (value) => value.endsWith(":8190"),
          "URL did not follow changed port",
        );
        await dialog.getByLabel("网页地址", { exact: true }).fill("https://example.com/my-app");
        await dialog.getByLabel("端口", { exact: true }).fill("8191");
        assert.equal(
          await dialog.getByLabel("网页地址", { exact: true }).inputValue(),
          "example.com/my-app",
        );
        assert.equal(await dialog.getByLabel("地址协议", { exact: true }).inputValue(), "https://");
        const colors = await dialog.getByLabel("日志策略").evaluate((select) => {
          const option = document.defaultView.getComputedStyle(select.options[1]);
          return {
            scheme: document.defaultView.getComputedStyle(select).colorScheme,
            foreground: option.color,
            background: option.backgroundColor,
          };
        });
        assert.equal(colors.scheme, "dark");
        assert.notEqual(colors.foreground, colors.background);
        assert.match(colors.background, /^rgb\(/, "Popup options need an opaque background");
        await screenshot(page, "application-discovery-prefill");
        await dialog.getByRole("button", { name: "保存应用", exact: true }).click();
        await dialog.waitFor({ state: "hidden" });
        const saved = (await ipc(page, "list_session_configs"))[0];
        assert.equal(saved.cwd, directory);
        assert.equal(saved.url, "https://example.com/my-app");
        assert.equal(saved.port, 8191);
        assert.equal((await ipc(page, "get_session", { sessionId: saved.id })).pid, null);
        return {
          nativePicker: "not run; directory scan checked over real IPC",
          saved,
          colors,
          neverStarted: true,
          boundary:
            "Native folder/file selection, automatic candidate prefill and popup visual contrast remain manual",
        };
      } finally {
        await app.stop();
      }
    },
  );

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
}

try {
  await check(
    "saved application removal: cancel, running refusal, persistence and restart",
    verifyApplicationRemoval,
  );
  if (process.argv[3] !== "--removal-only") await verifyExistingFlows();
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
