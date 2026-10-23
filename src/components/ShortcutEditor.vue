<script setup lang="ts">
import { computed } from 'vue';
import { makeShortcut, readShortcut, shortcutLabel } from '../shortcuts';
import type { ShortcutCatalogue } from '../shortcuts';
import { optionLabel, t } from '../i18n';
const props = defineProps<{ catalogue: ShortcutCatalogue; label: string; disabled?: boolean }>();
const model = defineModel<string>({ required: true });
const parsed = computed(() => readShortcut(model.value, props.catalogue));
const label = computed(() => shortcutLabel(model.value, props.catalogue));
function update(key: string, modifiers: string[]) {
  const action = makeShortcut(key, modifiers, props.catalogue);
  if (action) model.value = action;
}
function modifier(value: string, event: Event) {
  if (!parsed.value) return;
  const remaining = parsed.value.modifiers.filter(m => m !== value);
  update(parsed.value.key, (event.target as HTMLInputElement).checked ? [...remaining, value] : remaining);
}
</script>

<template>
  <fieldset class="shortcut-editor" :disabled="disabled">
    <legend>{{ t('shortcut.legend', { label: props.label }) }}</legend>
    <div class="shortcut-modifiers">
      <label v-for="option in catalogue.modifiers" :key="option.value" class="shortcut-modifier">
        <input type="checkbox" :checked="parsed?.modifiers.includes(option.value)" @change="modifier(option.value, $event)">{{ option.label }}
      </label>
    </div>
    <label class="shortcut-key">{{ t('shortcut.key') }}
      <select :value="parsed?.key" @change="update(($event.target as HTMLSelectElement).value, parsed?.modifiers || [])">
        <option v-for="key in catalogue.keys" :key="key.value" :value="key.value">{{ optionLabel(key.value, key.label) }}</option>
      </select>
    </label>
    <output class="shortcut-preview" aria-live="polite">{{ label }}</output>
    <small>{{ t('shortcut.hint') }}</small>
  </fieldset>
</template>

<style scoped>
.shortcut-editor { border: 1px solid var(--line); border-radius: 8px; padding: .7rem; min-width: 0; margin: .6rem 0; }
.shortcut-modifiers { display: flex; flex-wrap: wrap; gap: .6rem; }
.shortcut-modifier { display: flex; align-items: center; gap: .3rem; }
.shortcut-key { display: flex; align-items: center; gap: .5rem; margin: .7rem 0; }
.shortcut-key select { min-width: 120px; }
.shortcut-preview { display: block; margin-bottom: .5rem; font-weight: 600; }
</style>
