import { computed, ref } from 'vue';
import type { CustomShortcut, ShortcutCatalogue } from './shortcuts';
import { t } from './i18n.ts';
export interface Setting { value: number | boolean | null; available: boolean; choices?: number[] }
export interface GestureConfig { type: 'gestures'; units?: 'counts' | 'tenths-mm'; threshold: number; actions: Record<'up' | 'down' | 'left' | 'right' | 'click', string | null> }
export type ButtonAction = GestureConfig | CustomShortcut | number | 'modifier:alt' | 'modifier:ctrl' | 'shortcut:copy' | 'shortcut:paste' | 'shortcut:cut' | 'shortcut:undo' | 'desktop:workspace-previous' | 'desktop:workspace-next' | 'desktop:fullscreen' | 'desktop:maximize';
export interface ButtonControl { shortcuts?: ShortcutCatalogue; desktop_error?: string | null; gestures_available?: boolean; gesture_actions?: { value: string; label: string; disabled?: boolean }[]; id: string; label: string; value: ButtonAction | null; available: boolean; choices: { value: Exclude<ButtonAction, GestureConfig>; label: string; disabled?: boolean }[]; error?: string }
export interface Mouse { id: string; name: string; connection: string; protocol: number; battery: number | null; battery_status?: 'charging' | 'full' | 'discharging' | null; settings: Record<string, Setting>; buttons?: { available: boolean; controls: ButtonControl[]; gesture_feedback?: { sequence: number; direction: string | null; status: string } | null; error?: string } }
export interface ProfileTransfer { document?: Record<string, unknown>; imports?: { name: string; assigned_name: string }[] }
export interface Snapshot { desktop?: { available: boolean; name: string; active_application: string | null; applications?: string[]; error?: string } | null; rules?: { device: string; application: string; profile: string }[]; devices: Mouse[]; profiles: { device: string; name: string }[]; active: Record<string, string>; warnings: string[]; diagnostic: string | null; reconciliation?: { device: string; profile: string; applied: string[] }[]; transfer?: ProfileTransfer | null; profile_version?: number }
export interface Reply { ok: boolean; data?: Snapshot; error?: string; revision: number }
export type Transport = (payload: Record<string, unknown>) => Promise<Reply>;

/** State independent of window timers. Revisions order poll events and IPC replies. */
export function createDeviceState(transport: Transport) {
  const data = ref<Snapshot>({ devices: [], profiles: [], active: {}, warnings: [], diagnostic: null });
  const selected = ref(''); const busy = ref(false); const message = ref('');
  const connectionError = ref(''); const operationError = ref(''); const initialized = ref(false);
  const error = computed(() => operationError.value || connectionError.value);
  const mouse = computed(() => data.value.devices.find(d => d.id === selected.value));
  const profiles = computed(() => data.value.profiles.filter(p => p.device === selected.value));
  let revision = -1;
  function receive(reply: Reply) {
    if (reply.revision <= revision) return;
    revision = reply.revision;
    initialized.value = true;
    if (!reply.ok || !reply.data) {
      connectionError.value = reply.error || t('state.readError');
      data.value = { ...data.value, devices: [] };
      message.value = '';
      return;
    }
    connectionError.value = '';
    if (selected.value === '') {
      const known = [...new Set(reply.data.profiles.map(p => p.device))];
      selected.value = reply.data.devices[0]?.id || (known.length === 1 ? known[0] : '');
    }
    data.value = reply.data;
    if (!mouse.value || reply.data.warnings.length) message.value = '';
    const restored = reply.data.reconciliation?.find(r => r.device === selected.value && r.applied.length > 0);
    if (restored) message.value = t('state.restored', { name: restored.profile });
  }
  async function request(op: string, extra: Record<string, unknown> = {}, onSuccess?: (snapshot: Snapshot) => void) {
    if (busy.value) return false;
    const identity = selected.value;
    busy.value = true;
    operationError.value = ''; message.value = '';
    let responseRevision: number | undefined;
    const localOperation = ['rename','delete','import','import_preview','export','rule_set','rule_delete','applications'].includes(op);
    try {
      const reply = await transport({ op, device: identity || undefined, ...extra });
      responseRevision = reply.revision;
      if (reply.ok || !localOperation) receive(reply);
      else if (reply.revision > revision) revision = reply.revision;
      if (!reply.ok) throw new Error(reply.error || t('state.failed'));
      if (reply.data) onSuccess?.(reply.data);
      // The native poll may already have reported a later disconnect or recovery.
      // A completed operation remains successful, but its message is historical.
      if (reply.revision < revision) return true;
      if (op === 'set' && mouse.value) message.value = t('state.set');
      if (op === 'remap' && mouse.value) message.value = t('state.remap');
      if (op === 'save') message.value = t('state.save');
      if (op === 'restore') message.value = t('state.restore');
      if (op === 'auto') message.value = t('state.auto');
      if (op === 'rename') message.value = t('state.rename');
      if (op === 'delete') message.value = t('state.delete');
      if (op === 'rule_set' || op === 'rule_delete') message.value = t('state.rules');
      if (op === 'import') message.value = t('state.import');
      return true;
    } catch (e) {
      operationError.value = String(e instanceof Error ? e.message : e);
      // A write may have partially applied: never leave stale controls enabled.
      if (!localOperation && (responseRevision === undefined || responseRevision >= revision)) {
        data.value = { ...data.value, devices: [] };
        message.value = '';
      }
      return false;
    } finally { busy.value = false; }
  }
  async function set(key: string, value: number | boolean) { return request('set', { changes: { [key]: value } }); }
  return { data, selected, busy, error, message, mouse, profiles, initialized, receive, request, set };
}
