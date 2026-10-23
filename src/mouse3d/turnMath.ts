// SPDX-License-Identifier: GPL-3.0-or-later
// Pure helpers for scrubbing a turntable image sequence; no DOM access.
import type { Anchor } from './turntable.ts';

/** Unwrapped target frame reached from `current` the short way round. */
export function shortestTarget(current: number, goal: number, frames: number): number {
  const delta = ((((goal - current) % frames) + frames * 1.5) % frames) - frames / 2;
  return current + delta;
}
/** Wrapped frame index in 0..frames-1 for an unwrapped position. */
export function frameIndex(position: number, frames: number): number {
  return ((Math.round(position) % frames) + frames) % frames;
}
/** Anchor between the two nearest rendered frames; hidden when facing away. */
export function anchorAt(anchors: Record<string, Anchor>[], position: number, zone: string): Anchor & { visible: boolean } {
  const n = anchors.length;
  const base = Math.floor(position);
  const t = position - base;
  const a = anchors[((base % n) + n) % n][zone];
  const b = anchors[(((base + 1) % n) + n) % n][zone];
  const x = a.x + (b.x - a.x) * t;
  const y = a.y + (b.y - a.y) * t;
  const facing = a.facing + (b.facing - a.facing) * t;
  return { x, y, facing, visible: facing > 0.12 };
}
