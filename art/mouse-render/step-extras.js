// SPDX-License-Identifier: GPL-3.0-or-later
// Parts the printable STEP shell leaves out, added in its own frame (x forward,
// y up, thumb side -z): the scroll wheel and mode button in their top cut-outs,
// the dark side insert behind the side opening with the thumb wheel and the
// back/forward keys, inner backers so no cut-out shows the hollow print, and a
// sole. Every placement is measured on the shell by ray casts.
const WHEEL = { x: 36.9, z: -0.2, r: 10.2, w: 6.2, proud: 3.4 };
const MODE = { x: 13.8, z: -0.2, w: 9.2, d: 5.6 };
// Above the Back/Forward keys, between them and the top rim, as on the real mouse.
const THUMB = { x: 1, y: 40.2, r: 6, len: 12 };
// Forward and Back sit in the two slots the print leaves in the thumb rest's
// upper lip (found as deep rays in the side grid), under the thumb wheel.
const KEYS = [{ x: 5, y: 28.5, len: 9, tilt: 0.31 }, { x: -7.5, y: 32.5, len: 9, tilt: 0.31 }];

export const STEP_ANCHORS = {};

export function stepExtras(THREE, root, { highlight, lit }) {
  const shell = root.children.find((m) => m.name === "solid");
  shell.updateMatrixWorld(true);
  const ray = new THREE.Raycaster();
  const hit = (origin, dir) => { ray.set(new THREE.Vector3(...origin), new THREE.Vector3(...dir).normalize()); const h = ray.intersectObject(shell, false)[0]; return h ? h.point : null; };
  const topAt = (x, z) => hit([x, 200, z], [0, -1, 0])?.y ?? 40;
  // Height beside a cut-out (a ray down its middle falls through the hole).
  const rimTop = (x, z, half) => (topAt(x, z - half - 1.5) + topAt(x, z + half + 1.5)) / 2;
  // Thumb side surface at (x, y): first hit coming from -z.
  const sideAt = (x, y) => hit([x, y, -200], [0, 0, 1])?.z ?? -18;

  const dark = new THREE.MeshStandardMaterial({ color: 0x101114, roughness: 0.7 });
  const insert = new THREE.MeshPhysicalMaterial({ color: 0x2a2d32, roughness: 0.32, metalness: 0.05, clearcoat: 0.55, clearcoatRoughness: 0.3, envMapIntensity: 0.75 });
  const steel = new THREE.MeshStandardMaterial({ color: 0x9aa0a6, metalness: 1, roughness: 0.34 });
  const add = (mesh) => { mesh.castShadow = true; mesh.receiveShadow = true; root.add(mesh); return mesh; };
  const knurl = (radius, width, ribs, depth) => {
    const g = new THREE.CylinderGeometry(radius, radius, width, ribs * 2, 1, false);
    const p = g.attributes.position;
    for (let v = 0; v < p.count; v++) {
      const x = p.getX(v), z = p.getZ(v);
      if (Math.hypot(x, z) < radius * 0.5) continue;
      const a = Math.atan2(z, x);
      const k = Math.round(((a + Math.PI) / (2 * Math.PI)) * ribs * 2) % 2 ? 1 : 1 - depth / radius;
      p.setX(v, x * k);
      p.setZ(v, z * k);
    }
    g.computeVertexNormals();
    return g;
  };
  const box = (w, h, d, r = 1) => {
    const shape = new THREE.Shape();
    shape.moveTo(-w / 2 + r, -d / 2);
    shape.lineTo(w / 2 - r, -d / 2); shape.quadraticCurveTo(w / 2, -d / 2, w / 2, -d / 2 + r);
    shape.lineTo(w / 2, d / 2 - r); shape.quadraticCurveTo(w / 2, d / 2, w / 2 - r, d / 2);
    shape.lineTo(-w / 2 + r, d / 2); shape.quadraticCurveTo(-w / 2, d / 2, -w / 2, d / 2 - r);
    shape.lineTo(-w / 2, -d / 2 + r); shape.quadraticCurveTo(-w / 2, -d / 2, -w / 2 + r, -d / 2);
    const g = new THREE.ExtrudeGeometry(shape, { depth: h, bevelEnabled: true, bevelThickness: Math.min(0.6, h / 4), bevelSize: Math.min(0.6, r / 2), bevelSegments: 3 });
    g.rotateX(-Math.PI / 2);
    return g;
  };

  // Scroll wheel: steel, knurled, proud of the top by WHEEL.proud.
  const wheelTop = rimTop(WHEEL.x, WHEEL.z, 4);
  const wheel = add(new THREE.Mesh(knurl(WHEEL.r, WHEEL.w, 56, 0.6), highlight === "wheel" ? lit(steel) : steel));
  wheel.rotation.x = Math.PI / 2;
  wheel.position.set(WHEEL.x, wheelTop + WHEEL.proud - WHEEL.r, WHEEL.z);
  // Dark backers under the top cut-outs.
  // Extrusions grow by their bevel above and below: top = y + depth + bevel.
  const flush = (mesh, top, depth, bevel) => { mesh.position.y = top - depth - bevel; return mesh; };
  // A dark well under the wheel, below the lowest point of the cut-out's rim.
  const wellTop = Math.min(rimTop(WHEEL.x - 9, WHEEL.z, 4), rimTop(WHEEL.x + 9, WHEEL.z, 4)) - 3;
  const backer = add(new THREE.Mesh(box(20, 2.5, 9, 2), dark));
  backer.position.set(WHEEL.x, 0, WHEEL.z);
  flush(backer, wellTop, 2.5, 0.6);
  const modeTop = rimTop(MODE.x, MODE.z, MODE.d / 2);
  // Mode button: flush with the shell, 0.3 below its rim, like the real key.
  const keyCap = new THREE.MeshPhysicalMaterial({ color: 0x26282c, roughness: 0.5, metalness: 0.1, clearcoat: 0.25, clearcoatRoughness: 0.5, envMapIntensity: 0.45 });
  const mode = add(new THREE.Mesh(box(MODE.w, 1.6, MODE.d, 1.6), keyCap));
  mode.position.set(MODE.x, 0, MODE.z);
  flush(mode, modeTop - 0.3, 1.6, 0.4);
  const modeBacker = add(new THREE.Mesh(box(11.5, 1.5, 7.5, 2), dark));
  modeBacker.position.set(MODE.x, 0, MODE.z);
  flush(modeBacker, modeTop - 2.6, 1.5, 0.375);

  // Side insert. The print leaves the thumb side open between the thumb rest
  // and the top rim, and the whole front side under the left button. Outline
  // traced on the orthographic side view of the bare shell (model x, y in mm).
  const OUTLINE = [
    [-20.5, 45.5], [-19, 47.2], [-11.3, 48.3], [-2.7, 46.5], [9.7, 43.1], [25, 35.8], [39, 28.6], [49, 23.6], [56.7, 18.5],
    [56.7, 8], [18.7, 8], [18.7, 12.8], [20.2, 23.9], [19.5, 25.3], [1.4, 33.6], [-11.1, 38.5], [-19.5, 42.5],
  ];
  const inside = (x, y) => {
    let c = false;
    for (let i = 0, j = OUTLINE.length - 1; i < OUTLINE.length; j = i++) {
      const [xi, yi] = OUTLINE[i], [xj, yj] = OUTLINE[j];
      if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
    }
    return c;
  };
  // Nearest point of the outline and the signed distance to it (negative inside).
  const nearest = (x, y) => {
    let best = { d: Infinity, px: x, py: y };
    for (let i = 0, j = OUTLINE.length - 1; i < OUTLINE.length; j = i++) {
      const [ax, ay] = OUTLINE[j], [bx, by] = OUTLINE[i], dx = bx - ax, dy = by - ay;
      const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)));
      const px = ax + t * dx, py = ay + t * dy, d = Math.hypot(x - px, y - py);
      if (d < best.d) best = { d, px, py };
    }
    if (inside(x, y)) best.d = -best.d;
    return best;
  };
  const edgeDist = (x, y) => nearest(x, y).d;
  // Depth: rays from the thumb side on a 1 mm grid; the outer surface around the
  // outline fixes the boundary and a Laplace fill spans the opening.
  const GX0 = -30, GX1 = 60, GY0 = 4, GY1 = 54, NX = GX1 - GX0, NY = GY1 - GY0;
  const at = (i, j) => j * (NX + 1) + i;
  const raw = [];
  // The left button's side face closes the front above the outline, so rays
  // see it too; hits deeper than the outer skin (the print's far wall) are dropped.
  const skin = [shell, ...root.children.filter((m) => m.name === "LeftMB")];
  const skinHit = (x, y) => { ray.set(new THREE.Vector3(x, y, -200), new THREE.Vector3(0, 0, 1)); return ray.intersectObjects(skin, false)[0]?.point.z ?? null; };
  for (let j = 0; j <= NY; j++) for (let i = 0; i <= NX; i++) raw.push(skinHit(GX0 + i, GY0 + j));
  // Shell only: what must stay behind the insert (the button's face must not).
  const rawShell = raw.map((_, k) => hit([GX0 + (k % (NX + 1)), GY0 + Math.floor(k / (NX + 1)), -200], [0, 0, 1])?.z ?? null);
  const sd = raw.map((_, k) => edgeDist(GX0 + (k % (NX + 1)), GY0 + Math.floor(k / (NX + 1))));
  const hole = sd.map((d) => d < 0.5);
  // The thumb rest's lip (z < -24) stands out past the panel and does not bound it.
  const fixed = raw.map((z, k) => !hole[k] && z !== null && sd[k] < 1.6 && z > -24 && z < (GX0 + (k % (NX + 1)) < 10.7 ? -12 : -2));
  const zs = raw.map((z, k) => (fixed[k] ? z : -20));
  // Stay in front of whatever the rays meet inside the opening (guides, rails):
  // the nearest hit within 2 mm.
  const limit = raw.map((_, k) => {
    if (!hole[k]) return Infinity;
    const i = k % (NX + 1), j = Math.floor(k / (NX + 1));
    let m = Infinity;
    for (let b = j - 2; b <= j + 2; b++) for (let a = i - 2; a <= i + 2; a++) {
      if (a < 0 || b < 0 || a > NX || b > NY) continue;
      // Only what lies inside the opening; the rims around it stay in front.
      const z = rawShell[at(a, b)];
      if (z !== null && hole[at(a, b)]) m = Math.min(m, z);
    }
    return m - 0.6;
  });
  for (let it = 0; it < 1500; it++) for (let j = 0; j <= NY; j++) for (let i = 0; i <= NX; i++) {
    const k = at(i, j);
    if (!hole[k]) continue;
    let sum = 0, n = 0;
    for (const [di, dj] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const a = i + di, b = j + dj;
      if (a < 0 || b < 0 || a > NX || b > NY) continue;
      const q = at(a, b);
      if (fixed[q] || hole[q]) { sum += zs[q]; n++; }
    }
    if (n) zs[k] = Math.min(sum / n, limit[k]);
  }
  // Soften the terraces the per-cell limit leaves (they read as a saw edge
  // where the insert meets a sloped face), then re-apply the limit.
  for (let pass = 0; pass < 4; pass++) {
    const prev = zs.slice();
    for (let j = 1; j < NY; j++) for (let i = 1; i < NX; i++) {
      const k = at(i, j);
      if (!hole[k]) continue;
      let sum = 0, n = 0;
      for (let b = j - 1; b <= j + 1; b++) for (let a = i - 1; a <= i + 1; a++) { const q = at(a, b); if (fixed[q] || hole[q]) { sum += prev[q]; n++; } }
      zs[k] = Math.min(sum / n, limit[k] + 0.3);
    }
  }
  // Cells that are neither rim nor opening take the nearest one that is, so the
  // insert's margin follows the rim (and hides behind it) instead of a default.
  const cellZ = (i, j) => {
    i = Math.min(NX, Math.max(0, i)); j = Math.min(NY, Math.max(0, j));
    for (let r = 0; r <= 4; r++) for (let b = j - r; b <= j + r; b++) for (let a = i - r; a <= i + r; a++) {
      if (a < 0 || b < 0 || a > NX || b > NY || (Math.abs(a - i) !== r && Math.abs(b - j) !== r)) continue;
      const k = at(a, b);
      if (fixed[k] || hole[k]) return zs[k];
    }
    return null;
  };
  // Bilinear over the valid corners.
  const depth = (x, y) => {
    const u = x - GX0, v = y - GY0, i = Math.floor(u), j = Math.floor(v), fu = u - i, fv = v - j;
    let sum = 0, w = 0;
    for (const [a, b, wt] of [[i, j, (1 - fu) * (1 - fv)], [i + 1, j, fu * (1 - fv)], [i, j + 1, (1 - fu) * fv], [i + 1, j + 1, fu * fv]]) {
      const z = cellZ(a, b);
      if (z !== null) { sum += z * wt; w += wt; }
    }
    return w ? sum / w : -20;
  };
  // Mesh on a 0.5 mm grid, 2 mm beyond the outline so its edge tucks under
  // the rims, recessed 0.4 so they frame it.
  const MARGIN = 2, STEP = 0.5, MX = Math.round(NX / STEP), MY = Math.round(NY / STEP), verts = [], tris = [], ids = new Map();
  const vid = (i, j) => {
    const key = j * (MX + 1) + i;
    if (!ids.has(key)) {
      let x = GX0 + i * STEP, y = GY0 + j * STEP;
      // Pull the grid's outer corners onto the offset outline: a smooth edge, no steps.
      const n = nearest(x, y);
      if (n.d > MARGIN) { const f = MARGIN / n.d; x = n.px + (x - n.px) * f; y = n.py + (y - n.py) * f; }
      ids.set(key, verts.length / 3);
      verts.push(x, y, depth(x, y) + 0.4);
    }
    return ids.get(key);
  };
  for (let j = 0; j < MY; j++) for (let i = 0; i < MX; i++) {
    const x = GX0 + (i + 0.5) * STEP, y = GY0 + (j + 0.5) * STEP;
    if (edgeDist(x, y) > MARGIN) continue;
    const a = vid(i, j), b = vid(i + 1, j), c = vid(i, j + 1), d = vid(i + 1, j + 1);
    tris.push(a, c, b, b, c, d);
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.Float32BufferAttribute(verts, 3));
  geo.setIndex(tris);
  geo.computeVertexNormals();
  add(new THREE.Mesh(geo, Object.assign(insert.clone(), { side: THREE.DoubleSide })));
  // Drop the print's inner guides that would poke through the insert: shell
  // triangles well inside the outline and near its surface.
  {
    const g = shell.geometry, pos = g.attributes.position, idx = g.index.array, keep = [];
    for (let t = 0; t < idx.length; t += 3) {
      // Keep up-facing faces (the thumb rest's top) and the outward skin.
      const n = new THREE.Triangle(...[0, 1, 2].map((v) => new THREE.Vector3().fromBufferAttribute(pos, idx[t + v]))).getNormal(new THREE.Vector3());
      let drop = n.y < 0.4 && n.z > -0.7;
      for (let v = 0; v < 3 && drop; v++) {
        const q = idx[t + v], x = pos.getX(q), y = pos.getY(q), z = pos.getZ(q);
        if (edgeDist(x, y) > -1.2 || z < depth(x, y) - 6 || z > depth(x, y) + 9) drop = false;
      }
      if (!drop) keep.push(idx[t], idx[t + 1], idx[t + 2]);
    }
    g.setIndex(keep);
  }
  // Insert surface (outer side, without the recess) for parts that sit on it.
  const sideAtPanel = (y, x = 0) => depth(x, y);
  // Thumb wheel: two knurled steel halves and a chrome ring. Its axis runs along
  // the insert, front to back and rising toward the rear like the keys, so the
  // thumb rolls it up and down; it sinks
  // into the insert so about half its radius shows, as on the real mouse.
  const dzdx = (sideAtPanel(THUMB.y, THUMB.x + 1) - sideAtPanel(THUMB.y, THUMB.x - 1)) / 2;
  const dzdy = (sideAtPanel(THUMB.y + 1, THUMB.x) - sideAtPanel(THUMB.y - 1, THUMB.x)) / 2;
  const out = new THREE.Vector3(dzdx, dzdy, -1).normalize(); // outward normal of the insert
  const along = new THREE.Vector3(Math.cos(KEYS[0].tilt), -Math.sin(KEYS[0].tilt), 0);
  const axis = along.addScaledVector(out, -along.dot(out)).normalize();
  const thumbZ = sideAtPanel(THUMB.y, THUMB.x) + THUMB.r * 0.4;
  const thumbMat = highlight === "thumb" ? lit(steel) : steel;
  const chrome = new THREE.MeshStandardMaterial({ color: 0xe2e6ea, metalness: 1, roughness: 0.12 });
  const thumb = new THREE.Group();
  for (const sy of [-1, 1]) {
    const half = new THREE.Mesh(knurl(THUMB.r, THUMB.len / 2 - 0.6, 64, 0.35), thumbMat);
    half.position.y = sy * (THUMB.len / 4 + 0.3);
    half.castShadow = true;
    thumb.add(half);
  }
  thumb.add(new THREE.Mesh(new THREE.CylinderGeometry(THUMB.r - 0.4, THUMB.r - 0.4, 1.2, 48), chrome));
  thumb.position.set(THUMB.x, THUMB.y, sideAtPanel(THUMB.y, THUMB.x)).addScaledVector(out, -THUMB.r * 0.5);
  thumb.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), axis);
  root.add(thumb);
  // Status light behind the thumb wheel.
  const ledAt = [-11.5, 41.5];
  const led = add(new THREE.Mesh(new THREE.CapsuleGeometry(0.55, 3.2, 4, 10), new THREE.MeshBasicMaterial({ color: 0x5cf07a })));
  led.rotation.z = Math.PI / 2 - 0.25;
  led.position.set(ledAt[0], ledAt[1], sideAtPanel(ledAt[1], ledAt[0]) - 0.1);
  // Back and forward keys under the thumb wheel.
  const keyMat = highlight === "buttons" ? lit(insert) : insert;
  for (const k of KEYS) {
    const key = add(new THREE.Mesh(new THREE.CapsuleGeometry(2, k.len, 4, 12), keyMat));
    key.rotation.z = Math.PI / 2 - k.tilt;
    // Flush with the lip around the slot: its most outward point nearby.
    let lip = Infinity;
    for (let dx = -5; dx <= 5; dx++) for (let dy = -4; dy <= 4; dy++) { const z = hit([k.x + dx, k.y + dy, -200], [0, 0, 1])?.z; if (z !== undefined && z > -40) lip = Math.min(lip, z); }
    key.position.set(k.x, k.y, lip + 0.9);
  }
  // Sole under the open print.
  const foot = [];
  for (let a = 0; a < 64; a++) { const t = (a / 64) * Math.PI * 2; foot.push(new THREE.Vector2(-2 + 52 * Math.cos(t), -10 + 32 * Math.sin(t))); }
  const sole = add(new THREE.Mesh(new THREE.ShapeGeometry(new THREE.Shape(foot)), dark));
  sole.rotation.x = Math.PI / 2;
  sole.position.y = 3.6;

  Object.assign(STEP_ANCHORS, {
    // Hotspots sit just off their parts, outward, so they never cover them.
    pointer: { at: [47, topAt(47, 16) + 2, 16], normal: [0, 1, 0] },
    wheel: { at: [WHEEL.x - 4, wheelTop + WHEEL.proud + 15, WHEEL.z], normal: [0, 1, 0] },
    thumb: { at: [THUMB.x + 12, THUMB.y + 14, thumbZ - THUMB.r], normal: [0, 0, -1] },
    buttons: { at: [(KEYS[0].x + KEYS[1].x) / 2, (KEYS[0].y + KEYS[1].y) / 2 - 8, -33], normal: [0, 0, -1] },
  });
}
