import assert from 'node:assert/strict';
import test from 'node:test';
import { changeDistanceUnits, distanceLabel, physicalDistance, validDistance } from '../src/gestureDistance.ts';
import type { GestureConfig } from '../src/deviceState.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts
const original = (): GestureConfig => ({ type: 'gestures', threshold: 200, actions: { up: 'media:volume-up', down: null, left: null, right: null, click: null } });

test('legacy settings retain raw sensitivity until explicit conversion', () => {
  const config = original();
  assert.equal(physicalDistance(config), false);
  assert.equal(distanceLabel(config), '200');
  assert.deepEqual(changeDistanceUnits(config, false, 1850), config);
  assert.equal(config.threshold, 200);
});
test('physical conversion retains approximately the same reach at the observed DPI', () => {
  const config = original();
  const converted = changeDistanceUnits(config, true, 1850);
  assert.equal(converted.units, 'tenths-mm');
  assert.equal(converted.threshold, 27);
  assert.equal(distanceLabel(converted), '2,7 mm');
  assert.deepEqual(config, original());
  converted.actions.up = null;
  assert.equal(config.actions.up, 'media:volume-up');
});
test('returning to legacy removes the new field and respects its bounds', () => {
  const physical = { ...original(), units: 'tenths-mm' as const, threshold: 30 };
  const legacy = changeDistanceUnits(physical, false, 1850);
  assert.equal(legacy.threshold, 219);
  assert.equal('units' in legacy, false);
  assert.equal(changeDistanceUnits(physical, false, 200).threshold, 24);
});
test('distance validation uses the range of the selected units', () => {
  for (const threshold of [10, 30, 200]) assert.equal(validDistance({ ...original(), units: 'tenths-mm', threshold }), true);
  for (const threshold of [9, 201, 30.5, NaN]) assert.equal(validDistance({ ...original(), units: 'tenths-mm', threshold }), false);
  assert.equal(validDistance({ ...original(), threshold: 2000 }), true);
  assert.equal(validDistance({ ...original(), threshold: 19 }), false);
});
test('conversion rejects unknown DPI and clamps estimates to editable distances', () => {
  for (const dpi of [0, -1, NaN, Infinity, 1850.5, 65536]) assert.throws(() => changeDistanceUnits(original(), true, dpi));
  assert.equal(changeDistanceUnits({ ...original(), threshold: 2000 }, true, 200).threshold, 200);
  assert.equal(changeDistanceUnits({ ...original(), threshold: 20 }, true, 8000).threshold, 10);
});
