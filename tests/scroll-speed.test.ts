import assert from 'node:assert/strict';
import test from 'node:test';
import { createScrollSpeed } from '../src/scrollSpeed.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts
const unset = { available: true, factor: null, saved: null, min: 0.1, max: 5, error: null };

test('browser preview never reads or writes the compositor', async () => {
  const calls: string[] = [];
  const state = createScrollSpeed(async command => { calls.push(command); }, false);
  await state.load(); await state.set(1.5);
  assert.deepEqual(calls, []);
});

test('outside Hyprland the control stays unavailable and never writes', async () => {
  const calls: string[] = [];
  const state = createScrollSpeed(async command => { calls.push(command); return { ...unset, available: false, error: 'Missing Hyprland instance signature' }; }, true);
  await state.load(); await state.set(1.5);
  assert.deepEqual(calls, ['scroll_speed_preferences']);
  assert.equal(state.value.value.available, false);
});

test('a write shows the value Hyprland confirmed, and reset sends null', async () => {
  const calls: unknown[] = [];
  const state = createScrollSpeed(async (command, args) => {
    calls.push({ command, args });
    if (command === 'set_scroll_speed') return args?.factor === null ? unset : { ...unset, factor: 1.5, saved: 1.5 };
    return unset;
  }, true);
  await state.load(); await state.set(1.5);
  assert.equal(state.value.value.factor, 1.5);
  await state.set(null);
  assert.deepEqual(calls.at(-1), { command: 'set_scroll_speed', args: { factor: null } });
  assert.equal(state.value.value.factor, null);
});

test('out of range values are rejected locally without writing', async () => {
  const calls: string[] = [];
  const state = createScrollSpeed(async command => { calls.push(command); return unset; }, true);
  await state.load();
  for (const factor of [0, 0.05, 5.5, Number.NaN]) await state.set(factor);
  assert.deepEqual(calls, ['scroll_speed_preferences']);
  assert.match(state.error.value, /entre 0.1 y 5/);
});

test('a failed write keeps the error and rereads the actual value', async () => {
  const state = createScrollSpeed(async command => {
    if (command === 'set_scroll_speed') throw new Error('Hyprland no confirmó la velocidad de desplazamiento');
    return { ...unset, factor: 1.2, saved: 1.2 };
  }, true);
  await state.load(); await state.set(2);
  assert.equal(state.value.value.factor, 1.2);
  assert.match(state.error.value, /no confirmó/);
  assert.equal(state.busy.value, false);
});

test('malformed replies are rejected', async () => {
  const state = createScrollSpeed(async () => ({ available: 'yes' }), true);
  await state.load();
  assert.equal(state.value.value.available, false);
  assert.match(state.error.value, /No se pudo leer/);
});
