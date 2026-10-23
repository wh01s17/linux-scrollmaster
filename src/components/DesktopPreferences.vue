<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import { invoke, isTauri } from '@tauri-apps/api/core';
import AutostartPreference from './AutostartPreference.vue';
import { createDesktopPreferences } from '../desktopPreferences';
import { LOCALES, locale, setLocale, t, tr, type Locale } from '../i18n';
// Centered modal: the state loads once at mount so opening it shows confirmed values at once.
const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
const preferences = createDesktopPreferences((command, args) => invoke(command, args), isTauri());
const { tray, autostart, busy, trayReady, autostartReady, error } = preferences;
const dialog = ref<HTMLElement | null>(null);
onMounted(preferences.load);
watch(() => props.open, async open => { if (open) { await nextTick(); dialog.value?.focus(); } });
async function toggle(event: Event) {
  const input = event.target as HTMLInputElement;
  await preferences.setTray(input.checked);
  input.checked = tray.value.tray;
}
function language(event: Event) {
  const next = (event.target as HTMLSelectElement).value as Locale;
  setLocale(next);
  // The tray menu is native: it follows the saved language too (best effort).
  if (isTauri()) void invoke('set_locale', { locale: next }).catch(() => {});
}
</script>

<template>
  <div v-if="open" class="prefs-backdrop" @click.self="emit('close')" @keydown.esc="emit('close')">
    <section id="desktop-preferences" ref="dialog" class="prefs-dialog" role="dialog" aria-modal="true" tabindex="-1" aria-labelledby="desktop-preferences-title">
      <div class="prefs-head">
        <div><p class="eyebrow">{{ t('prefs.eyebrow') }}</p><h2 id="desktop-preferences-title">{{ t('prefs.title') }}</h2></div>
        <button class="prefs-close" :aria-label="t('prefs.close')" @click="emit('close')">✕</button>
      </div>
      <div class="pref">
        <label class="pref-row" for="prefs-language">{{ t('prefs.language') }}
          <select id="prefs-language" :value="locale" @change="language">
            <option v-for="option in LOCALES" :key="option.id" :value="option.id" :lang="option.id">{{ option.label }}</option>
          </select>
        </label>
        <small>{{ t('prefs.languageHint') }}</small>
      </div>
      <div class="pref divided">
        <label class="toggle-row">{{ t('prefs.tray') }}<input type="checkbox" :checked="tray.tray" :disabled="busy || !trayReady" @change="toggle"></label>
        <p>{{ t('prefs.trayHint') }}</p>
        <small>{{ t('prefs.trayNote') }}</small>
      </div>
      <AutostartPreference :value="autostart" :busy="busy" :ready="autostartReady" @change="preferences.setAutostart" />
      <button v-if="error" class="retry" :disabled="busy || !isTauri()" @click="preferences.load">{{ t('prefs.retry') }}</button>
      <p v-if="error" class="notice error" role="alert">{{ error.split('\n').map(tr).join('\n') }}</p>
    </section>
  </div>
</template>

<style scoped>
.prefs-backdrop { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 24px; background: #0b0d10b3; backdrop-filter: blur(3px); }
.prefs-dialog { width: min(620px, 100%); max-height: calc(100% - 48px); overflow: auto; background: var(--card); border: 1px solid var(--line); border-radius: 16px; padding: 22px 26px 24px; box-shadow: 0 24px 60px #0008; outline: none; }
.prefs-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; margin-bottom: 6px; }
.prefs-head .eyebrow { margin: 0 0 6px; }
.prefs-close { border: 1px solid var(--line); background: transparent; color: var(--muted); border-radius: 8px; width: 32px; height: 32px; font-size: 13px; flex-shrink: 0; }
.prefs-close:hover { color: var(--text); background: var(--raised); }
.pref { padding: 14px 0 4px; }
.pref.divided { margin-top: 12px; border-top: 1px solid var(--line); padding-top: 2px; }
.pref-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 12px; }
.pref-row select { min-width: 160px; }
p { font-size: 12px; line-height: 1.6; margin: 8px 0 6px; }
small { display: block; font-size: 10.5px; line-height: 1.5; opacity: .8; margin-top: 6px; }
.retry { border: 1px solid var(--line); background: var(--raised); color: inherit; border-radius: 8px; padding: .6rem; margin-top: .8rem; }
</style>
