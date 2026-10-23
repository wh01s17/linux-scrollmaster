import assert from 'node:assert/strict';
import test from 'node:test';
import { makeShortcut, readShortcut, shortcutLabel } from '../src/shortcuts.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts
const catalogue = { keys: [{ value: 'KeyS', label: 'S' }, { value: 'F12', label: 'F12' }],
  modifiers: [{ value: 'ctrl', label: 'Ctrl' }, { value: 'alt', label: 'Alt' }, { value: 'shift', label: 'Shift' }, { value: 'super', label: 'Super' }] };

test('shortcut selection follows the advertised catalogue and canonical order', () => {
  assert.equal(makeShortcut('KeyS', ['shift', 'ctrl'], catalogue), 'custom:ctrl+shift:KeyS');
  assert.equal(makeShortcut('Power', ['ctrl'], catalogue), null);
  assert.equal(makeShortcut('KeyS', ['exec'], catalogue), null);
  assert.equal(makeShortcut('F12', [], catalogue), 'custom::F12');
});
test('shortcut parser rejects malformed values and noncanonical combinations', () => {
  for (const value of [null, 5, {}, 'custom:shift+ctrl:KeyS', 'custom:ctrl+ctrl:KeyS', 'custom:exec:KeyS', 'custom:ctrl:Power', 'custom:ctrl:KeyS:extra']) {
    assert.equal(readShortcut(value, catalogue), null);
  }
  assert.deepEqual(readShortcut('custom:ctrl+shift:KeyS', catalogue), { key: 'KeyS', modifiers: ['ctrl', 'shift'] });
});
test('shortcut preview uses readable labels for every modifier and unmodified keys', () => {
  assert.equal(shortcutLabel('custom:ctrl+alt+shift+super:KeyS', catalogue), 'Ctrl + Alt + Shift + Super + S');
  assert.equal(shortcutLabel('custom::F12', catalogue), 'F12');
  assert.equal(shortcutLabel('shortcut:copy', catalogue), 'Atajo sin configurar');
});
test('editing a shortcut does not mutate the catalogue or a previously saved action', () => {
  const before = structuredClone(catalogue);
  const original = 'custom:ctrl:KeyS';
  const parsed = readShortcut(original, catalogue)!;
  parsed.modifiers.push('shift');
  assert.equal(makeShortcut(parsed.key, parsed.modifiers, catalogue), 'custom:ctrl+shift:KeyS');
  assert.equal(original, 'custom:ctrl:KeyS');
  assert.deepEqual(catalogue, before);
});
