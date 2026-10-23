<script setup lang="ts">
import { computed, ref } from 'vue';
import type { createDeviceState } from '../deviceState';
import { t, tr } from '../i18n';
const props = defineProps<{ state: ReturnType<typeof createDeviceState> }>();
const { data, selected, profiles, busy, request } = props.state;
const applications = ref<string[]>([]); const application = ref(''); const profile = ref('');
const rules = computed(() => data.value.rules?.filter(r => r.device === selected.value) || []);
const desktop = computed(() => data.value.desktop);
const globalProfile = computed(() => data.value.active[selected.value]);
const effective = computed(() => globalProfile.value ? rules.value.find(r => r.application === desktop.value?.active_application)?.profile || globalProfile.value : t('apps.off'));
async function refresh() {
  await request('applications', {}, snapshot => { applications.value = snapshot.desktop?.applications || []; });
  if (!applications.value.includes(application.value)) application.value = applications.value[0] || '';
}
async function save() {
  if (application.value && profile.value) await request('rule_set', { application: application.value, name: profile.value });
}
</script>

<template>
  <details class="application-profiles">
    <summary>{{ t('apps.title') }} <small>Hyprland</small></summary>
    <p>{{ t('apps.intro') }}</p>
    <p v-if="!desktop?.available" class="tip">{{ desktop?.error ? tr(desktop.error) : t('apps.unavailable') }} {{ t('apps.unavailableHint') }}</p>
    <p v-else role="status">{{ t('apps.status', { app: desktop.active_application || t('apps.noWindow'), profile: effective }) }}</p>
    <button :disabled="busy" @click="refresh">{{ t('apps.search') }}</button>
    <div class="application-form">
      <label class="field">{{ t('apps.application') }}<select v-model="application" :disabled="busy || !applications.length"><option disabled value="">{{ t('apps.searchFirst') }}</option><option v-for="app in applications" :key="app">{{ app }}</option></select></label>
      <label class="field">{{ t('apps.profile') }}<select v-model="profile" :disabled="busy || !profiles.length"><option disabled value="">{{ t('profiles.select') }}</option><option v-for="item in profiles" :key="item.name">{{ item.name }}</option></select></label>
      <button :disabled="busy || !selected || !application || !profiles.some(p => p.name === profile)" @click="save">{{ t('apps.saveRule') }}</button>
    </div>
    <ul v-if="rules.length"><li v-for="rule in rules" :key="rule.application"><span>{{ rule.application }} → {{ rule.profile }}</span><button :disabled="busy" @click="request('rule_delete', { application: rule.application })">{{ t('apps.remove') }}</button></li></ul>
    <small>{{ t('apps.footer') }}</small>
  </details>
</template>

<style scoped>
.application-profiles { border-top: 1px solid var(--line); margin-top: 1rem; padding-top: 1rem; }
.application-profiles summary { cursor: pointer; }
.application-form { display: flex; flex-wrap: wrap; align-items: end; gap: .6rem; margin: .7rem 0; }
.application-form .field { flex: 1; min-width: 180px; margin: 0; }
select { width: 100%; }
li { display: flex; gap: .5rem; justify-content: space-between; align-items: center; margin: .6rem 0; overflow-wrap: anywhere; }
button { border: 1px solid var(--line); background: transparent; color: inherit; border-radius: 8px; padding: .5rem; cursor: pointer; }
button:disabled { opacity: .45; cursor: default; }
</style>
