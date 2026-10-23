import { computed, shallowRef } from 'vue';
import { t } from './i18n.ts';

export interface TrayPreferences { tray: boolean; error: string | null }
export interface AutostartPreferences { enabled: boolean; can_change: boolean; needs_update: boolean; error: string | null }
export type DesktopTransport = (command: string, args?: { enabled: boolean }) => Promise<unknown>;

function trayReply(value: unknown): TrayPreferences {
  const data = value as TrayPreferences;
  if (!data || typeof data.tray !== 'boolean' || (data.error !== null && typeof data.error !== 'string')) throw new Error(t('prefs.trayReadError'));
  return data;
}
function autostartReply(value: unknown): AutostartPreferences {
  const data = value as AutostartPreferences;
  if (!data || typeof data.enabled !== 'boolean' || typeof data.can_change !== 'boolean' || typeof data.needs_update !== 'boolean' || (data.error !== null && typeof data.error !== 'string')) throw new Error(t('prefs.autostartReadError'));
  return data;
}
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Separate from HID state: preference failures must never discard mouse readings. */
export function createDesktopPreferences(transport: DesktopTransport, native: boolean) {
  const tray = shallowRef<TrayPreferences>({ tray: false, error: null });
  const autostart = shallowRef<AutostartPreferences>({ enabled: false, can_change: false, needs_update: false, error: null });
  const busy = shallowRef(false), trayReady = shallowRef(false), autostartReady = shallowRef(false);
  const trayError = shallowRef(''), autostartError = shallowRef('');
  const error = computed(() => [trayError.value || tray.value.error, autostartError.value || autostart.value.error].filter(Boolean).join('\n'));
  async function load() {
    if (!native || busy.value) return;
    busy.value = true;
    const replies = await Promise.allSettled([transport('desktop_preferences'), transport('autostart_preferences')]);
    try {
      if (replies[0].status === 'rejected') throw replies[0].reason;
      tray.value = trayReply(replies[0].value); trayReady.value = true; trayError.value = '';
    } catch (e) { trayReady.value = false; trayError.value = errorText(e); }
    try {
      if (replies[1].status === 'rejected') throw replies[1].reason;
      autostart.value = autostartReply(replies[1].value); autostartReady.value = true; autostartError.value = '';
    } catch (e) { autostartReady.value = false; autostartError.value = errorText(e); }
    busy.value = false;
  }
  async function setTray(enabled: boolean) {
    if (!native || busy.value || !trayReady.value) return;
    busy.value = true; trayError.value = '';
    try { tray.value = trayReply(await transport('set_tray', { enabled })); }
    catch (e) {
      trayError.value = errorText(e);
      try { tray.value = trayReply(await transport('desktop_preferences')); }
      catch { trayReady.value = false; }
    } finally { busy.value = false; }
  }
  async function setAutostart(enabled: boolean) {
    if (!native || busy.value || !autostartReady.value || !autostart.value.can_change) return;
    busy.value = true; autostartError.value = '';
    try { autostart.value = autostartReply(await transport('set_autostart', { enabled })); }
    catch (e) {
      autostartError.value = errorText(e);
      // A directory fsync may fail after publish: always read actual file state.
      try { autostart.value = autostartReply(await transport('autostart_preferences')); }
      catch { autostartReady.value = false; }
    } finally { busy.value = false; }
  }
  return { tray, autostart, busy, trayReady, autostartReady, error, load, setTray, setAutostart };
}
