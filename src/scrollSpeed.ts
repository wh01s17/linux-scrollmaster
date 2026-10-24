import { shallowRef } from 'vue';
import { t } from './i18n.ts';

/** Hyprland per-device scroll factor; `factor` is null while the global value applies. */
export interface ScrollSpeed { available: boolean; factor: number | null; saved: number | null; min: number; max: number; error: string | null }
export type ScrollTransport = (command: string, args?: { factor: number | null }) => Promise<unknown>;

const optionalNumber = (value: unknown) => value === null || (typeof value === 'number' && Number.isFinite(value));
function reply(value: unknown): ScrollSpeed {
  const data = value as ScrollSpeed;
  if (!data || typeof data.available !== 'boolean' || !optionalNumber(data.factor) || !optionalNumber(data.saved)
    || typeof data.min !== 'number' || typeof data.max !== 'number' || (data.error !== null && typeof data.error !== 'string')) throw new Error(t('scroll.readError'));
  return data;
}
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Separate from HID state: a desktop failure never discards mouse readings. */
export function createScrollSpeed(transport: ScrollTransport, native: boolean) {
  const value = shallowRef<ScrollSpeed>({ available: false, factor: null, saved: null, min: 0.1, max: 5, error: null });
  const busy = shallowRef(false), error = shallowRef('');
  async function load() {
    if (!native || busy.value) return;
    busy.value = true;
    try { value.value = reply(await transport('scroll_speed_preferences')); error.value = ''; }
    catch (e) { error.value = errorText(e); }
    finally { busy.value = false; }
  }
  async function set(factor: number | null) {
    if (!native || busy.value || !value.value.available) return;
    if (factor !== null && !(factor >= value.value.min && factor <= value.value.max)) { error.value = t('scroll.invalid', { min: value.value.min, max: value.value.max }); return; }
    busy.value = true; error.value = '';
    try { value.value = reply(await transport('set_scroll_speed', { factor })); }
    catch (e) {
      error.value = errorText(e);
      // Show what Hyprland actually holds, never the requested value.
      try { value.value = reply(await transport('scroll_speed_preferences')); } catch { /* keep the error */ }
    } finally { busy.value = false; }
  }
  return { value, busy, error, load, set };
}
