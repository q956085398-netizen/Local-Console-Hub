// Reads the alpha of an icon's four corners, so "the corners are transparent"
// is a measurement rather than an assumption about the rasterizer.
//
// The Hub mark is a rounded tile with nothing painted behind it: every Windows
// surface (taskbar, tray, installed shortcut) is supposed to show its own
// background through the corners. A flattened white corner would be invisible
// in a light viewer and obvious on a dark taskbar, which is the kind of defect
// a picture of the *source* cannot catch. This reads the shipped bytes.
//
//   npm install --no-save puppeteer-core
//   node scripts/check-icon-alpha.mjs [icon…]
//
// Defaults to the bundle's own files (`tauri.conf.json`'s `bundle.icon` plus
// the sizes `npx tauri icon` writes). Chrome decodes PNG and ICO alike; set
// `EDGE_PATH` (or `CHROME_PATH`) to point at another Chromium-based browser.
//
// A corner passes when it is transparent or nearly so — a rounded corner is
// antialiased, so a pixel or two of alpha on the outermost edge is expected and
// harmless as long as what it blends towards is the tile, not white.

/* global Image */
// `Image` is the one browser global this file needs inside a `page.evaluate`
// callback (the shared eslint globals cover `document`, not the rest of the DOM
// surface) — see the scripts block in `eslint.config.js`.

import { existsSync, readFileSync } from "node:fs";
import { extname, resolve } from "node:path";

import puppeteer from "puppeteer-core";

const DEFAULT_ICONS = [
  "src-tauri/icons/icon.png",
  "src-tauri/icons/32x32.png",
  "src-tauri/icons/64x64.png",
  "src-tauri/icons/icon.ico",
];

/** Chrome ships everywhere Windows does; Edge is the fallback. */
const BROWSERS = [
  process.env.EDGE_PATH,
  process.env.CHROME_PATH,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
].filter(Boolean);

const CONTENT_TYPES = { ".png": "image/png", ".ico": "image/x-icon", ".svg": "image/svg+xml" };

/** How much alpha a corner may still have and count as transparent. */
const ALPHA_TOLERANCE = 8;

const icons = (process.argv.slice(2).length > 0 ? process.argv.slice(2) : DEFAULT_ICONS).map(
  (icon) => resolve(icon),
);

const executablePath = BROWSERS.find((path) => existsSync(path));
if (executablePath === undefined) {
  throw new Error("no Chromium-based browser found; set CHROME_PATH or EDGE_PATH");
}

const browser = await puppeteer.launch({ executablePath, headless: true });
let failed = false;

try {
  const page = await browser.newPage();
  await page.goto("about:blank");

  for (const icon of icons) {
    if (!existsSync(icon)) {
      console.log(`MISSING ${icon}`);
      failed = true;
      continue;
    }

    const type = CONTENT_TYPES[extname(icon).toLowerCase()] ?? "application/octet-stream";
    const url = `data:${type};base64,${readFileSync(icon).toString("base64")}`;

    const report = await page.evaluate(
      (source, tolerance) =>
        new Promise((done, fail) => {
          const image = new Image();
          image.onerror = () => fail(new Error("the image did not decode"));
          image.onload = () => {
            const canvas = document.createElement("canvas");
            canvas.width = image.naturalWidth;
            canvas.height = image.naturalHeight;
            const context = canvas.getContext("2d");
            context.drawImage(image, 0, 0);

            const at = (x, y) => [...context.getImageData(x, y, 1, 1).data];
            const { width, height } = canvas;
            const corners = {
              topLeft: at(0, 0),
              topRight: at(width - 1, 0),
              bottomLeft: at(0, height - 1),
              bottomRight: at(width - 1, height - 1),
            };
            const worst = Math.max(...Object.values(corners).map((rgba) => rgba[3]));

            done({
              size: `${width}x${height}`,
              corners,
              inside: at(Math.round(width * 0.12), Math.round(height * 0.12)),
              pass: worst <= tolerance,
            });
          };
          image.src = source;
        }),
      url,
      ALPHA_TOLERANCE,
    );

    const shown = Object.entries(report.corners)
      .map(([where, rgba]) => `${where}=${rgba[3]}`)
      .join(" ");
    console.log(
      `${report.pass ? "ok  " : "FAIL"} ${icon.replace(/\\/g, "/")}  ${report.size}  ${shown}  inside=rgba(${report.inside.join(",")})`,
    );
    if (!report.pass) {
      console.log(
        `     a corner is not transparent: alpha above ${ALPHA_TOLERANCE} — something is painted behind the tile`,
      );
      failed = true;
    }
  }
} finally {
  await browser.close();
}

process.exit(failed ? 1 : 0);
