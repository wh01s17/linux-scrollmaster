<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import type { createScrollSpeed } from '../scrollSpeed';
import { t, tr } from '../i18n';
// Shown only on Hyprland with the MX Master listed; one value covers both wheels.
const props = defineProps<{ state: ReturnType<typeof createScrollSpeed> }>();
const { value, busy, error } = props.state;
const shown = computed(() => value.value.factor ?? 1);
const text = ref('');
watch(shown, v => { text.value = v.toFixed(2); }, { immediate: true });
async function slide(event: Event) {
  const input = event.target as HTMLInputElement;
  await props.state.set(Number(input.value));
  input.value = String(shown.value);
}
async function exact() {
  await props.state.set(Number(text.value));
  text.value = shown.value.toFixed(2);
}
</script>

<template>
  <div v-if="value.available" class="scroll-speed">
    <div class="setting-heading"><label for="scroll-speed">{{ t('scroll.title') }}</label><output>×{{ shown.toFixed(2) }}</output></div>
    <input id="scroll-speed" type="range" :min="value.min" :max="value.max" step="0.05" :value="shown" :disabled="busy" @change="slide">
    <div class="range-labels"><span>{{ t('scroll.less') }}</span><span>{{ t('scroll.more') }}</span></div>
    <div class="dpi-exact"><label for="scroll-speed-exact">{{ t('scroll.exact') }}</label><input id="scroll-speed-exact" v-model="text" type="number" :min="value.min" :max="value.max" step="0.05" :disabled="busy" @keydown.enter.prevent="exact"><button :disabled="busy" @click="exact">{{ t('pointer.apply') }}</button><button :disabled="busy || value.saved === null" @click="state.set(null)">{{ t('scroll.reset') }}</button></div>
    <small>{{ t('scroll.hint', { min: value.min, max: value.max }) }}</small>
  </div>
  <p v-if="error || (value.available && value.error)" class="notice error" role="alert">{{ tr(error || value.error) }}</p>
</template>

<style scoped>
.scroll-speed .dpi-exact { flex-wrap: wrap; }
</style>
