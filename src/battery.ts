// SPDX-License-Identifier: GPL-3.0-or-later
/** Pure battery presentation: no device access. */
import { t } from './i18n.ts';
export type BatteryStatus = 'charging' | 'full' | 'discharging' | null | undefined;
export interface BatteryView { text: string; label: string; fill: number; tone: 'unknown' | 'low' | 'ok' | 'charging'; charging: boolean }

export function batteryView(percent: number | null | undefined, status: BatteryStatus): BatteryView {
  const known = typeof percent === 'number' && Number.isFinite(percent);
  const value = known ? Math.min(100, Math.max(0, Math.round(percent))) : null;
  const charging = status === 'charging';
  const state = charging ? t('battery.charging') : status === 'full' ? t('battery.full') : '';
  return {
    text: value == null ? '—' : `${value}%`,
    label: value == null ? t('battery.unknown') : t('battery.level', { value, state }),
    fill: value ?? 0,
    tone: value == null ? 'unknown' : charging ? 'charging' : value <= 20 ? 'low' : 'ok',
    charging,
  };
}
