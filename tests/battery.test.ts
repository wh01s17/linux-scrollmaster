// SPDX-License-Identifier: GPL-3.0-or-later
import assert from 'node:assert/strict';
import test from 'node:test';
import { batteryView } from '../src/battery.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts

test('charging, full, low and unknown batteries are distinguishable', () => {
  assert.deepEqual(batteryView(25, 'charging'), { text: '25%', label: 'Batería 25%, cargando', fill: 25, tone: 'charging', charging: true });
  assert.equal(batteryView(100, 'full').label, 'Batería 100%, carga completa');
  assert.equal(batteryView(15, 'discharging').tone, 'low');
  assert.equal(batteryView(20, null).tone, 'low');
  assert.equal(batteryView(21, undefined).tone, 'ok');
  assert.deepEqual(batteryView(null, 'charging'), { text: '—', label: 'Batería desconocida', fill: 0, tone: 'unknown', charging: true });
});
test('out-of-range readings are clamped, never drawn past the icon', () => {
  assert.equal(batteryView(140, null).fill, 100);
  assert.equal(batteryView(-3, null).text, '0%');
  assert.equal(batteryView(Number.NaN, null).text, '—');
});
