// Compile the real SFCs and exercise their events in Vue's in-memory renderer.
// No browser, Tauri, display server or hardware access.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';
import { createRenderer, h, nextTick, shallowRef } from 'vue';
import { parse, compileScript } from 'vue/compiler-sfc';
import ts from 'typescript';
import { createDeviceState } from '../src/deviceState.ts';

async function component(path) {
  const { descriptor } = parse(await readFile(path, 'utf8'), { filename: path });
  const script = compileScript(descriptor, { id: path, inlineTemplate: true });
  let code = ts.transpileModule(script.content, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  for (const match of [...code.matchAll(/from\s*['"]([^'"]+)['"]/g)]) {
    const name = match[1];
    const url = name.endsWith('.vue') ? await component(resolve(dirname(path), name))
      : name.startsWith('.') ? pathToFileURL(resolve(dirname(path), name.endsWith('.ts') ? name : `${name}.ts`)).href : import.meta.resolve(name);
    code = code.replace(match[0], `from ${JSON.stringify(url)}`);
  }
  return `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
}
const root = fileURLToPath(new URL('..', import.meta.url));
// Components share this module instance (same file URL); assertions use the Spanish source texts.
(await import(pathToFileURL(resolve(root, 'src/i18n.ts')).href)).setLocale('es');
const { default: ButtonAssignment } = await import(await component(resolve(root, 'src/components/ButtonAssignment.vue')));
const { default: AutostartPreference } = await import(await component(resolve(root, 'src/components/AutostartPreference.vue')));
const { default: GestureEditor } = await import(await component(resolve(root, 'src/components/GestureEditor.vue')));
const { default: ProfileManager } = await import(await component(resolve(root, 'src/components/ProfileManager.vue')));
const node = (type, text = '') => ({ type, text, props: {}, children: [], parent: null,
  events: {}, addEventListener(name, handler) { (this.events[name] ??= []).push(handler); },
  get options() { return this.children.filter(child => child.type === 'option'); },
});
const renderer = createRenderer({
  createElement: node, createText: text => node('#text', text), createComment: text => node('#comment', text),
  setText: (el, text) => { el.text = text; }, setElementText: (el, text) => { el.text = text; el.children = []; },
  patchProp: (el, key, _old, value) => { el.props[key] = value; if (key === 'value') { el.value = value; el._value = value; } },
  parentNode: el => el.parent, nextSibling: el => el.parent?.children[el.parent.children.indexOf(el) + 1] || null,
  insert(el, parent, anchor) {
    if (el.parent) el.parent.children.splice(el.parent.children.indexOf(el), 1);
    const index = anchor ? parent.children.indexOf(anchor) : parent.children.length;
    parent.children.splice(index, 0, el); el.parent = parent;
  },
  remove(el) { el.parent?.children.splice(el.parent.children.indexOf(el), 1); el.parent = null; },
});
function findAll(tree, type) { return [...(tree.type === type ? [tree] : []), ...tree.children.flatMap(child => findAll(child, type))]; }
const catalogue = { keys: [{ value: 'KeyS', label: 'S' }, { value: 'F12', label: 'F12' }],
  modifiers: [{ value: 'ctrl', label: 'Ctrl' }, { value: 'shift', label: 'Shift' }] };
function fixture(t, value = 83) {
  const container = node('root'), applied = [], busy = shallowRef(false);
  const control = shallowRef({ id: '83', label: 'Atrás', value, available: true, shortcuts: catalogue,
    choices: [{ value: 83, label: 'Atrás' }, { value: 86, label: 'Adelante' }] });
  const app = renderer.createApp({ render: () => h(ButtonAssignment, { control: control.value, disabled: busy.value, onApply: action => applied.push(action) }) });
  app.mount(container); t.after(() => app.unmount());
  return { container, applied, busy, control };
}
async function change(el, value, checked) { el.props.onChange({ target: { value, checked } }); await nextTick(); }

test('custom shortcut edits emit valid assignments without an Apply button', async t => {
  const f = fixture(t);
  await change(findAll(f.container, 'select')[0], 'custom');
  assert.deepEqual(f.applied, ['custom:ctrl:KeyS']);
  await change(findAll(f.container, 'input')[1], undefined, true);
  f.control.value = { ...f.control.value }; await nextTick();
  assert.equal(findAll(f.container, 'input')[1].props.checked, true);
  assert.equal(findAll(f.container, 'button').length, 0);
  assert.deepEqual(f.applied, ['custom:ctrl:KeyS', 'custom:ctrl+shift:KeyS']);
  // Returning to the original value must cancel a shortcut still being edited.
  await change(findAll(f.container, 'select')[0], '83');
  assert.deepEqual(f.applied, ['custom:ctrl:KeyS', 'custom:ctrl+shift:KeyS', 83]);
});
test('confirmed readback and external profile updates replace the shortcut draft', async t => {
  const f = fixture(t, 'custom:ctrl:KeyS');
  assert.equal(findAll(f.container, 'button').length, 0);
  await change(findAll(f.container, 'select')[1], 'F12');
  assert.equal(findAll(f.container, 'button').length, 0);
  f.control.value = { ...f.control.value, value: 'custom:shift:F12' }; await nextTick();
  assert.equal(findAll(f.container, 'input')[0].props.checked, false);
  assert.equal(findAll(f.container, 'input')[1].props.checked, true);
  assert.equal(findAll(f.container, 'button').length, 0);
  assert.deepEqual(f.applied, ['custom:ctrl:F12']);
});
test('busy or unavailable hardware disables shortcut fields', async t => {
  const f = fixture(t);
  await change(findAll(f.container, 'select')[0], 'custom');
  f.busy.value = true; await nextTick();
  assert.equal(findAll(f.container, 'fieldset')[0].props.disabled, true);
  assert.equal(findAll(f.container, 'button').length, 0);
  f.busy.value = false; f.control.value = { ...f.control.value, available: false }; await nextTick();
  assert.equal(findAll(f.container, 'fieldset')[0].props.disabled, true);
  assert.equal(findAll(f.container, 'button').length, 0);
});
test('native selections retain immediate application and reset to confirmed state after a failed change', async t => {
  const f = fixture(t);
  await change(findAll(f.container, 'select')[0], '86');
  assert.deepEqual(f.applied, [86]);
  f.busy.value = true; await nextTick(); f.busy.value = false; await nextTick();
  assert.equal(findAll(f.container, 'select')[0].props.value, 83);
});

test('desktop options stay disabled without IPC and cannot emit an assignment', async t => {
  const f = fixture(t);
  f.control.value = { ...f.control.value, desktop_error: 'IPC ausente', choices: [...f.control.value.choices,
    { value: 'desktop:workspace-next', label: 'Espacio siguiente · Hyprland', disabled: true }] };
  await nextTick();
  const option = findAll(f.container, 'option').find(o => o.props.value === 'desktop:workspace-next');
  assert.equal(option.props.disabled, true);
  await change(findAll(f.container, 'select')[0], 'desktop:workspace-next');
  assert.deepEqual(f.applied, []);
  assert.equal(findAll(f.container, 'select')[0].props.value, 83);
});
test('loading a desktop binding never executes it and selecting an enabled action assigns once', async t => {
  const f = fixture(t, 'desktop:workspace-next');
  f.control.value = { ...f.control.value, choices: [...f.control.value.choices,
    { value: 'desktop:workspace-next', label: 'Espacio siguiente · Hyprland' },
    { value: 'desktop:maximize', label: 'Alternar maximizado · Hyprland' }] };
  await nextTick();
  assert.deepEqual(f.applied, []);
  await change(findAll(f.container, 'select')[0], 'desktop:maximize');
  assert.deepEqual(f.applied, ['desktop:maximize']);
});

function profileFixture(t, Component, connected = true) {
  const container = node('root'), sent = [];
  const data = { devices: connected ? [{ id: 'mouse', name: 'MX', settings: { dpi: { available: true, value: 1800 } },
    buttons: { controls: [{ id: '195', available: true, value: 195, choices: [{ value: 195, label: 'Original' }],
      gestures_available: true, gesture_actions: [{ value: 'shortcut:copy', label: 'Copiar' }] }] } }] : [],
    profiles: [{ device: 'mouse', name: '__current__' }], active: { mouse: '__current__' }, warnings: [], diagnostic: null };
  const state = createDeviceState(async payload => {
    sent.push(payload);
    return { ok: true, revision: sent.length + 1, data: { ...data } };
  });
  state.receive({ ok: true, revision: 1, data });
  const app = renderer.createApp({ render: () => h(Component, { state }) });
  app.mount(container); t.after(() => { app.unmount(); state.flushRemaps(); });
  return { container, sent, state, app };
}

test('gesture changes save after leaving the editor and clearing all actions restores the button', async t => {
  const f = profileFixture(t, GestureEditor);
  assert.equal(f.sent.length, 0); // mounting and polling cannot enable capture
  await change(findAll(f.container, 'select')[0], 'shortcut:copy');
  assert.equal(f.state.saving.value, true);
  await change(findAll(f.container, 'select')[0], '');
  f.app.unmount();
  await new Promise(resolve => setTimeout(resolve, 450));
  assert.deepEqual(f.sent, [{ op: 'remap', device: 'mouse', changes: { '195': 195 }, autosave: true }]);
});

test('Save as copies the current profile while disconnected without reading hardware', async t => {
  const f = profileFixture(t, ProfileManager, false);
  const button = findAll(f.container, 'button').find(b => b.text === 'Guardar como…');
  assert.equal(button.props.disabled, false);
  const option = findAll(f.container, 'option').find(o => o.props.value === '__current__');
  assert.equal(option.text, 'Perfil actual');
  assert.equal(findAll(f.container, 'input').filter(input => input.props.type === 'checkbox').length, 0);
  await button.props.onClick();
  assert.deepEqual(f.sent, [{ op: 'save_copy', device: 'mouse', name: 'Mi perfil' }]);
});

test('autostart checkbox emits intent but remains at confirmed state until readback', async t => {
  const container = node('root'), changes = [];
  const value = shallowRef({ enabled: false, can_change: true, needs_update: false, error: null });
  const app = renderer.createApp({ render: () => h(AutostartPreference, { value: value.value, ready: true, busy: false, onChange: enabled => changes.push(enabled) }) });
  app.mount(container); t.after(() => app.unmount());
  assert.deepEqual(changes, []);
  const input = findAll(container, 'input')[0]; const target = { checked: true };
  input.props.onChange({ target }); await nextTick();
  assert.deepEqual(changes, [true]); assert.equal(target.checked, false);
  assert.equal(input.props.checked, false);
  value.value = { ...value.value, enabled: true }; await nextTick();
  assert.equal(input.props.checked, true);
});
test('autostart relocation stays explicit and controls lock on unverified or foreign state', async t => {
  const container = node('root'), changes = [];
  const value = shallowRef({ enabled: true, can_change: true, needs_update: true, error: null });
  const ready = shallowRef(false), busy = shallowRef(false);
  const app = renderer.createApp({ render: () => h(AutostartPreference, { value: value.value, ready: ready.value, busy: busy.value, onChange: enabled => changes.push(enabled) }) });
  app.mount(container); t.after(() => app.unmount());
  assert.equal(findAll(container, 'input')[0].props.disabled, true); assert.deepEqual(changes, []);
  ready.value = true; await nextTick();
  findAll(container, 'button')[0].props.onClick(); assert.deepEqual(changes, [true]);
  busy.value = true; await nextTick(); assert.equal(findAll(container, 'button')[0].props.disabled, true);
  busy.value = false; value.value = { ...value.value, can_change: false }; await nextTick();
  assert.equal(findAll(container, 'input')[0].props.disabled, true);
});
