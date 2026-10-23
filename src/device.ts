import { onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { createDeviceState, type Reply } from './deviceState.ts';
import { t } from './i18n.ts';
export type { Mouse, Setting } from './deviceState.ts';

export function useDevice() {
  const state = createDeviceState(async payload => {
    if (!('__TAURI_INTERNALS__' in window)) throw new Error(t('state.webOnly'));
    return invoke<Reply>('request', { payload });
  });
  let unlisten: UnlistenFn | undefined;
  let disposed = false;
  const refresh = () => { if (!state.busy.value) void state.request('snapshot'); };
  onMounted(async () => {
    try {
      if ('__TAURI_INTERNALS__' in window) {
        const release = await listen<Reply>('device-snapshot', event => state.receive(event.payload));
        if (disposed) { release(); return; }
        unlisten = release;
      }
      if (!disposed) refresh();
    } catch (e) {
      state.receive({ ok: false, revision: 0, error: t('state.serviceError', { error: String(e) }) });
    }
  });
  // No re-read on window focus: it blocked every control while the mouse answered.
  // The native poll (every 5 s, without blocking) keeps the view current.
  onUnmounted(() => { disposed = true; unlisten?.(); });
  return state;
}
