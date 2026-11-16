<script setup lang="ts">
import { useAppStore } from "@/store/app";
import { clampPercent, CPU_HISTORY_MS } from "./shared";
import type { CpuSample } from "./shared";

const props = defineProps<{ samples: readonly CpuSample[] }>();

const app = useAppStore();
const canvas = shallowRef<HTMLCanvasElement | null>(null);
let observer: ResizeObserver | undefined;

function draw() {
  const target = canvas.value;
  const latest = props.samples[props.samples.length - 1];
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
  if (!latest) return;

  const endX = width - 5;
  const points = props.samples.map((sample) => ({
    x: endX - ((latest.time - sample.time) / CPU_HISTORY_MS) * endX,
    y: 5 + (1 - clampPercent(sample.usage) / 100) * (height - 10),
  }));
  const [first] = points;
  const last = points[points.length - 1];
  const color = getComputedStyle(target).getPropertyValue("--c-monitor-cpu").trim();

  const line = new Path2D();
  line.moveTo(first.x, first.y);
  points.slice(1).forEach(({ x, y }) => line.lineTo(x, y));
  const area = new Path2D(line);
  area.lineTo(last.x, height);
  area.lineTo(first.x, height);
  area.closePath();

  const fill = ctx.createLinearGradient(0, 0, 0, height);
  fill.addColorStop(0, color);
  fill.addColorStop(1, "transparent");
  ctx.fillStyle = fill;
  ctx.globalAlpha = 0.28;
  ctx.fill(area);
  ctx.globalAlpha = 1;

  ctx.lineJoin = "round";
  ctx.lineWidth = 2;
  ctx.strokeStyle = color;
  ctx.stroke(line);

  ctx.beginPath();
  ctx.arc(last.x, last.y, 3.5, 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();
}

watch([() => props.samples, () => app.isDark], draw, { flush: "post" });

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
    <div
      class="relative h-[var(--monitor-chart-h)] min-w-0 overflow-hidden rounded-sm border border-hairline"
    >
      <div class="absolute inset-0 bg-monitor-grid" aria-hidden="true" />
      <canvas ref="canvas" class="relative block h-full w-full" aria-hidden="true" />
      <p
        v-if="!samples.length"
        class="absolute inset-0 m-0 grid place-items-center text-12px text-fg-2"
      >
        正在等待 CPU 采样
      </p>
    </div>
    <div class="flex flex-col justify-between text-10px text-fg-2 font-mono" aria-hidden="true">
      <span>100%</span><span>50%</span><span>0%</span>
    </div>
    <figcaption class="flex justify-between text-11px text-fg-2">
      <span>60 秒前</span><span>最新采样</span>
    </figcaption>
  </figure>
</template>
