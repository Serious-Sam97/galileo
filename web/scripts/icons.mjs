// Renders the app icons from SVG: apple-icon.png (180) and favicon.ico (16 + 32 + 48, PNG entries).
// Run after changing the mark: node scripts/icons.mjs
import { Resvg } from "@resvg/resvg-js";
import { readFileSync, writeFileSync } from "node:fs";

const detailed = readFileSync("src/app/icon.svg", "utf8");
// At 16 px the orbit line and the far moons turn to mud: planet and one moon only.
const tiny = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="12" fill="#1c0f38"/><circle cx="27" cy="32" r="17" fill="#ff5ccf"/><circle cx="52" cy="32" r="7" fill="#5ee9ff"/></svg>`;
const png = (svg, size) => new Resvg(svg, { fitTo: { mode: "width", value: size } }).render().asPng();

writeFileSync("src/app/apple-icon.png", png(detailed, 180));

const images = [[16, png(tiny, 16)], [32, png(detailed, 32)], [48, png(detailed, 48)]];
const header = Buffer.alloc(6); header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(images.length, 4);
let offset = 6 + 16 * images.length;
const entries = images.map(([size, data]) => {
  const e = Buffer.alloc(16);
  e.writeUInt8(size, 0); e.writeUInt8(size, 1); e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
  e.writeUInt32LE(data.length, 8); e.writeUInt32LE(offset, 12); offset += data.length;
  return e;
});
writeFileSync("src/app/favicon.ico", Buffer.concat([header, ...entries, ...images.map(([, d]) => d)]));
console.log("icons written");
