import { optionLabel, t } from './i18n.ts';
export interface ShortcutCatalogue {
  keys: { value: string; label: string }[];
  modifiers: { value: string; label: string }[];
}
export type CustomShortcut = `custom:${string}:${string}`;

export function readShortcut(value: unknown, catalogue: ShortcutCatalogue) {
  if (typeof value !== 'string' || value.length > 80) return null;
  const parts = value.split(':');
  if (parts.length !== 3 || parts[0] !== 'custom' || !catalogue.keys.some(k => k.value === parts[2])) return null;
  const modifiers = parts[1] ? parts[1].split('+') : [];
  const ordered = catalogue.modifiers.map(m => m.value).filter(m => modifiers.includes(m));
  if (ordered.join('+') !== modifiers.join('+')) return null;
  return { key: parts[2]!, modifiers };
}

export function makeShortcut(key: string, modifiers: string[], catalogue: ShortcutCatalogue): CustomShortcut | null {
  if (!catalogue.keys.some(k => k.value === key) || modifiers.some(m => !catalogue.modifiers.some(option => option.value === m))) return null;
  const ordered = catalogue.modifiers.map(m => m.value).filter(m => modifiers.includes(m));
  return `custom:${ordered.join('+')}:${key}`;
}

export function shortcutLabel(value: unknown, catalogue: ShortcutCatalogue): string {
  const parsed = readShortcut(value, catalogue);
  if (!parsed) return t('shortcut.unset');
  return [...catalogue.modifiers.filter(m => parsed.modifiers.includes(m.value)).map(m => m.label),
    optionLabel(parsed.key, catalogue.keys.find(k => k.value === parsed.key)!.label)].join(' + ');
}
