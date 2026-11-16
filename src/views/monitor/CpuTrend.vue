<script setup lang="ts">
import type { CpuSample } from "@/composables/useSystemStats";
import { useAppStore } from "@/store/app";
import { clampPercent, cpuRanges, cpuSegments } from "./shared";
import type { CpuRange } from "./shared";

const props = defineProps<{ samples: readonly CpuSample[]; error: boolean }>();

const app = useAppStore();
const canvas = useTemplateRef<HTMLCanvasElement>("canvas");
const range = shallowRef<CpuRange>(cpuRanges[0]);
const lastSample = computed(() => props.samples[props.samples.length - 1]);
const windowEnd = computed(() => Math.max(Date.now(), lastSample.value?.time ?? 0));
const windowStart = computed(() => windowEnd.value - range.value.duration);
const segments = computed(() => cpuSegments(props.samples, windowStart.value));
const emptyLabel = computed(() => {
  if (props.error) return "暂时无法读取 CPU 历史";
  return props.samples.length ? "此时间范围内没有 CPU 采样" : "正在等待 CPU 采样";
});
let observer: ResizeObserver | undefined;

function selectRange(value: CpuRange) {
  range.value = value;
}

function draw() {
  const target = canvas.value;
  const ctx = target?.getContext("2d");
  if (!target || !ctx) return;

  const { width, height } = target.getBoundingClientRect();
  if (!width || !height) return;
  const scale = window.devicePixelRatio;
  const deviceWidth = Math.round(width * scale);
  const deviceHeight = Math.round(height * scale);
  if (target.width !== deviceWidth) target.width = deviceWidth;
  if (target.height !== deviceHeight) target.height = deviceHeight;
  ctx.setTransform(scale, 0, 0, scale, 0, 0);
  ctx.clearRect(0, 0, width, height);
  if (!segments.value.length) return;

  const color = getComputedStyle(target).getPropertyValue("--c-monitor-cpu").trim();
  const fill = ctx.createLinearGradient(0, 0, 0, height);
  fill.addColorStop(0, color);
  fill.addColorStop(1, "transparent");
  ctx.lineJoin = "round";
  ctx.lineWidth = 2;
  ctx.strokeStyle = color;

  for (const segment of segments.value) {
    const points = segment.map((sample) => ({
      x: 5 + ((sample.time - windowStart.value) / range.value.duration) * (width - 10),
      y: 5 + (1 - clampPercent(sample.usage) / 100) * (height - 10),
    }));
    const [first] = points;
    const end = points[points.length - 1];
    if (points.length > 1) {
      const line = new Path2D();
      line.moveTo(first.x, first.y);
      for (const { x, y } of points.slice(1)) line.lineTo(x, y);
      const area = new Path2D(line);
      area.lineTo(end.x, height);
      area.lineTo(first.x, height);
      area.closePath();

      ctx.fillStyle = fill;
      ctx.globalAlpha = 0.28;
      ctx.fill(area);
      ctx.globalAlpha = 1;
      ctx.stroke(line);
    }

    const isLatest = segment[segment.length - 1] === lastSample.value;
    if (isLatest || points.length === 1) {
      ctx.beginPath();
      ctx.arc(end.x, end.y, isLatest ? 3.5 : 2, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();
    }
  }
}

watch([segments, () => app.isDark], draw, { flush: "post" });

onMounted(() => {
  if (!canvas.value) return;
  observer = new ResizeObserver(draw);
  observer.observe(canvas.value);
  draw();
});

onUnmounted(() => observer?.disconnect());
</script>

<template>
  <figure class="m-0 grid grid-cols-[1fr_auto] gap-x-10px gap-y-7px">
    <div class="col-span-2 flex flex-wrap items-center justify-between gap-8px">
      <div class="text-12px text-fg-2">
        CPU 历史
        <span v-if="error && segments.length" class="text-amber"> · 更新失败</span>
      </div>
      <div
        class="inline-flex gap-2px rounded-sm border border-hairline bg-elevated p-2px"
        role="group"
        aria-label="CPU 历史范围"
      >
        <button
          v-for="option in cpuRanges"
          :key="option.duration"
          type="button"
          class="rounded-[6px] border-none bg-transparent px-8px py-4px text-11px text-fg-2 cursor-pointer hover:text-fg"
          :class="range === option && 'bg-accent-soft !text-[var(--c-monitor-cpu)]'"
          :aria-pressed="range === option"
          @click="selectRange(option)"
        >
          {{ option.label }}
        </button>
      </div>
    </div>
    <div
      class="relative h-[var(--monitor-chart-h)] min-w-0 overflow-hidden rounded-sm border border-hairline"
    >
      <div class="absolute inset-0 bg-monitor-grid" aria-hidden="true" />
      <canvas ref="canvas" class="relative block h-full w-full" aria-hidden="true" />
      <p
        v-if="!segments.length"
        class="absolute inset-0 m-0 grid place-items-center text-12px text-fg-2"
      >
        {{ emptyLabel }}
      </p>
    </div>
    <div class="flex flex-col justify-between text-10px text-fg-2 font-mono" aria-hidden="true">
      <span>100%</span><span>50%</span><span>0%</span>
    </div>
    <figcaption class="flex justify-between text-11px text-fg-2">
      <span>{{ range.label }}前</span><span>现在</span>
    </figcaption>
  </figure>
</template>
