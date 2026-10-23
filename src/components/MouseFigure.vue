<script setup lang="ts">
// 3D mouse as a pre-rendered turntable (art/mouse-render, CC BY-NC-SA frames). Switching
// tabs scrubs the frames the short way round to the angle that shows the part,
// then fades in the frame with that part lit in the app accent.
import { onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { FRAME_URLS, LIT_URLS, TURNTABLE, type Zone } from '../mouse3d/turntable.ts';
import { anchorAt, frameIndex, shortestTarget } from '../mouse3d/turnMath.ts';
import { t } from '../i18n';

const props = defineProps<{ zone: string }>();
const emit = defineEmits<{ select: [zone: Zone] }>();

const ORDER: { id: Zone }[] = [{ id: 'pointer' }, { id: 'wheel' }, { id: 'thumb' }, { id: 'buttons' }];
const N = TURNTABLE.frames;
const goalOf = (zone: Zone) => (TURNTABLE.angles[zone] / 360) * N;
const zoneOf = (value: string): Zone => (value in TURNTABLE.angles ? value as Zone : 'pointer');

const current = ref(frameIndex(goalOf(zoneOf(props.zone)), N));
const litZone = ref<Zone | null>(zoneOf(props.zone));
type Spot = { left: string; top: string; visibility: 'visible' | 'hidden' };
const spots = reactive({} as Record<Zone, Spot>);
let position = goalOf(zoneOf(props.zone));
let target = position;
let frame = 0;
let last = 0;
const still = matchMedia('(prefers-reduced-motion: reduce)');
// Decode every frame up front so scrubbing never waits for the disk.
const preload = FRAME_URLS.map((url) => Object.assign(new Image(), { src: url, decoding: 'async' as const }));

function place() {
  current.value = frameIndex(position, N);
  for (const { id } of ORDER) {
    const a = anchorAt(TURNTABLE.anchors, position, id);
    spots[id] = { left: `${a.x * 100}%`, top: `${a.y * 100}%`, visibility: a.visible ? 'visible' : 'hidden' };
  }
}
function tick(now: number) {
  const dt = last ? Math.min(0.05, (now - last) / 1000) : 0;
  last = now;
  // Exponential follow, time constant 0.09 s: about 0.3 s to arrive.
  position += (target - position) * (1 - Math.exp(-dt / 0.09));
  if (Math.abs(target - position) < 0.02) position = target;
  place();
  frame = position === target ? 0 : requestAnimationFrame(tick);
  if (!frame) litZone.value = zoneOf(props.zone);
}
function turnTo(zone: Zone) {
  litZone.value = null;
  target = shortestTarget(position, goalOf(zone), N);
  if (still.matches) {
    position = target;
    place();
    litZone.value = zone;
    return;
  }
  if (!frame) {
    last = 0;
    frame = requestAnimationFrame(tick);
  }
}

onMounted(place);
onBeforeUnmount(() => { cancelAnimationFrame(frame); preload.length = 0; });
watch(() => props.zone, (zone) => turnTo(zoneOf(zone)));
</script>

<template>
  <div class="mouse-figure" role="img" :aria-label="t('figure.aria')">
    <img class="frame" :src="FRAME_URLS[current]" alt="" draggable="false">
    <img v-for="spot in ORDER" :key="spot.id" class="frame lit" :class="{ on: litZone === spot.id }" :src="LIT_URLS[spot.id]" alt="" draggable="false">
    <button v-for="(spot, index) in ORDER" :key="spot.id" class="hotspot" :class="{ active: zone === spot.id }" :style="spots[spot.id]" :aria-label="t(`zone.${spot.id}.aria`)" @click="emit('select', spot.id)">{{ index + 1 }}</button>
  </div>
</template>

<style scoped>
.mouse-figure { position: relative; width: 100%; max-width: 460px; margin: 0 auto; aspect-ratio: 840 / 680; }
.frame { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
/* Fade in only: a lit still fading out over turning frames would ghost. */
.lit { opacity: 0; }
.lit.on { opacity: 1; transition: opacity .25s ease; }
.mouse-figure .hotspot { transform: translate(-50%, -50%); }
@media (prefers-reduced-motion: reduce) { .lit { transition: none; } }
</style>
