<script setup lang="ts">
import { isWindows } from "@/composables/usePlatform";
import { useSystemStats } from "@/composables/useSystemStats";
import CapacityCard from "./CapacityCard.vue";
import CpuTrend from "./CpuTrend.vue";
import { clampPercent, CPU_HISTORY_MS, formatCapacity, usedPercent } from "./shared";
import type { CpuSample } from "./shared";

const { stats, host, disks, cpuTemp, error, diskStatus, refresh } = useSystemStats();
const history = shallowRef<readonly CpuSample[]>([]);
const refreshing = ref(false);

const cpuUsage = computed(() =>
  stats.value?.sampled_at ? clampPercent(stats.value.cpu_usage) : null,
);
const processorLabel = computed(() => host.value?.cpu_brand || "CPU");
const temperature = computed(() =>
  cpuTemp.value === null ? "不可用" : `${cpuTemp.value.toFixed(0)} °C`,
);
const uptime = computed(() => {
  if (!stats.value) return "—";
  const days = Math.floor(stats.value.uptime / 86400);
  const hours = Math.floor((stats.value.uptime % 86400) / 3600);
  const minutes = Math.floor((stats.value.uptime % 3600) / 60);
  if (days) return `${days} 天 ${hours} 小时`;
  return hours ? `${hours} 小时 ${minutes} 分` : `${minutes} 分`;
});
const updateTimeFormat = new Intl.DateTimeFormat("zh-CN", { timeStyle: "medium", hour12: false });
const statusLabel = computed(() => {
  const time = stats.value?.sampled_at;
  if (error.value)
    return time
      ? `更新失败 · 上次采样 ${updateTimeFormat.format(time)}`
      : "暂时无法读取 CPU 和内存";
  return time ? `已更新 ${updateTimeFormat.format(time)}` : "正在读取系统状态";
});

const swapLabel = computed(() =>
  stats.value?.mem_total ? formatCapacity(stats.value.swap_used, true) : undefined,
);
const volumes = computed(() => disks.value.filter((disk) => disk.total > 0));
const primaryDisk = computed(
  () => volumes.value.find((disk) => disk.mount === "/") ?? volumes.value[0],
);
const otherVolumes = computed(() =>
  volumes.value
    .filter((disk) => disk !== primaryDisk.value)
    .map((disk) => ({
      ...disk,
      usedPercent: usedPercent(disk.total, disk.available),
      usedLabel: formatCapacity(disk.used, false),
      availableLabel: formatCapacity(disk.available, false),
    })),
);
const diskMessage = computed(() => {
  if (diskStatus.value === "loading") return "正在读取存储容量";
  if (diskStatus.value === "error") return "暂时无法读取存储容量";
  return "未发现可读的存储卷";
});
const diskDetail = computed(() => {
  const disk = primaryDisk.value;
  if (!disk) return undefined;
  if (diskStatus.value === "error") return "更新失败，显示上次读数";
  return disk.name || disk.mount;
});

async function refreshMonitor() {
  refreshing.value = true;
  try {
    await refresh();
  } finally {
    refreshing.value = false;
  }
}

watch(stats, (sample) => {
  if (!sample?.sampled_at) return;
  const time = sample.sampled_at;
  const latest = history.value[history.value.length - 1];
  // 窗口恢复或系统时间跳变时重开曲线，避免在缺失的时间段上补画趋势。
  const recent =
    latest && time > latest.time && time - latest.time <= 6_000
      ? history.value.filter((point) => time - point.time <= CPU_HISTORY_MS)
      : [];
  history.value = [...recent, { time, usage: sample.cpu_usage }];
});
</script>

<template>
  <div class="flex flex-col gap-14px">
    <div class="flex items-center justify-between gap-12px">
      <div class="flex items-center gap-8px text-12px text-fg-2 tabular-nums">
        <span
          class="h-7px w-7px shrink-0 rounded-full"
          :class="
            error
              ? 'bg-amber'
              : stats?.sampled_at
                ? 'bg-accent shadow-[0_0_0_3px_var(--c-accent-soft)]'
                : 'bg-hairline-strong'
          "
          aria-hidden="true"
        />
        {{ statusLabel }}
      </div>
      <button
        class="ghost-btn shrink-0 disabled:cursor-default disabled:opacity-50"
        type="button"
        title="刷新系统指标"
        aria-label="刷新系统指标"
        :aria-busy="refreshing"
        :disabled="refreshing"
        @click="refreshMonitor"
      >
        <span
          class="i-ph-arrow-clockwise-bold text-14px"
          :class="{ 'animate-spin': refreshing }"
          aria-hidden="true"
        />
      </button>
    </div>

    <section class="card relative overflow-hidden" aria-labelledby="monitor-cpu-title">
      <div class="pointer-events-none absolute inset-0 bg-monitor-cpu" aria-hidden="true" />
      <div class="relative px-18px pt-16px">
        <header class="mb-14px flex items-center justify-between gap-14px">
          <div class="flex min-w-0 items-center gap-10px">
            <span
              class="grid h-36px w-36px shrink-0 place-items-center rounded-sm bg-accent-soft text-[var(--c-monitor-cpu)]"
              aria-hidden="true"
            >
              <span class="i-ph-cpu-duotone text-21px" />
            </span>
            <div class="min-w-0">
              <h2
                id="monitor-cpu-title"
                class="m-0 truncate text-15px font-650"
                :title="processorLabel"
              >
                {{ processorLabel }}
              </h2>
              <span class="text-11px text-fg-2">CPU 使用率</span>
            </div>
          </div>
          <strong class="shrink-0 font-mono text-28px font-650 leading-none tracking-tight">
            {{ cpuUsage === null ? "—" : cpuUsage.toFixed(1)
            }}<span v-if="cpuUsage !== null" class="ml-2px text-15px text-fg-2">%</span>
          </strong>
        </header>
        <CpuTrend :samples="history" />
      </div>

      <dl
        class="relative mb-0 mt-14px grid grid-cols-3 divide-x divide-hairline border-t border-hairline"
      >
        <div
          class="min-w-0 px-14px py-10px min-[1100px]:flex min-[1100px]:items-baseline min-[1100px]:justify-between min-[1100px]:gap-8px"
        >
          <dt class="flex items-center gap-5px text-12px text-fg-2">
            <span class="i-ph-thermometer-simple-duotone text-15px" aria-hidden="true" />
            {{ isWindows ? "系统温度" : "CPU 温度" }}
          </dt>
          <dd class="m-0 mt-2px font-mono font-550 min-[1100px]:mt-0">{{ temperature }}</dd>
        </div>
        <div
          class="min-w-0 px-14px py-10px min-[1100px]:flex min-[1100px]:items-baseline min-[1100px]:justify-between min-[1100px]:gap-8px"
        >
          <dt class="flex items-center gap-5px text-12px text-fg-2">
            <span class="i-ph-stack-duotone text-15px" aria-hidden="true" />逻辑核心
          </dt>
          <dd class="m-0 mt-2px font-mono font-550 min-[1100px]:mt-0">
            {{ host?.core_count || stats?.per_core.length || "—" }}
          </dd>
        </div>
        <div
          class="min-w-0 px-14px py-10px min-[1100px]:flex min-[1100px]:items-baseline min-[1100px]:justify-between min-[1100px]:gap-8px"
        >
          <dt class="flex items-center gap-5px text-12px text-fg-2">
            <span class="i-ph-timer-duotone text-15px" aria-hidden="true" />运行时间
          </dt>
          <dd class="m-0 mt-2px font-mono font-550 min-[1100px]:mt-0">{{ uptime }}</dd>
        </div>
      </dl>
    </section>

    <div class="grid grid-cols-[repeat(auto-fit,minmax(min(100%,260px),1fr))] gap-14px">
      <CapacityCard
        title="内存"
        icon="i-ph-memory-duotone"
        accent="var(--c-monitor-memory)"
        :total="stats?.mem_total"
        :available="stats?.mem_available"
        :detail-label="swapLabel ? '已用交换空间' : undefined"
        :detail-value="swapLabel"
        :empty-text="error ? '暂时无法读取内存容量' : '正在读取内存容量'"
        binary
      />
      <CapacityCard
        title="存储空间"
        icon="i-ph-hard-drives-duotone"
        accent="var(--c-monitor-storage)"
        :total="primaryDisk?.total"
        :available="primaryDisk?.available"
        :detail-label="diskDetail ? '挂载卷' : undefined"
        :detail-value="diskDetail"
        :empty-text="diskMessage"
      />
    </div>

    <section
      v-if="otherVolumes.length"
      class="card px-18px py-14px"
      aria-labelledby="monitor-volumes-title"
    >
      <h2 id="monitor-volumes-title" class="m-0 text-15px font-650">其他存储卷</h2>
      <ul class="m-0 mt-8px list-none divide-y divide-hairline p-0">
        <li
          v-for="disk in otherVolumes"
          :key="disk.mount"
          class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-14px gap-y-6px py-10px"
        >
          <div class="flex min-w-0 items-center gap-7px">
            <span
              class="i-ph-hard-drive-duotone text-16px text-[var(--c-monitor-storage)]"
              aria-hidden="true"
            />
            <span class="truncate font-550" :title="disk.mount">{{ disk.name || disk.mount }}</span>
            <span
              v-if="disk.name && disk.name !== disk.mount"
              class="truncate text-11px text-fg-2"
              :title="disk.mount"
              >{{ disk.mount }}</span
            >
          </div>
          <span class="whitespace-nowrap text-11px text-fg-2 font-mono"
            >已用 {{ disk.usedLabel }} · 可用 {{ disk.availableLabel }}</span
          >
          <div class="col-span-2 h-4px overflow-hidden rounded-full bg-hairline" aria-hidden="true">
            <span
              class="block h-full rounded-full bg-[var(--c-monitor-storage)]"
              :style="{ width: `${disk.usedPercent}%` }"
            />
          </div>
        </li>
      </ul>
    </section>
  </div>
</template>
