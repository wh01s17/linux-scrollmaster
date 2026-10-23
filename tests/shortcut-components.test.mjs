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

async function component(path) {
  const { descriptor } = parse(await readFile(path, 'utf8'), { filename: path });
  const script = compileScript(descriptor, { id: path, inlineTemplate: true });
  let code = ts.transpileModule(script.content, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  for (const match of [...code.matchAll(/from\s*['"]([^'"]+)['"]/g)]) {
    const name = match[1];
    const url = name.endsWith('.vue') ? await component(resolve(dirname(path), name))
      : name.startsWith('.') ? pathToFileURL(resolve(dirname(path), `${name}.ts`)).href : import.meta.resolve(name);
    code = code.replace(match[0], `from ${JSON.stringify(url)}`);
  }
  return `data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
}
const root = fileURLToPath(new URL('..', import.meta.url));
// Components share this module instance (same file URL); assertions use the Spanish source texts.
(await import(pathToFileURL(resolve(root, 'src/i18n.ts')).href)).setLocale('es');
const { default: ButtonAssignment } = await import(await component(resolve(root, 'src/components/ButtonAssignment.vue')));
const { default: AutostartPreference } = await import(await component(resolve(root, 'src/components/AutostartPreference.vue')));
const node = (type, text = '') => ({ type, text, props: {}, children: [], parent: null });
const renderer = createRenderer({
  createElement: node, createText: text => node('#text', text), createComment: text => node('#comment', text),
  setText: (el, text) => { el.text = text; }, setElementText: (el, text) => { el.text = text; el.children = []; },
  patchProp: (el, key, _old, value) => { el.props[key] = value; },
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

test('custom shortcut edits remain local until Apply, including repeated hardware polls', async t => {
  const f = fixture(t);
  await change(findAll(f.container, 'select')[0], 'custom');
  assert.deepEqual(f.applied, []);
  await change(findAll(f.container, 'input')[1], undefined, true);
  f.control.value = { ...f.control.value }; await nextTick();
  assert.equal(findAll(f.container, 'input')[1].props.checked, true);
  assert.deepEqual(f.applied, []);
  findAll(f.container, 'button')[0].props.onClick();
  assert.deepEqual(f.applied, ['custom:ctrl+shift:KeyS']);
});
test('confirmed readback and external profile updates replace the shortcut draft', async t => {
  const f = fixture(t, 'custom:ctrl:KeyS');
  assert.equal(findAll(f.container, 'button')[0].props.disabled, true);
  await change(findAll(f.container, 'select')[1], 'F12');
  assert.equal(findAll(f.container, 'button')[0].props.disabled, false);
  f.control.value = { ...f.control.value, value: 'custom:shift:F12' }; await nextTick();
  assert.equal(findAll(f.container, 'input')[0].props.checked, false);
  assert.equal(findAll(f.container, 'input')[1].props.checked, true);
  assert.equal(findAll(f.container, 'button')[0].props.disabled, true);
  assert.deepEqual(f.applied, []);
});
test('busy or unavailable hardware disables both shortcut fields and Apply', async t => {
  const f = fixture(t);
  await change(findAll(f.container, 'select')[0], 'custom');
  f.busy.value = true; await nextTick();
  assert.equal(findAll(f.container, 'fieldset')[0].props.disabled, true);
  assert.equal(findAll(f.container, 'button')[0].props.disabled, true);
  f.busy.value = false; f.control.value = { ...f.control.value, available: false }; await nextTick();
  assert.equal(findAll(f.container, 'fieldset')[0].props.disabled, true);
  assert.equal(findAll(f.container, 'button')[0].props.disabled, true);
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
