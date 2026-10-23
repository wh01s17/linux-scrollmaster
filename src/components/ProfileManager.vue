<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { createDeviceState, type ProfileTransfer } from '../deviceState';
import ApplicationProfiles from './ApplicationProfiles.vue';
import { exportProfileFile, importProfileFile } from '../profileFiles';
import { locale, t, tr } from '../i18n';
import { es } from '../locales/es.ts';
import { en } from '../locales/en.ts';

const props = defineProps<{ state: ReturnType<typeof createDeviceState> }>();
const { data, selected, busy, mouse, profiles, request } = props.state;
const profileName = ref(t('profiles.defaultName')); const chosen = ref(''); const fileBusy = ref(false);
const fileError = ref(''); const fileMessage = ref(''); const deleting = ref('');
const renaming = ref(false); const newName = ref('');
const importDocument = ref<Record<string, unknown> | null>(null);
const preview = ref<NonNullable<ProfileTransfer['imports']>>([]);
const disabled = computed(() => busy.value || fileBusy.value);

watch([selected, profiles], () => {
  if (!profiles.value.some(p => p.name === chosen.value))
    chosen.value = data.value.active[selected.value] || profiles.value[0]?.name || '';
}, { immediate: true });
// An untouched default name follows the language.
watch(locale, () => {
  if ([es['profiles.defaultName'], en['profiles.defaultName']].includes(profileName.value)) profileName.value = t('profiles.defaultName');
});
watch(selected, () => { importDocument.value = null; preview.value = []; deleting.value = ''; renaming.value = false; });

async function save() {
  const name = profileName.value.trim();
  if (await request('save', { name })) chosen.value = name;
}
async function rename() {
  const name = newName.value.trim();
  if (await request('rename', { name: chosen.value, new_name: name })) {
    chosen.value = name; renaming.value = false;
  }
}
async function remove() {
  if (await request('delete', { name: deleting.value })) deleting.value = '';
}
async function exportFile() {
  fileBusy.value = true; fileError.value = ''; fileMessage.value = '';
  try {
    let document: Record<string, unknown> | undefined;
    if (await request('export', { name: chosen.value }, snapshot => { document = snapshot.transfer?.document; })) {
      if (!document) throw new Error(t('profiles.noExport'));
      const path = await exportProfileFile(document);
      if (path) fileMessage.value = t('profiles.exported', { path });
    }
  } catch (e) { fileError.value = e instanceof Error ? e.message : String(e); }
  finally { fileBusy.value = false; }
}
async function selectImport() {
  fileBusy.value = true; fileError.value = ''; fileMessage.value = '';
  importDocument.value = null; preview.value = [];
  try {
    const document = await importProfileFile();
    if (document && await request('import_preview', { document }, snapshot => { preview.value = snapshot.transfer?.imports || []; }))
      importDocument.value = document;
  } catch (e) { fileError.value = e instanceof Error ? e.message : String(e); }
  finally { fileBusy.value = false; }
}
async function confirmImport() {
  if (!importDocument.value) return;
  if (await request('import', { document: importDocument.value }, snapshot => {
    const names = snapshot.transfer?.imports;
    if (names?.length) chosen.value = names[0].assigned_name;
  })) { importDocument.value = null; preview.value = []; }
}
async function automatic(event: Event) {
  const input = event.target as HTMLInputElement;
  await request('auto', { name: input.checked ? chosen.value : null });
  input.checked = !!data.value.active[selected.value];
}
</script>

<template>
  <section class="profiles-card">
    <div><p class="eyebrow">{{ t('profiles.eyebrow') }}</p><h2>{{ t('profiles.title') }}</h2><p>{{ t('profiles.intro') }}</p></div>
    <div class="profile-actions">
      <div class="inline"><input v-model="profileName" :aria-label="t('profiles.name')" maxlength="60" :placeholder="t('profiles.name')"><button :disabled="disabled || !mouse || !profileName.trim()" @click="save">{{ t('profiles.save') }}</button></div>
      <div class="inline"><select v-model="chosen" :aria-label="t('profiles.saved')"><option value="" disabled>{{ t('profiles.select') }}</option><option v-for="p in profiles" :key="p.name">{{ p.name }}</option></select><button :disabled="disabled || !mouse || !chosen" @click="request('restore', { name: chosen })">{{ t('profiles.restore') }}</button></div>
      <div class="inline profile-tools"><button :disabled="disabled || !chosen" @click="renaming = true; newName = chosen">{{ t('profiles.rename') }}</button><button :disabled="disabled || !chosen" @click="deleting = chosen">{{ t('profiles.delete') }}</button><button :disabled="disabled || !chosen" @click="exportFile">{{ t('profiles.export') }}</button><button :disabled="disabled || !selected" @click="selectImport">{{ t('profiles.import') }}</button></div>
      <div v-if="renaming" class="profile-confirm"><label>{{ t('profiles.newName') }}<input v-model="newName" maxlength="60" :aria-label="t('profiles.newNameAria')"></label><div class="inline"><button :disabled="disabled || !newName.trim()" @click="rename">{{ t('profiles.saveName') }}</button><button @click="renaming = false">{{ t('profiles.cancel') }}</button></div></div>
      <div v-if="deleting" class="profile-confirm"><p>{{ t('profiles.confirmDelete', { name: deleting }) }}</p><div class="inline"><button :disabled="disabled" @click="remove">{{ t('profiles.deleteProfile') }}</button><button @click="deleting = ''">{{ t('profiles.cancel') }}</button></div></div>
      <div v-if="importDocument" class="profile-confirm"><p>{{ t('profiles.importPreview', { count: preview.length }) }}</p><ul><li v-for="(item, index) in preview" :key="index">{{ item.assigned_name }}<small v-if="item.name !== item.assigned_name">{{ t('profiles.originalName', { name: item.name }) }}</small></li></ul><div class="inline"><button :disabled="disabled" @click="confirmImport">{{ t('profiles.confirmImport') }}</button><button @click="importDocument = null; preview = []">{{ t('profiles.cancel') }}</button></div></div>
      <p v-if="fileError" class="notice error" role="alert">{{ tr(fileError) }}</p><p v-if="fileMessage" role="status">{{ fileMessage }}</p>
      <label class="auto"><input type="checkbox" :checked="!!data.active[selected]" :disabled="disabled || !selected || (!chosen && !data.active[selected])" @change="automatic">{{ data.active[selected] ? t('profiles.reapplyNamed', { name: data.active[selected] }) : t('profiles.reapplySelected') }}</label><small>{{ t('profiles.reapplyHint') }}</small>
      <ApplicationProfiles :state="state" />
    </div>
  </section>
</template>
