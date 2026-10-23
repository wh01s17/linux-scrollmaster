// SPDX-License-Identifier: GPL-3.0-or-later
// Scene from the tessellated STEP shell and buttons (art/mouse-render/cache),
// with our own materials. STEP frame: x forward, y up, thumb rest toward -z.
// Textures and marks of the source are never used.

export async function loadParts(base = "./cache/") {
  const [meta, bin] = await Promise.all([fetch(base + "parts.json").then((r) => r.json()), fetch(base + "parts.bin").then((r) => r.arrayBuffer())]);
  return meta.map((p) => ({
    ...p,
    position: new Float32Array(bin, p.position.offset, p.position.length),
    normal: new Float32Array(bin, p.normal.offset, p.normal.length),
    index: new Uint32Array(bin, p.index.offset, p.index.length),
  }));
}

export function buildStepMouse(THREE, parts, { highlight = null, accent = 0x66c2a5, extras = null } = {}) {
  const root = new THREE.Group();
  const graphite = new THREE.MeshPhysicalMaterial({ color: 0x2c2f33, roughness: 0.6, metalness: 0.12, clearcoat: 0.12, clearcoatRoughness: 0.7, envMapIntensity: 0.45 });
  const buttons = new THREE.MeshPhysicalMaterial({ color: 0x232529, roughness: 0.5, metalness: 0.1, clearcoat: 0.25, clearcoatRoughness: 0.5, envMapIntensity: 0.45 });
  const lit = (material) => Object.assign(material.clone(), { color: new THREE.Color(accent), emissive: new THREE.Color(accent).multiplyScalar(0.25) });
  for (const p of parts) {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(p.position, 3));
    g.setAttribute("normal", new THREE.BufferAttribute(p.normal, 3));
    g.setIndex(new THREE.BufferAttribute(p.index, 1));
    const isButton = p.group === "buttons";
    const material = isButton ? (highlight === "pointer" ? lit(buttons) : buttons) : graphite;
    const mesh = new THREE.Mesh(g, material);
    mesh.name = p.name;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    root.add(mesh);
  }
  if (extras) extras(THREE, root, { highlight, lit });
  return root;
}
