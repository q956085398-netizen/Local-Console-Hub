// Renders assets/brand/app-icon.svg to assets/brand/app-icon.png (1024×1024),
// the input expected by `npx tauri icon`.
//
// Usage:
//   npm install --no-save @resvg/resvg-js   # native rasterizer, not a permanent dep
//   node scripts/generate-icon.mjs
//   npx tauri icon assets/brand/app-icon.png

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { Resvg } from "@resvg/resvg-js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const svgPath = join(root, "assets", "brand", "app-icon.svg");
const pngPath = join(root, "assets", "brand", "app-icon.png");

const svg = readFileSync(svgPath, "utf8");
const resvg = new Resvg(svg, {
  fitTo: { mode: "width", value: 1024 },
});
const png = resvg.render().asPng();
writeFileSync(pngPath, png);
console.log(`wrote ${pngPath} (${png.length} bytes)`);
