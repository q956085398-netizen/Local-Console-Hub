// Capture the V2 reference states from a built frontend, so the visual
// acceptance check in `docs/VERIFICATION.md` is something a person (or an
// agent) can reproduce in seconds rather than reconstruct by clicking.
//
// It renders the *fixture workspace* — the six sessions and three groups the
// reference images show — because that is what the references depict and what
// `src/state/fixtures.ts` exists for (`docs/DECISIONS.md` D-020). The built
// `dist/` is served over loopback and driven in Microsoft Edge, which ships
// with Windows, so the only thing to install is the driver:
//
//   npm run build
//   npm install --no-save puppeteer-core
//   node scripts/capture-ui-states.mjs [output-directory]
//
// Output defaults to `%TEMP%\lch-ui-capture`. Compare each file against the
// reference named beside it in the console output, and against the deliberate
// deviations `docs/DESIGN_SPEC_EXTRACTED.md` §5 records — an `UI 预览`
// badge, `External` where the reference shows `Auto`, and fixture-relative
// uptimes are all expected, not defects.
//
// Since #68 the title bar also renders the window's own minimize / maximize /
// close controls, because the desktop window is undecorated and the bar *is*
// its title bar. In this capture they are present but inert — a browser page
// has no window to control — so what the capture can show about them is their
// place and their weight in the bar. Whether they move the real window is a
// desktop-only check (`docs/VERIFICATION.md` §4, W-1…W-6).

import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { mkdir, readFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { extname, join, normalize, resolve } from "node:path";

import puppeteer from "puppeteer-core";

const DIST = resolve(import.meta.dirname, "..", "dist");
const OUT = resolve(process.argv[2] ?? join(tmpdir(), "lch-ui-capture"));

/** The window size `src-tauri/tauri.conf.json` opens with. */
const VIEWPORT = { width: 1280, height: 800 };

/** Edge ships with Windows; anything else can be named explicitly. */
const EDGE_CANDIDATES = [
  process.env.EDGE_PATH,
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
].filter(Boolean);

const CONTENT_TYPES = {
  ".css": "text/css",
  ".html": "text/html",
  ".js": "text/javascript",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

/**
 * Serve `dist/` on an ephemeral loopback port.
 *
 * The build emits absolute asset paths (no `base` in `vite.config.ts`), so a
 * `file://` load would not find its own bundle; a real origin is the shortest
 * way to the same bytes a window would load.
 */
async function serveDist() {
  const server = createServer(async (request, response) => {
    const requested = decodeURIComponent(new URL(request.url, "http://x").pathname);
    const candidate = join(DIST, normalize(requested).replace(/^(\.\.[/\\])+/, ""));
    const path =
      candidate === DIST || candidate.endsWith("\\") ? join(candidate, "index.html") : candidate;

    try {
      const body = await readFile(path);
      response.writeHead(200, {
        "content-type": CONTENT_TYPES[extname(path)] ?? "application/octet-stream",
      });
      response.end(body);
    } catch {
      // A single-page app: anything the build did not emit is a route, and the
      // shell is what serves it.
      response.writeHead(200, { "content-type": "text/html" });
      response.end(await readFile(join(DIST, "index.html")));
    }
  });

  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  return { server, origin: `http://127.0.0.1:${server.address().port}` };
}

/** Click the sidebar row whose text names `session`, then let the pane settle. */
async function selectSession(page, session) {
  const clicked = await page.evaluate((name) => {
    const row = [...document.querySelectorAll(".session-row")].find((candidate) =>
      candidate.textContent?.includes(name),
    );
    row?.click();
    return Boolean(row);
  }, session);
  if (!clicked) {
    throw new Error(`no sidebar row names \`${session}\` — did the fixture cast change?`);
  }
  await new Promise((done) => setTimeout(done, 150));
}

/** Click one of the Terminal / Logs / Details tabs. */
async function selectTab(page, label) {
  const clicked = await page.evaluate((wanted) => {
    const tab = [...document.querySelectorAll('[role="tab"]')].find((candidate) =>
      candidate.textContent?.includes(wanted),
    );
    tab?.click();
    return Boolean(tab);
  }, label);
  if (!clicked) {
    throw new Error(`no tab labelled \`${label}\``);
  }
  await new Promise((done) => setTimeout(done, 150));
}

async function main() {
  if (!existsSync(join(DIST, "index.html"))) {
    throw new Error(`no build at ${DIST} — run \`npm run build\` first`);
  }
  const executablePath = EDGE_CANDIDATES.find((path) => existsSync(path));
  if (!executablePath) {
    throw new Error(`no Edge found; set EDGE_PATH to a Chromium-based browser`);
  }
  if (!existsSync(OUT) || !(await stat(OUT)).isDirectory()) {
    await mkdir(OUT, { recursive: true });
  }

  const { server, origin } = await serveDist();
  const browser = await puppeteer.launch({ executablePath, headless: true });

  try {
    const page = await browser.newPage();
    await page.setViewport(VIEWPORT);
    await page.goto(origin, { waitUntil: "networkidle0" });
    await page.waitForSelector(".session-row");

    const shots = [
      ["service-terminal", "ComfyUI", "终端", "assets/ui/ui-v2-service.png"],
      ["service-logs", "ComfyUI", "日志", "— (UI_STYLE_GUIDE §8)"],
      ["service-details", "ComfyUI", "详情", "— (UI_STYLE_GUIDE §6)"],
      ["terminal-terminal", "PowerShell", "终端", "assets/ui/ui-v2-terminal.png"],
      ["terminal-logs", "PowerShell", "日志", "— (UI_STYLE_GUIDE §8)"],
    ];

    for (const [name, session, tab, reference] of shots) {
      await selectSession(page, session);
      await selectTab(page, tab);
      const path = join(OUT, `${name}.png`);
      await page.screenshot({ path });
      console.log(`${name.padEnd(20)} ${session} / ${tab}  →  ${path}`);
      console.log(`${" ".repeat(20)}   compare with: ${reference}`);
    }
  } finally {
    await browser.close();
    server.close();
  }

  console.log(`\nwrote ${OUT}`);
}

await main();
