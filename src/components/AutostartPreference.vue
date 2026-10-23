<script setup lang="ts">
import type { AutostartPreferences } from '../desktopPreferences';
import { t } from '../i18n';
const props = defineProps<{ value: AutostartPreferences; busy: boolean; ready: boolean }>();
const emit = defineEmits<{ change: [enabled: boolean] }>();
function toggle(event: Event) {
  const input = event.target as HTMLInputElement;
  emit('change', input.checked);
  input.checked = props.value.enabled; // show confirmed state while native persistence runs
}
</script>

<template>
  <div class="autostart-preference">
    <label class="toggle-row">{{ t('prefs.autostart') }}<input type="checkbox" :checked="value.enabled" :disabled="busy || !ready || !value.can_change" @change="toggle"></label>
    <p>{{ t('prefs.autostartHint') }}</p>
    <small>{{ t('prefs.autostartNote') }}</small>
    <p v-if="value.needs_update" class="notice">{{ t('prefs.autostartMoved') }}</p>
    <button v-if="value.needs_update" class="update-location" :disabled="busy || !ready || !value.can_change" @click="emit('change', true)">{{ t('prefs.autostartUpdate') }}</button>
  </div>
</template>

<style scoped>
.autostart-preference { margin-top: 16px; border-top: 1px solid var(--line); padding-top: 2px; }
p { font-size: 12px; line-height: 1.6; margin: 8px 0 6px; }
small { font-size: 10.5px; line-height: 1.5; opacity: .8; }
.notice { font-size: 11.5px; margin: 12px 0 10px; }
.update-location { border: 1px solid transparent; background: var(--accent-bg); color: var(--accent); border-radius: 8px; padding: 8px 12px; font-size: 12px; }
</style>
