import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n.ts';

function desktop() {
  if (!('__TAURI_INTERNALS__' in window)) throw new Error(t('profiles.desktopOnly'));
}
export async function importProfileFile(): Promise<Record<string, unknown> | null> {
  desktop(); return invoke('import_profile_file');
}
export async function exportProfileFile(document: Record<string, unknown>): Promise<string | null> {
  desktop(); return invoke('export_profile_file', { document });
}
