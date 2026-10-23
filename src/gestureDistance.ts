import type { GestureConfig } from './deviceState';
import { numberFormat, t } from './i18n.ts';

export function physicalDistance(config: GestureConfig): boolean {
  return config.units === 'tenths-mm';
}

export function validDistance(config: GestureConfig): boolean {
  const [min, max] = physicalDistance(config) ? [10, 200] : [20, 2000];
  return Number.isInteger(config.threshold) && config.threshold >= min && config.threshold <= max;
}

/** Convert only at the user's explicit choice, retaining the current estimated reach. */
export function changeDistanceUnits(config: GestureConfig, physical: boolean, dpi: number): GestureConfig {
  if (!Number.isInteger(dpi) || dpi < 1 || dpi > 65535) throw new Error(t('gesture.needDpi'));
  if (physical === physicalDistance(config)) return { ...config, actions: { ...config.actions } };
  const { units: _units, ...previous } = config;
  const threshold = physical
    ? Math.max(10, Math.min(200, Math.round(config.threshold * 254 / dpi)))
    : Math.max(20, Math.min(2000, Math.round(config.threshold * dpi / 254)));
  return { ...previous, ...(physical ? { units: 'tenths-mm' as const } : {}), threshold, actions: { ...config.actions } };
}

export function distanceLabel(config: GestureConfig): string {
  return physicalDistance(config) ? `${numberFormat(config.threshold / 10)} mm` : String(config.threshold);
}
