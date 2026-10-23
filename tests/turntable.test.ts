// SPDX-License-Identifier: GPL-3.0-or-later
import assert from 'node:assert/strict';
import test from 'node:test';
import { anchorAt, frameIndex, shortestTarget } from '../src/mouse3d/turnMath.ts';

test('the turntable takes the short way round in both directions', () => {
  assert.equal(shortestTarget(7, 66, 72), -6);   // 7 → 66 backwards across 0
  assert.equal(shortestTarget(66, 7, 72), 79);   // 66 → 7 forwards across 0
  assert.equal(shortestTarget(19, 27, 72), 27);
  assert.equal(shortestTarget(150, 7, 72), 151); // unwrapped positions stay continuous
  assert.equal(frameIndex(-6, 72), 66);
  assert.equal(frameIndex(79.4, 72), 7);
});
test('anchors interpolate between frames and hide when the part faces away', () => {
  const anchors = [{ a: { x: 0.2, y: 0.4, facing: 0.5 } }, { a: { x: 0.4, y: 0.6, facing: -0.5 } }];
  const mid = anchorAt(anchors, 0.5, 'a');
  assert.ok(Math.abs(mid.x - 0.3) < 1e-9 && Math.abs(mid.y - 0.5) < 1e-9);
  assert.equal(mid.visible, false);
  assert.equal(anchorAt(anchors, 2, 'a').visible, true); // wraps to frame 0
});
