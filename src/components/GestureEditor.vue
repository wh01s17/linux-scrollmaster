<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { physicalDistance, validDistance, changeDistanceUnits, distanceLabel } from '../gestureDistance';
import type { createDeviceState, GestureConfig } from '../deviceState';
import ActionSelect from './ActionSelect.vue';
import { t, tr } from '../i18n';
const props = defineProps<{ state: ReturnType<typeof createDeviceState> }>();
const control = computed(() => props.state.mouse.value?.buttons?.controls.find(c => c.id === '195'));
const feedback = computed(() => props.state.mouse.value?.buttons?.gesture_feedback);
const feedbackLabel = computed(() => directions.value.find(d => d.id === feedback.value?.direction)?.label);
const active = computed(() => typeof control.value?.value === 'object' && control.value.value !== null);
const directions = computed(() => (['up', 'left', 'click', 'right', 'down'] as const).map(id => ({ id, label: t(`gesture.${id}`) })));
const options = computed(() => control.value?.gesture_actions || []);
function empty(): GestureConfig { return { type: 'gestures', units: 'tenths-mm', threshold: 30, actions: { up: null, down: null, left: null, right: null, click: null } }; }
const draft = ref<GestureConfig>(empty());
const dirty = ref(false);
watch(() => props.state.selected.value, () => { dirty.value = false; draft.value = empty(); }, { flush: 'sync' });
watch([() => props.state.selected.value, () => JSON.stringify(control.value?.value)], () => {
  const value = control.value?.value;
  if (dirty.value && props.state.saving.value && JSON.stringify(value) !== JSON.stringify(draft.value)) return;
  draft.value = typeof value === 'object' && value ? JSON.parse(JSON.stringify(value)) : empty();
  dirty.value = false;
}, { immediate: true });
const disabled = computed(() => props.state.busy.value || !control.value?.available);
const physical = computed(() => physicalDistance(draft.value));
const dpi = computed(() => {
  const setting = props.state.mouse.value?.settings.dpi;
  return setting?.available && typeof setting.value === 'number' && setting.value > 0 ? setting.value : null;
});
const valid = computed(() => validDistance(draft.value) && (!physical.value || dpi.value !== null) && Object.values(draft.value.actions).some(Boolean));
function distanceUnits(event: Event) {
  const input = event.target as HTMLInputElement;
  if (dpi.value !== null) { draft.value = changeDistanceUnits(draft.value, input.checked, dpi.value); schedule(); }
  input.checked = physical.value;
}
function schedule() {
  dirty.value = true;
  if (valid.value) props.state.stageRemap('195', draft.value);
  else if (!Object.values(draft.value.actions).some(Boolean)) props.state.stageRemap('195', 195);
}
async function disable() { await props.state.request('remap', { changes: { '195': 195 } }); }
function action(direction: keyof GestureConfig['actions'], value: string | number | null) {
  draft.value.actions[direction] = typeof value === 'string' ? value : null;
  schedule();
}
</script>

<template>
  <details v-if="control?.gestures_available" class="field gesture-editor">
    <summary>{{ t('gesture.title') }} <small :class="{ on: active }">{{ active ? t('gesture.active') : t('gesture.inactive') }}</small></summary>
    <p>{{ t('gesture.intro') }}</p>
    <small class="gesture-note">{{ t('gesture.note') }}</small>
    <small v-if="control.desktop_error">{{ t('assign.desktopUnavailable', { error: tr(control.desktop_error) }) }}</small>
    <p v-if="active && feedback" role="status">{{ feedback.direction ? t('gesture.last', { direction: feedbackLabel ?? feedback.direction }) : t('gesture.ambiguous') }}</p>
    <div class="gesture-grid">
      <ActionSelect v-for="direction in directions" :key="direction.id" class="field" :class="'gesture-' + direction.id"
        :model-value="draft.actions[direction.id]" :label="direction.label" :options="options" :catalogue="control?.shortcuts" :disabled="disabled" allow-none
        @update:model-value="action(direction.id, $event)" />
    </div>
    <label class="toggle-row">{{ t('gesture.keepDistance') }}<input type="checkbox" :checked="physical" :disabled="disabled || dpi === null" @change="distanceUnits"></label>
    <label class="field gesture-distance">{{ t('gesture.distance') }} <output>{{ distanceLabel(draft) }}</output>
      <input v-model.number="draft.threshold" type="range" :min="physical ? 10 : 20" :max="physical ? 200 : 2000" :step="physical ? 1 : 20" :disabled="disabled" @input="dirty = true" @change="schedule">
      <small>{{ physical ? t('gesture.physicalHint') : t('gesture.countsHint') }}</small>
    </label>
    <div v-if="active" class="gesture-buttons"><button :disabled="disabled" @click="disable">{{ t('gesture.restore') }}</button></div>
    <small>{{ t('gesture.footer') }}</small>
  </details>
</template>

<style scoped>
.gesture-editor p { font-size: 11.5px; margin: 0 0 8px; }
.gesture-note { display: block; font-size: 10.5px; line-height: 1.55; margin-bottom: 10px; }
.gesture-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px 10px; margin: 6px 0 4px; }
.gesture-grid .field { display: flex; flex-direction: column; align-items: stretch; gap: 5px; margin: 0; font-size: 11px; color: var(--muted); }
.gesture-grid select { width: 100%; }
.gesture-up { grid-column: 2; } .gesture-left { grid-row: 2; } .gesture-click { grid-row: 2; } .gesture-right { grid-row: 2; } .gesture-down { grid-column: 2; }
.gesture-distance { display: flex; flex-wrap: wrap; align-items: center; gap: .6rem; }
.gesture-distance input, .gesture-distance small { flex-basis: 100%; }
.gesture-editor .gesture-distance { margin-top: 14px; font-size: 12px; }
.gesture-buttons { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 8px; }
.gesture-buttons button { border: 1px solid var(--line); background: transparent; color: var(--text); border-radius: 8px; padding: 8px 12px; font-size: 11.5px; cursor: pointer; }
.gesture-buttons button:first-child { border-color: transparent; background: var(--accent-bg); color: var(--accent); }
.gesture-buttons button:disabled { opacity: .45; cursor: default; }
</style>
