<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { useDevice } from './device';
import type { ButtonAction } from './deviceState';
import ProfileManager from './components/ProfileManager.vue';
import GestureEditor from './components/GestureEditor.vue';
import DesktopPreferences from './components/DesktopPreferences.vue';
import ButtonAssignment from './components/ButtonAssignment.vue';
import { batteryView } from './battery';
import MouseFigure from './components/MouseFigure.vue';
import ScrollSpeed from './components/ScrollSpeed.vue';
import { createScrollSpeed } from './scrollSpeed';
import { locale, t, tr } from './i18n';
const device = useDevice();
const { data, selected, busy, error, message, mouse, request, set } = device;
// Desktop setting (Hyprland), separate from HID state; shared by both wheel tabs.
const scrollSpeed = createScrollSpeed((command, args) => invoke(command, args), isTauri());
onMounted(scrollSpeed.load);
const zone = ref('pointer'); const dark = ref(localStorage.getItem('linux-scrollmaster-theme') !== 'light');
watch(zone, z => { if (z === 'wheel' || z === 'thumb') void scrollSpeed.load(); });
const keySample = ref('');
const shortcutSample = ref('');
function testModifier(event: KeyboardEvent) {
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
  const held = [event.ctrlKey ? 'Ctrl' : '', event.altKey ? 'Alt' : ''].filter(Boolean);
  keySample.value = t('buttons.lastKey', { held: held.length ? held.join(' + ') : t('buttons.noModifiers'), key: event.key.toUpperCase() });
}
const zones = computed(() => ([['pointer', '↗'], ['wheel', '↕'], ['thumb', '↔'], ['buttons', '⌘']] as const).map(([id, icon]) => ({ id, icon, label: t(`zone.${id}`) })));
const title = computed(() => zones.value.find(z => z.id === zone.value)?.label);
const howItems = ['buttons.howCustom', 'buttons.howModifiers', 'buttons.howEdit', 'buttons.howDesktop', 'buttons.howProfiles'] as const;
const battery = computed(() => batteryView(mouse.value?.battery, mouse.value?.battery_status));
const dpi = computed(() => mouse.value?.settings.dpi?.choices ?? []);
const dpiText = ref(''); const dpiError = ref('');
watch(() => mouse.value?.settings.dpi?.value, v => { dpiText.value = v == null ? '' : String(v); }, { immediate: true });
async function applyExactDpi() {
  const target = Number(dpiText.value);
  if (!Number.isInteger(target) || !dpi.value.includes(target)) {
    dpiError.value = t('pointer.invalid');
    return;
  }
  dpiError.value = '';
  if (target !== value('dpi')) await set('dpi', target);
  dpiText.value = String(value('dpi') ?? '');
}
function value(key: string) { return mouse.value?.settings[key]?.value; }
function available(key: string) { return !busy.value && mouse.value?.settings[key]?.available; }
async function numberChange(key: string, e: Event) {
  const input = e.target as HTMLInputElement;
  await set(key, Number(input.value));
  input.value = String(value(key) ?? '');
}
async function dpiChange(e: Event) {
  const input = e.target as HTMLInputElement;
  const target = dpi.value[Number(input.value)];
  if (target !== undefined) await set('dpi', target);
  input.value = String(Math.max(0, dpi.value.indexOf(Number(value('dpi')))));
}
async function toggle(key: string, e: Event) {
  const input = e.target as HTMLInputElement;
  await set(key, input.checked);
  // DOM inputs change optimistically; restore them even when the model value is unchanged.
  input.checked = !!value(key);
}
async function remapButton(id: string, action: string | number) {
  if (typeof action === 'string' && action.startsWith('custom:')) {
    device.stageRemap(id, action as ButtonAction);
    return;
  }
  await request('remap', { changes: { [id]: action } });
}
const preferencesOpen = ref(false);
function openPreferences() { preferencesOpen.value = true; }
function theme() { dark.value = !dark.value; localStorage.setItem('linux-scrollmaster-theme', dark.value ? 'dark' : 'light'); }
</script>

<template>
  <div class="app" :class="{ light: !dark }" :lang="locale">
    <aside>
      <div class="brand"><svg class="brand-mark" viewBox="38 14 52 100" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><rect x="41.5" y="21.5" width="45" height="85" rx="22.5" stroke-width="7"/><path d="M64 22v26" stroke-width="6"/><path d="M56 72l8-8 8 8M56 86l8 8 8-8" stroke-width="5"/></g><rect x="58.5" y="30" width="11" height="20" rx="5.5" fill="currentColor"/></svg><span class="brand-name">Linux <strong>ScrollMaster</strong></span></div>
      <p class="eyebrow">{{ t('app.workspace') }}</p>
      <button class="device-nav" @click="zone = 'pointer'"><span>◒</span><div>MX Master 3S<small>{{ mouse ? t('app.connected') : t('app.disconnected') }}</small></div><i :class="{ online: mouse }"></i></button>
      <div class="sidebar-bottom"><span>{{ t('app.madeForLinux') }}</span><small>{{ t('app.localControl') }}</small><button class="theme" @click="theme">{{ dark ? t('app.themeLight') : t('app.themeDark') }}</button></div>
    </aside>
    <main>
      <header><div class="breadcrumb">{{ t('app.devices') }} <span>/</span> MX Master 3S</div><div class="header-tools"><button class="refresh" @click="openPreferences">{{ t('app.preferences') }}</button><span class="badge">{{ t('app.preview') }}</span></div></header>
      <section class="heading"><div><p class="eyebrow accent">{{ t('app.tagline') }}</p><h1>{{ mouse?.name ?? 'MX Master 3S' }}</h1><span class="connection"><i :class="{ online: mouse }"></i>{{ mouse?.connection ?? t('app.waiting') }}</span><p>{{ t('app.subtitle') }}</p></div><div class="heading-tools"><button class="refresh" :disabled="busy" @click="request('snapshot')">{{ busy ? t('app.reading') : t('app.refresh') }}</button><span class="battery" :class="battery.tone" role="img" :aria-label="battery.label" :title="battery.label"><span class="battery-icon" aria-hidden="true"><span class="battery-fill" :style="{ width: battery.fill + '%' }"></span></span><strong>{{ battery.text }}</strong><span v-if="battery.charging" class="battery-bolt" aria-hidden="true">⚡</span></span></div></section>
      <div v-if="error" class="notice error" role="alert">{{ tr(error) }}</div>
      <div v-if="data.diagnostic" class="notice" role="status">{{ tr(data.diagnostic) }}</div>
      <div v-for="warning in data.warnings" :key="warning" class="notice error">{{ tr(warning) }}</div>
      <section class="workspace">
        <div class="visual-card">
          <select v-if="data.devices.length > 1" v-model="selected" :aria-label="t('app.selectMouse')"><option v-for="d in data.devices" :key="d.id" :value="d.id">{{ d.name }} · {{ d.id }}</option></select>
          <MouseFigure :zone="zone" @select="zone = $event" />
          <p class="visual-hint">{{ t('app.figureHint') }}</p>
          <small class="figure-credit">{{ t('figure.credit') }}</small>
          <div class="hardware-note"><span>◈</span><div>{{ t('app.onDevice') }}<small>{{ t('app.onDeviceHint') }}</small></div></div>
        </div>
        <div class="settings-card">
          <nav :aria-label="t('app.controls')"><button v-for="z in zones" :key="z.id" :class="{ selected: zone === z.id }" @click="zone = z.id"><span>{{ z.icon }}</span>{{ z.label }}</button></nav>
          <div class="panel"><p class="eyebrow">{{ t('app.controlSettings') }}</p><h2>{{ title }}</h2>
            <template v-if="zone === 'pointer'">
              <p>{{ t('pointer.intro') }}</p>
              <div class="setting-heading"><label for="dpi">{{ t('pointer.sensitivity') }}</label><output>{{ value('dpi') ?? '—' }} <small>DPI</small></output></div>
              <input id="dpi" type="range" :min="0" :max="Math.max(0, dpi.length - 1)" :value="dpi.indexOf(Number(value('dpi')))" :disabled="!available('dpi')" @change="dpiChange">
              <div class="range-labels"><span>{{ t('pointer.precise') }}</span><span>{{ t('pointer.fast') }}</span></div>
              <div class="dpi-exact"><label for="dpi-exact">{{ t('pointer.exact') }}</label><input id="dpi-exact" v-model="dpiText" type="number" :min="dpi[0]" :max="dpi[dpi.length - 1]" :step="dpi.length > 1 ? dpi[1] - dpi[0] : 1" :disabled="!available('dpi')" @change="applyExactDpi" @keydown.enter.prevent="applyExactDpi"></div>
              <small v-if="dpi.length">{{ t('pointer.range', { min: dpi[0], max: dpi[dpi.length - 1], step: dpi.length > 1 ? dpi[1] - dpi[0] : 1 }) }}</small>
              <p v-if="dpiError" role="alert">{{ dpiError }}</p>
              <p class="tip">{{ t('pointer.tip') }}</p>
            </template>
            <template v-if="zone === 'wheel'">
              <p>{{ t('wheel.intro') }}</p>
              <label class="field">{{ t('wheel.mode') }}<select :value="value('scroll-ratchet') ?? ''" :disabled="!available('scroll-ratchet')" @change="numberChange('scroll-ratchet', $event)"><option disabled value="">{{ t('wheel.unread') }}</option><option :value="2">{{ t('wheel.ratchet') }}</option><option :value="1">{{ t('wheel.free') }}</option></select></label>
              <div class="setting-heading"><label for="shift">{{ t('wheel.threshold') }}</label><output>{{ value('smart-shift') ?? '—' }}</output></div>
              <input id="shift" type="range" min="1" max="50" :value="value('smart-shift') ?? 10" :disabled="!available('smart-shift') || value('scroll-ratchet') === 1" @change="numberChange('smart-shift', $event)">
              <div class="range-labels"><span>{{ t('wheel.earlier') }}</span><span>{{ t('wheel.always') }}</span></div>
              <label class="toggle-row">{{ t('wheel.invert') }}<input type="checkbox" :checked="!!value('hires-smooth-invert')" :disabled="!available('hires-smooth-invert')" @change="toggle('hires-smooth-invert', $event)"></label>
              <ScrollSpeed :state="scrollSpeed" />
              <label class="toggle-row">{{ t('wheel.hires') }}<input type="checkbox" :checked="!!value('hires-smooth-resolution')" :disabled="!available('hires-smooth-resolution')" @change="toggle('hires-smooth-resolution', $event)"></label>
            </template>
            <template v-if="zone === 'thumb'">
              <p>{{ t('thumb.intro') }}</p>
              <label class="toggle-row">{{ t('thumb.invert') }}<input type="checkbox" :checked="!!value('thumb-scroll-invert')" :disabled="!available('thumb-scroll-invert')" @change="toggle('thumb-scroll-invert', $event)"></label>
              <ScrollSpeed :state="scrollSpeed" />
              <p class="tip">{{ t('thumb.tip') }}</p>
            </template>
            <template v-if="zone === 'buttons'">
              <p>{{ t('buttons.intro') }}</p>
              <p v-if="mouse?.buttons?.error" class="notice error" role="alert">{{ tr(mouse.buttons.error) }}</p>
              <template v-if="mouse?.buttons?.controls.length">
                <div class="assignments">
                  <ButtonAssignment v-for="control in mouse.buttons.controls" :key="selected + ':' + control.id" :control="control" :disabled="busy" @apply="remapButton(control.id, $event)" />
                </div>
                <GestureEditor :state="device" />
                <details class="info-card"><summary>{{ t('buttons.try') }}</summary>
                  <div class="try"><h3>{{ t('buttons.tryModifiers') }}</h3>
                    <p>{{ t('buttons.tryModifiersHint') }}</p>
                    <input :aria-label="t('buttons.tryModifiersAria')" :placeholder="t('buttons.tryModifiersPlaceholder')" readonly :disabled="!mouse" @keydown.stop.prevent="testModifier">
                    <small role="status">{{ keySample || t('buttons.noKeyYet') }}</small>
                  </div>
                  <div class="try"><h3>{{ t('buttons.tryCopy') }}</h3>
                    <p>{{ t('buttons.tryCopyHint') }}</p>
                    <textarea v-model="shortcutSample" class="action-test" :aria-label="t('buttons.tryCopyAria')" rows="3" :placeholder="t('buttons.tryCopyPlaceholder')" :disabled="!mouse"></textarea>
                  </div>
                </details>
              </template>
              <p v-else class="tip">{{ mouse ? t('buttons.noControls') : t('buttons.connect') }}</p>
              <details class="info-card"><summary>{{ t('buttons.how') }}</summary>
                <!-- Own catalogue text with <strong> markup, never device or user data. -->
                <ul class="info-list"><li v-for="key in howItems" :key="key" v-html="t(key)"></li></ul>
              </details>
            </template>
            <div class="confirmation" role="status">{{ busy ? t('app.communicating') : message || t('app.appliedOnRelease') }}</div>
          </div>
        </div>
      </section>
      <ProfileManager :state="device" />
      <DesktopPreferences :open="preferencesOpen" @close="preferencesOpen = false" />
      <footer>{{ t('app.footer') }} <span>{{ t('app.noCloud') }}</span></footer>
    </main>
  </div>
</template>
