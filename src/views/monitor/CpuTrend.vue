<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import type { CpuSample } from "@/composables/useSystemStats";
import { useAppStore } from "@/store/app";
import { clampPercent, cpuRanges, cpuSegments } from "./shared";
import type { CpuRange } from "./shared";

type TimeRange = [start: number, end: number];

type ArchiveQuery =
  | { status: "loading" | "error"; bounds: TimeRange }
  | { status: "ready"; bounds: TimeRange; samples: readonly CpuSample[] };

const props = defineProps<{ samples: readonly CpuSample[]; error: boolean }>();

const app = useAppStore();
const canvas = useTemplateRef<HTMLCanvasElement>("canvas");
const range = shallowRef<CpuRange | "archive">(cpuRanges[0]);
const archiveDraft = shallowRef<TimeRange | null>(null);
const archive = shallowRef<ArchiveQuery | null>(null);
const viewingArchive = computed(() => range.value === "archive");
const archiveLoading = computed(() => archive.value?.status === "loading");
const invalidRange = computed(() => {
  const bounds = archiveDraft.value;
  return bounds !== null && (bounds[0] < 0 || bounds[0] >= bounds[1]);
});
const samples = computed(() => {
  if (!viewingArchive.value) return props.samples;
  return archive.value?.status === "ready" ? archive.value.samples : [];
});
const lastSample = computed(() => samples.value[samples.value.length - 1]);
const liveEnd = computed(() =>
  Math.max(Date.now(), props.samples[props.samples.length - 1]?.time ?? 0),
);
const bounds = computed<TimeRange | null>(() => {
  const selection = range.value;
  if (selection === "archive") return archive.value?.bounds ?? null;
  return [liveEnd.value - selection.duration, liveEnd.value];
});
const segments = computed(() => {
  const timeBounds = bounds.value;
  return timeBounds
    ? cpuSegments(samples.value, timeBounds[0], timeBounds[1], !viewingArchive.value)
    : [];
});
const dateTimeFormat = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  hourCycle: "h23",
});
const axisLabels = computed(() => {
  if (range.value !== "archive") return [`${range.value.label}前`, "现在"];
  return bounds.value?.map((time) => dateTimeFormat.format(time));
});
const emptyLabel = computed(() => {
  if (viewingArchive.value) {
    switch (archive.value?.status) {
      case "loading":
        return "正在读取 CPU 历史";
      case "error":
        return "暂时无法读取 CPU 历史";
      case "ready":
        return "此时间范围内没有 CPU 采样";
      default:
        return "选择开始和结束时间后查看记录";
    }
  }
  if (props.error) return "暂时无法读取 CPU 历史";
  return props.samples.length ? "此时间范围内没有 CPU 采样" : "正在等待 CPU 采样";
});
let observer: ResizeObserver | undefined;

function selectRange(value: CpuRange | "archive") {
  range.value = value;
}

async function loadArchive() {
  const requested = archiveDraft.value;
  if (!requested || invalidRange.value || archiveLoading.value) return;
  const [start, end] = requested;
  archive.value = { status: "loading", bounds: requested };
  try {
    const points = await invoke<readonly CpuSample[]>("read_cpu_history_range", { start, end });
    archive.value = { status: "ready", bounds: requested, samples: points };
  } catch {
    archive.value = { status: "error", bounds: requested };
  }
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
  const timeBounds = bounds.value;
  if (!timeBounds || !segments.value.length) return;
  const [start, endTime] = timeBounds;
  const duration = endTime - start;

  const color = getComputedStyle(target).getPropertyValue("--c-monitor-cpu").trim();
  const fill = ctx.createLinearGradient(0, 0, 0, height);
  fill.addColorStop(0, color);
  fill.addColorStop(1, "transparent");
  ctx.lineJoin = "round";
  ctx.lineWidth = 2;
  ctx.strokeStyle = color;

  for (const segment of segments.value) {
    const points = segment.map((sample) => ({
      x: 5 + ((sample.time - start) / duration) * (width - 10),
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
        <span v-if="!viewingArchive && error && segments.length" class="text-amber">
          · 更新失败
        </span>
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
        <button
          type="button"
          class="inline-flex items-center gap-4px rounded-[6px] border-none bg-transparent px-8px py-4px text-11px text-fg-2 cursor-pointer hover:text-fg"
          :class="viewingArchive && 'bg-accent-soft !text-[var(--c-monitor-cpu)]'"
          :aria-pressed="viewingArchive"
          aria-controls="cpu-history-date-range"
          @click="selectRange('archive')"
        >
          <span class="i-ph-calendar-blank text-12px" aria-hidden="true" />
          按日期查看
        </button>
      </div>
    </div>
    <form
      v-if="viewingArchive"
      id="cpu-history-date-range"
      class="col-span-2 flex flex-wrap items-center gap-8px"
      aria-label="CPU 历史查询"
      @submit.prevent="loadArchive"
    >
      <NDatePicker
        v-model:value="archiveDraft"
        type="datetimerange"
        size="small"
        format="yyyy-MM-dd HH:mm"
        :time-picker-props="{ format: 'HH:mm' }"
        :default-time="['00:00:00', '23:59:00']"
        start-placeholder="开始日期和时间"
        end-placeholder="结束日期和时间"
        placement="bottom-end"
        to="#overlay-host"
        clearable
        class="min-w-0 flex-1 basis-300px"
      />
      <NButton
        attr-type="submit"
        size="small"
        :disabled="!archiveDraft || invalidRange || archiveLoading"
      >
        查看记录
      </NButton>
      <p v-if="invalidRange" class="m-0 basis-full text-11px text-amber" role="alert">
        请选择有效日期，结束时间须晚于开始时间
      </p>
    </form>
    <div
      class="relative h-[var(--monitor-chart-h)] min-w-0 overflow-hidden rounded-sm border border-hairline"
    >
      <div class="absolute inset-0 bg-monitor-grid" aria-hidden="true" />
      <canvas ref="canvas" class="relative block h-full w-full" aria-hidden="true" />
      <p
        v-if="!segments.length"
        class="absolute inset-0 m-0 grid place-items-center text-12px text-fg-2"
        role="status"
      >
        {{ emptyLabel }}
      </p>
    </div>
    <div class="flex flex-col justify-between text-10px text-fg-2 font-mono" aria-hidden="true">
      <span>100%</span><span>50%</span><span>0%</span>
    </div>
    <figcaption v-if="axisLabels" class="flex justify-between gap-8px text-11px text-fg-2">
      <span>{{ axisLabels[0] }}</span
      ><span>{{ axisLabels[1] }}</span>
    </figcaption>
  </figure>
</template>
