// SPDX-License-Identifier: GPL-3.0-or-later
import assert from 'node:assert/strict';
import test from 'node:test';
import { es } from '../src/locales/es.ts';
import { en, backendEn } from '../src/locales/en.ts';
import { batteryView } from '../src/battery.ts';
import { locale, optionLabel, setLocale, t, tr } from '../src/i18n.ts';

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map(m => m[1]).sort();

test('English covers every Spanish key with the same placeholders and markup', () => {
  assert.deepEqual(Object.keys(en).sort(), Object.keys(es).sort());
  for (const key of Object.keys(es) as (keyof typeof es)[]) {
    assert.deepEqual(placeholders(en[key]), placeholders(es[key]), key);
    assert.equal(en[key].includes('<strong>'), es[key].includes('<strong>'), key);
    assert.ok(en[key].trim().length > 0, key);
  }
});
test('switching language changes interface text and formatted values', () => {
  setLocale('en');
  assert.equal(locale.value, 'en');
  assert.equal(t('zone.thumb'), 'Thumb wheel');
  assert.equal(t('pointer.range', { min: 200, max: 8000, step: 50 }), '200–8000 DPI · step 50');
  assert.equal(batteryView(80, 'charging').label, 'Battery 80%, charging');
  setLocale('es');
  assert.equal(t('zone.thumb'), 'Rueda lateral');
  assert.equal(batteryView(80, 'charging').label, 'Batería 80%, cargando');
});
test('backend labels translate by stable value and keep unknown text', () => {
  setLocale('en');
  assert.equal(optionLabel(83, 'Atrás'), 'Back');
  assert.equal(optionLabel('shortcut:copy', 'Copiar (Ctrl+C)'), 'Copy (Ctrl+C)');
  assert.equal(optionLabel('KeyA', 'A'), 'A');
  assert.equal(optionLabel(4242, 'Función nativa 1092'), 'Native function 1092');
  assert.equal(tr('El mouse no responde: está apagado, en reposo o fuera de alcance. Se reintentará automáticamente.'), backendEn.exact['El mouse no responde: está apagado, en reposo o fuera de alcance. Se reintentará automáticamente.']);
  assert.equal(tr('Sin acceso al receptor Logi Bolt: Permission denied'), 'No access to the Logi Bolt receiver: Permission denied');
  assert.equal(tr('Mensaje nuevo sin traducir'), 'Mensaje nuevo sin traducir');
  setLocale('es');
  assert.equal(optionLabel(83, 'Atrás'), 'Atrás');
  assert.equal(tr('Sin acceso al receptor Logi Bolt: x'), 'Sin acceso al receptor Logi Bolt: x');
});
