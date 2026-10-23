// SPDX-License-Identifier: GPL-3.0-or-later
// Tessellate the local STEP shell and buttons (3d/, never committed) into a
// mesh cache the renderer loads. Run once after placing the STEP files.
//   node art/mouse-render/convert.mjs
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const occt = await createRequire(import.meta.url)("occt-import-js")();
const SOURCES = [["shell", "3d/MxMaster3v58.step"], ["buttons", "3d/MouseButtonsMxM3v68.step"]];
const parts = [];
const chunks = [];
let offset = 0;
for (const [group, file] of SOURCES) {
  const result = occt.ReadStepFile(new Uint8Array(readFileSync(join(ROOT, file))), { linearUnit: "millimeter", linearDeflectionType: "bounding_box_ratio", linearDeflection: 0.0006, angularDeflection: 0.25 });
  if (!result.success) throw new Error(`Cannot read ${file}`);
  for (const mesh of result.meshes) {
    const arrays = { position: new Float32Array(mesh.attributes.position.array), normal: new Float32Array(mesh.attributes.normal.array), index: new Uint32Array(mesh.index.array) };
    const entry = { group, name: mesh.name };
    for (const [key, array] of Object.entries(arrays)) {
      entry[key] = { offset, length: array.length };
      chunks.push(Buffer.from(array.buffer));
      offset += array.byteLength;
    }
    parts.push(entry);
  }
}
const cache = join(HERE, "cache");
mkdirSync(cache, { recursive: true });
writeFileSync(join(cache, "parts.bin"), Buffer.concat(chunks));
writeFileSync(join(cache, "parts.json"), JSON.stringify(parts));
console.log(parts.map((p) => `${p.group}/${p.name}: ${p.index.length / 3} triangles`).join("\n"));
