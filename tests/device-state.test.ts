import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createDeviceState, type Reply, type Snapshot } from '../src/deviceState.ts';
import { setLocale } from '../src/i18n.ts';
setLocale('es'); // these assertions check the Spanish source texts
const snapshot = (devices = true): Snapshot => ({ devices: devices ? [{id:'mouse',name:'MX',connection:'Bolt',protocol:4.5,battery:40,settings:{dpi:{value:1800,available:true}}}] : [], profiles:[{device:'mouse',name:'Work'}],active:{mouse:'Work'},warnings:[],diagnostic:null });
const reply = (revision: number, devices = true): Reply => ({ok:true,revision,data:snapshot(devices)});

test('native events recover from disconnect without changing selected identity', () => {
  const s=createDeviceState(async()=>reply(1));
  s.receive(reply(1)); assert.equal(s.mouse.value?.id,'mouse');
  s.receive(reply(2,false)); assert.equal(s.mouse.value,undefined); assert.equal(s.selected.value,'mouse');
  assert.equal(s.profiles.value[0].name,'Work');
  s.receive(reply(3)); assert.equal(s.mouse.value?.id,'mouse');
});
test('late replies cannot overwrite a newer hardware read', () => {
  const s=createDeviceState(async()=>reply(1));
  s.receive(reply(3,false));s.receive(reply(2));assert.equal(s.mouse.value,undefined);
});
test('native poll errors clear stale controls and subsequent events recover', () => {
  const s=createDeviceState(async()=>reply(1));s.receive(reply(1));
  s.receive({ok:false,revision:2,error:'Permisos'});assert.equal(s.mouse.value,undefined);assert.equal(s.error.value,'Permisos');
  s.receive(reply(3));assert.equal(s.error.value,'');assert.equal(s.mouse.value?.id,'mouse');
});
test('failed mutation never leaves a success message or stale controls', async () => {
  const s=createDeviceState(async()=>({ok:false,revision:2,error:'Desconectado'}));s.receive(reply(1));
  assert.equal(await s.set('dpi',1850),false);assert.equal(s.mouse.value,undefined);assert.equal(s.message.value,'');
  s.receive(reply(3));assert.equal(s.error.value,'Desconectado'); // operation error remains visible until retry
});
test('concurrent poll events cannot silently discard a user write', async () => {
  let finish!: (r:Reply)=>void;let payload:Record<string,unknown>|undefined;
  const s=createDeviceState(p=>{payload=p;return new Promise(resolve=>{finish=resolve;});});s.receive(reply(1));
  const writing=s.set('dpi',1850);s.receive(reply(2));assert.equal(s.busy.value,true);
  finish(reply(3));assert.equal(await writing,true);assert.deepEqual(payload,{op:'set',device:'mouse',changes:{dpi:1850},autosave:true});
});
test('a second mouse never silently replaces a disconnected selected mouse', () => {
  const s=createDeviceState(async()=>reply(1));s.receive(reply(1));
  const other=reply(2);other.data!.devices[0].id='other';s.receive(other);
  assert.equal(s.mouse.value,undefined);assert.equal(s.selected.value,'mouse');
});
test('saved profile remains selectable if app starts with the mouse disconnected', () => {
  const s=createDeviceState(async()=>reply(1));s.receive(reply(1,false));
  assert.equal(s.selected.value,'mouse');assert.equal(s.profiles.value[0].name,'Work');
});
test('a confirmed automatic write is distinguished from retained firmware values', () => {
  const s=createDeviceState(async()=>reply(1));const changed=reply(1);
  changed.data!.reconciliation=[{device:'mouse',profile:'Work',applied:['dpi']}];
  s.receive(changed);assert.match(s.message.value,/recuperado y confirmado/);
  const unchanged=reply(2);unchanged.data!.reconciliation=[{device:'mouse',profile:'Work',applied:[]}];
  s.receive(unchanged);assert.match(s.message.value,/recuperado y confirmado/);
});
test('a late failed operation cannot erase a newer successful hardware read', async () => {
  let finish!: (r:Reply)=>void;
  const s=createDeviceState(()=>new Promise(resolve=>{finish=resolve;}));s.receive(reply(1));
  const writing=s.set('dpi',1850);s.receive(reply(3));
  finish({ok:false,revision:2,error:'Error anterior'});
  assert.equal(await writing,false);assert.equal(s.mouse.value?.id,'mouse');assert.equal(s.error.value,'Error anterior');
});
test('a late restore success cannot announce an applied profile after disconnection', async () => {
  let finish!: (r:Reply)=>void;
  const s=createDeviceState(()=>new Promise(resolve=>{finish=resolve;}));s.receive(reply(1));
  const restoring=s.request('restore',{name:'Work'});
  s.receive(reply(3,false));finish(reply(2));
  assert.equal(await restoring,true);
  assert.equal(s.mouse.value,undefined);assert.equal(s.message.value,'');
  assert.equal(s.busy.value,false);
});
test('a late write success preserves a newer automatic recovery confirmation', async () => {
  let finish!: (r:Reply)=>void;
  const s=createDeviceState(()=>new Promise(resolve=>{finish=resolve;}));s.receive(reply(1));
  const writing=s.set('dpi',1850);
  const restored=reply(3);
  restored.data!.reconciliation=[{device:'mouse',profile:'Work',applied:['dpi']}];
  s.receive(restored);finish(reply(2));
  assert.equal(await writing,true);
  assert.match(s.message.value,/Perfil «Work» recuperado y confirmado/);
});
test('native remap sends a typed change and shows hardware confirmation', async () => {
  let payload:Record<string,unknown>|undefined;
  const updated=reply(2);
  updated.data!.devices[0].buttons={available:true,controls:[{id:'83',label:'Atrás',value:86,available:true,choices:[{value:86,label:'Adelante'}]}]};
  const s=createDeviceState(async p=>{payload=p;return updated;});s.receive(reply(1));
  assert.equal(await s.request('remap',{changes:{'83':86}}),true);
  assert.deepEqual(payload,{op:'remap',device:'mouse',changes:{'83':86},autosave:true});
  assert.equal(s.mouse.value?.buttons?.controls[0].value,86);
  assert.equal(s.message.value,'Asignación confirmada y guardada automáticamente');
});
test('failed native remap clears stale controls and cannot announce success', async () => {
  const s=createDeviceState(async()=>({ok:false,revision:2,error:'No se confirmó la asignación'}));
  s.receive(reply(1));
  assert.equal(await s.request('remap',{changes:{'83':86}}),false);
  assert.equal(s.mouse.value,undefined);assert.equal(s.message.value,'');
  assert.match(s.error.value,/No se confirmó/);
});
test('held modifiers remain typed actions rather than fabricated native IDs', async () => {
  let payload:Record<string,unknown>|undefined;
  const updated=reply(2);
  updated.data!.devices[0].buttons={available:true,controls:[{id:'83',label:'Atrás',value:'modifier:ctrl',available:true,choices:[{value:'modifier:ctrl',label:'Ctrl (mantener)'}]}]};
  const s=createDeviceState(async p=>{payload=p;return updated;});s.receive(reply(1));
  assert.equal(await s.request('remap',{changes:{'83':'modifier:ctrl'}}),true);
  assert.deepEqual(payload,{op:'remap',device:'mouse',changes:{'83':'modifier:ctrl'},autosave:true});
  assert.equal(s.mouse.value?.buttons?.controls[0].value,'modifier:ctrl');
});
test('action recovery warning clears a previously confirmed assignment message', async () => {
  const s=createDeviceState(async()=>reply(2));s.receive(reply(1));
  assert.equal(await s.request('remap',{changes:{'83':'shortcut:copy'}}),true);
  assert.equal(s.message.value,'Asignación confirmada y guardada automáticamente');
  const recovered=reply(3);recovered.data!.warnings=['El motor se detuvo; vuelve a asignar.'];
  s.receive(recovered);
  assert.equal(s.message.value,'');assert.equal(s.data.value.warnings[0],'El motor se detuvo; vuelve a asignar.');
});
test('local profile failure does not discard a valid hardware reading',async()=>{
  const s=createDeviceState(async()=>({ok:false,revision:2,error:'Nombre duplicado'}));s.receive(reply(1));
  assert.equal(await s.request('rename',{name:'Old',new_name:'Existing'}),false);
  assert.equal(s.mouse.value?.id,'mouse');assert.equal(s.error.value,'Nombre duplicado');
});
test('file transfer result survives a newer native poll without changing its state',async()=>{
  let finish!:(r:Reply)=>void;
  const s=createDeviceState(()=>new Promise(resolve=>{finish=resolve;}));s.receive(reply(1));
  let document:unknown;
  const exporting=s.request('export',{name:'Work'},snapshot=>{document=snapshot.transfer?.document;});
  s.receive(reply(3,false));const older=reply(2);older.data!.transfer={document:{version:4,profiles:[],active:{}}};
  finish(older);assert.equal(await exporting,true);
  assert.deepEqual(document,{version:4,profiles:[],active:{}});assert.equal(s.mouse.value,undefined);
});

test('rapid edits are serialized and retain their original device and values', async () => {
  const payloads: Record<string, unknown>[] = [];
  let finish!: (r: Reply) => void;
  const s = createDeviceState(p => {
    payloads.push(p);
    return payloads.length === 1 ? new Promise(resolve => { finish = resolve; }) : Promise.resolve(reply(3));
  });
  s.receive(reply(1));
  const first = s.set('dpi', 2000);
  const changes = { '83': 86 };
  const second = s.request('remap', { changes });
  changes['83'] = 83;
  s.selected.value = 'other';
  assert.equal(payloads.length, 1);
  finish(reply(2));
  assert.equal(await first, true);
  assert.equal(await second, true);
  assert.deepEqual(payloads[1], { op: 'remap', device: 'mouse', changes: { '83': 86 }, autosave: true });
});

test('a disk-only profile copy preserves the last hardware reading', async () => {
  const s = createDeviceState(async () => reply(2, false));
  s.receive(reply(1));
  assert.equal(await s.request('save_copy', { name: 'Work copy' }), true);
  assert.equal(s.mouse.value?.id, 'mouse');
});

test('editing several shortcuts coalesces into one write before a named copy', async () => {
  const payloads: Record<string, unknown>[] = [];
  const s = createDeviceState(async p => { payloads.push(p); return reply(payloads.length + 1); });
  s.receive(reply(1));
  s.stageRemap('83', 'custom:ctrl:KeyS');
  s.stageRemap('83', 'custom:ctrl+shift:KeyS');
  s.stageRemap('86', 'custom:ctrl:F12');
  assert.equal(s.saving.value, true);
  assert.equal(payloads.length, 0);
  await s.request('save_copy', { name: 'Favorite' });
  assert.deepEqual(payloads, [
    { op: 'remap', device: 'mouse', changes: { '83': 'custom:ctrl+shift:KeyS', '86': 'custom:ctrl:F12' }, autosave: true },
    { op: 'save_copy', device: 'mouse', name: 'Favorite' },
  ]);
  assert.equal(s.saving.value, false);
});

test('switching away from an editor cannot discard or redirect a staged edit', async () => {
  const payloads: Record<string, unknown>[] = [];
  const s = createDeviceState(async p => { payloads.push(p); return reply(2); });
  s.receive(reply(1));
  s.stageRemap('83', 'custom:ctrl:KeyS');
  s.selected.value = 'other';
  await new Promise(resolve => setTimeout(resolve, 450));
  assert.deepEqual(payloads, [{ op: 'remap', device: 'mouse', changes: { '83': 'custom:ctrl:KeyS' }, autosave: true }]);
});

test('a native selection supersedes the unfinished shortcut for the same button', async () => {
  const payloads: Record<string, unknown>[] = [];
  const s = createDeviceState(async p => { payloads.push(p); return reply(2); });
  s.receive(reply(1));
  s.stageRemap('83', 'custom:ctrl:KeyS');
  await s.request('remap', { changes: { '83': 83 } });
  assert.deepEqual(payloads, [{ op: 'remap', device: 'mouse', changes: { '83': 83 }, autosave: true }]);
  assert.equal(s.saving.value, false);
});

test('a failed staged edit prevents a queued copy of the previous current profile', async () => {
  const sent: Record<string, unknown>[] = [];
  const s = createDeviceState(async p => { sent.push(p); return { ok: false, revision: 2, error: 'disk' }; });
  s.receive(reply(1));
  s.stageRemap('83', 'custom:ctrl:KeyS');
  assert.equal(await s.request('save_copy', { name: 'Favorite' }), false);
  assert.equal(sent.length, 1);
  assert.equal(s.error.value, 'disk');
});
