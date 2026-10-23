<script setup lang="ts">
import { computed, useId } from 'vue';
import ShortcutEditor from './ShortcutEditor.vue';
import { makeShortcut } from '../shortcuts';
import { optionLabel, t } from '../i18n';
import type { ShortcutCatalogue } from '../shortcuts';
const props = defineProps<{ label: string; options: { value: string | number; label: string; disabled?: boolean }[]; catalogue?: ShortcutCatalogue; disabled?: boolean; allowNone?: boolean }>();
const model = defineModel<string | number | null>({ required: true });
const id = useId();
const custom = computed(() => typeof model.value === 'string' && model.value.startsWith('custom:'));
const options = computed(() => props.options.filter(o => !String(o.value).startsWith('custom:')));
function select(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (value === 'custom' && props.catalogue) {
    model.value = makeShortcut('KeyS', ['ctrl'], props.catalogue);
  } else {
    const option = options.value.find(o => String(o.value) === value);
    if (option?.disabled) return;
    model.value = option?.value ?? null;
  }
}
</script>

<template>
  <div class="action-select">
    <label :for="id">{{ label }}</label>
    <select :id="id" :value="custom ? 'custom' : model ?? ''" :disabled="disabled" @change="select">
      <option value="" :disabled="!allowNone">{{ allowNone ? t('action.none') : t('assign.unconfirmed') }}</option>
      <option v-for="option in options" :key="option.value" :value="option.value" :disabled="option.disabled">{{ optionLabel(option.value, option.label) }}{{ option.disabled ? t('action.unavailable') : '' }}</option>
      <option v-if="catalogue" value="custom">{{ t('action.custom') }}</option>
    </select>
    <ShortcutEditor v-if="custom && catalogue" :model-value="String(model)" :catalogue="catalogue" :label="label" :disabled="disabled" @update:model-value="model = $event" />
  </div>
</template>

<style scoped>
.action-select { display: flex; flex-direction: column; gap: .4rem; min-width: 0; }
.action-select > select { width: 100%; padding: .5rem; }
</style>
