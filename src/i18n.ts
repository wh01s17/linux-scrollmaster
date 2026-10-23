// SPDX-License-Identifier: GPL-3.0-or-later
// Interface language. Spanish is the source text; English is a full translation.
// Labels and messages produced by the Rust backend are Spanish: they are mapped
// here by stable value (button ids, action values, key codes) or by exact text.
import { ref } from 'vue';
import { es, type Messages } from './locales/es.ts';
import { en, backendEn } from './locales/en.ts';

export type Locale = 'es' | 'en';
export const LOCALES: { id: Locale; label: string }[] = [{ id: 'es', label: 'Español' }, { id: 'en', label: 'English' }];
const STORAGE = 'linux-scrollmaster-locale';
const catalogues: Record<Locale, Messages> = { es, en };

function stored(): Locale | null {
  try {
    const value = globalThis.localStorage?.getItem(STORAGE);
    return value === 'es' || value === 'en' ? value : null;
  } catch { return null; }
}
/** Saved choice, else the system language (Spanish for es-*, English otherwise). */
export function detectLocale(): Locale {
  const saved = stored();
  if (saved) return saved;
  const languages = globalThis.navigator?.languages ?? [globalThis.navigator?.language ?? ''];
  return languages.some(l => typeof l === 'string' && l.toLowerCase().startsWith('es')) ? 'es' : 'en';
}

export const locale = ref<Locale>(detectLocale());
export function setLocale(next: Locale) {
  locale.value = next;
  try { globalThis.localStorage?.setItem(STORAGE, next); } catch { /* per-viewer convenience only */ }
  if (globalThis.document?.documentElement) globalThis.document.documentElement.lang = next;
}

export type MessageKey = keyof Messages;
/** Translated text; `{name}` placeholders take values from `params`. */
export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  const text = catalogues[locale.value][key] ?? es[key];
  return text.replace(/\{(\w+)\}/g, (_, name) => String(params[name] ?? `{${name}}`));
}

/** Backend text (Spanish) in the current language: exact matches and known patterns. */
export function tr(text: string | null | undefined): string {
  if (!text) return '';
  if (locale.value === 'es') return text;
  const exact = backendEn.exact[text];
  if (exact) return exact;
  for (const [pattern, replacement] of backendEn.patterns) {
    if (pattern.test(text)) return text.replace(pattern, replacement);
  }
  return text;
}

/** Label of a backend option by its stable value, falling back to the backend label. */
export function optionLabel(value: unknown, fallback: string): string {
  if (locale.value === 'es') return fallback;
  const key = String(value);
  return backendEn.options[key] ?? tr(fallback);
}

export function numberFormat(value: number): string {
  return value.toLocaleString(locale.value);
}
