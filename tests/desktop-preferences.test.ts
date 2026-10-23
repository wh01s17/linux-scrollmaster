import assert from 'node:assert/strict';
import test from 'node:test';
import { createDesktopPreferences } from '../src/desktopPreferences.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts
const tray = { tray: false, error: null };
const off = { enabled: false, can_change: true, needs_update: false, error: null };

test('browser preview never reads or writes native preferences', async () => {
  const calls: string[] = [];
  const state = createDesktopPreferences(async command => { calls.push(command); }, false);
  await state.load(); await state.setTray(true); await state.setAutostart(true);
  assert.deepEqual(calls, []); assert.equal(state.autostartReady.value, false);
});
test('initial reads keep tray and autostart independent and never enable them', async () => {
  const calls: string[] = [];
  const state = createDesktopPreferences(async command => { calls.push(command); return command === 'desktop_preferences' ? tray : off; }, true);
  await state.load();
  assert.deepEqual(calls.sort(), ['autostart_preferences', 'desktop_preferences']);
  assert.equal(state.tray.value.tray, false); assert.equal(state.autostart.value.enabled, false);
  assert.equal(state.autostartReady.value, true);
});
test('enabling waits for confirmation and blocks concurrent preference writes', async () => {
  const calls: string[] = []; let confirm!: (value: unknown) => void;
  const state = createDesktopPreferences(async command => {
    calls.push(command);
    if (command === 'set_autostart') return await new Promise(resolve => { confirm = resolve; });
    return command === 'desktop_preferences' ? tray : off;
  }, true);
  await state.load(); const pending = state.setAutostart(true);
  assert.equal(state.busy.value, true); assert.equal(state.autostart.value.enabled, false);
  await state.setTray(true); await state.setAutostart(false);
  confirm({ ...off, enabled: true }); await pending;
  assert.equal(state.autostart.value.enabled, true); assert.equal(state.tray.value.tray, false);
  assert.equal(calls.filter(c => c.startsWith('set_')).length, 1);
});
test('failure after publish re-reads the actual entry without reporting false off', async () => {
  let stored = false;
  const state = createDesktopPreferences(async command => {
    if (command === 'set_autostart') { stored = true; throw new Error('No se confirmó fsync'); }
    return command === 'desktop_preferences' ? tray : { ...off, enabled: stored };
  }, true);
  await state.load(); await state.setAutostart(true);
  assert.equal(state.autostart.value.enabled, true); assert.match(state.error.value, /fsync/);
  assert.equal(state.busy.value, false);
});
test('foreign entry disables changes while tray remains usable', async () => {
  const calls: string[] = [];
  const state = createDesktopPreferences(async command => {
    calls.push(command);
    return command === 'desktop_preferences' ? tray : command === 'set_tray' ? { tray: true, error: null } : { ...off, can_change: false, error: 'Entrada ajena' };
  }, true);
  await state.load(); await state.setAutostart(true); await state.setTray(true);
  assert.equal(calls.includes('set_autostart'), false); assert.equal(state.tray.value.tray, true);
  assert.match(state.error.value, /ajena/);
});
test('failed readback invalidates controls until an explicit refresh succeeds', async () => {
  let failing = false;
  const state = createDesktopPreferences(async command => {
    if (command === 'set_autostart' || (command === 'autostart_preferences' && failing)) throw new Error('Disco no disponible');
    return command === 'desktop_preferences' ? tray : off;
  }, true);
  await state.load(); failing = true; await state.setAutostart(true);
  assert.equal(state.autostartReady.value, false);
  failing = false; await state.load(); assert.equal(state.autostartReady.value, true); assert.equal(state.error.value, '');
});
test('relocation is only updated through explicit enable and malformed replies are rejected', async () => {
  const calls: { command: string; args: unknown }[] = [];
  const state = createDesktopPreferences(async (command, args) => {
    calls.push({ command, args });
    return command === 'desktop_preferences' ? tray : command === 'set_autostart' ? { ...off, enabled: true } : { ...off, enabled: true, needs_update: true };
  }, true);
  await state.load(); assert.equal(state.autostart.value.needs_update, true);
  assert.equal(calls.length, 2); await state.setAutostart(true);
  assert.deepEqual(calls.at(-1), { command: 'set_autostart', args: { enabled: true } });
  assert.equal(state.autostart.value.needs_update, false);
  const bad = createDesktopPreferences(async () => ({}), true); await bad.load();
  assert.equal(bad.trayReady.value, false); assert.equal(bad.autostartReady.value, false);
});
