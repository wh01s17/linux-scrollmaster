// SPDX-License-Identifier: GPL-3.0-or-later
/// <reference types="vite/client" />
// Pre-rendered turntable of the original mouse model (art/mouse-render).
import data from './turntable.json';

export type Zone = 'pointer' | 'wheel' | 'thumb' | 'buttons';
export interface Anchor { x: number; y: number; facing: number }
export interface Turntable { frames: number; width: number; height: number; angles: Record<Zone, number>; anchors: Record<Zone, Anchor>[] }

const urls = (files: Record<string, unknown>) => Object.keys(files).sort().map((key) => files[key] as string);
export const FRAME_URLS = urls(import.meta.glob('./frames/f*.webp', { eager: true, import: 'default', query: '?url' }));
const lit = import.meta.glob('./frames/lit-*.webp', { eager: true, import: 'default', query: '?url' });
export const LIT_URLS = Object.fromEntries(Object.entries(lit).map(([key, url]) => [key.replace(/^.*lit-|\.webp$/g, ''), url as string])) as Record<Zone, string>;
export const TURNTABLE = data as Turntable;
