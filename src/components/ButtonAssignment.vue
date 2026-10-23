<script setup lang="ts">
import { computed, shallowRef, watch } from 'vue';
import ActionSelect from './ActionSelect.vue';
import type { ButtonControl } from '../deviceState';
import { optionLabel, t, tr } from '../i18n';
const props = defineProps<{ control: ButtonControl; disabled: boolean }>();
const emit = defineEmits<{ apply: [action: string | number] }>();
const draft = shallowRef<string | number | null>(null);
watch(() => props.control.value, value => { draft.value = typeof value === 'object' ? null : value; }, { immediate: true });
watch(() => props.disabled, disabled => {
  if (!disabled && !custom.value) draft.value = typeof props.control.value === 'object' ? null : props.control.value;
});
const custom = computed(() => typeof draft.value === 'string' && draft.value.startsWith('custom:'));
const options = computed(() => props.control.choices.map(choice => ({ ...choice, label: optionLabel(choice.value, choice.label) + (choice.value === Number(props.control.id) ? t('assign.original') : '') })));
const label = computed(() => optionLabel(props.control.id, props.control.label));
function select(action: string | number | null) {
  draft.value = action;
  if (action !== null && !custom.value) emit('apply', action);
}
</script>

<template>
  <div class="field button-assignment">
    <template v-if="control.id !== '195'">
      <ActionSelect :model-value="draft" :label="label" :options="options" :catalogue="control.shortcuts" :disabled="disabled || !control.available" @update:model-value="select" />
      <button v-if="custom" class="apply-shortcut" :disabled="disabled || !control.available || draft === control.value" @click="draft !== null && emit('apply', draft)">{{ t('assign.applyShortcut') }}</button>
    </template>
    <label v-else>{{ label }}
      <select :value="typeof control.value === 'object' && control.value ? 'gestures' : control.value ?? ''" :disabled="disabled || !control.available" @change="select(Number(($event.target as HTMLSelectElement).value))">
        <option disabled value="">{{ t('assign.unconfirmed') }}</option>
        <option v-if="typeof control.value === 'object' && control.value" disabled value="gestures">{{ t('assign.gesturesActive') }}</option>
        <option v-for="choice in options" :key="choice.value" :value="choice.value">{{ choice.label }}</option>
      </select>
    </label>
    <small v-if="control.error" role="alert">{{ tr(control.error) }}</small>
    <small v-if="control.id !== '195' && control.desktop_error">{{ t('assign.desktopUnavailable', { error: tr(control.desktop_error) }) }}</small>
  </div>
</template>

<style scoped>
.button-assignment { display: block; margin: 0; padding: 12px 0; border-top: 1px solid var(--line); }
.button-assignment:first-child { border-top: 0; }
.button-assignment > label { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.35fr); align-items: center; gap: 12px; font-size: 12px; }
.apply-shortcut { display: block; margin: 10px 0 0 auto; border: 1px solid transparent; background: var(--accent-bg); color: var(--accent); border-radius: 8px; padding: 8px 12px; font-size: 11px; cursor: pointer; }
.apply-shortcut:disabled { opacity: .45; cursor: default; }
</style>
