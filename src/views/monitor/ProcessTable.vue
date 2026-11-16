<script setup lang="ts">
import type { ProcessStats } from "@/composables/useSystemStats";
import { formatCapacity, rankProcesses } from "./shared";
import type { ProcessSort } from "./shared";

const props = defineProps<{
  processes: readonly ProcessStats[] | null;
  error: boolean;
}>();

const metrics = [
  { key: "cpu_usage", label: "CPU" },
  { key: "memory", label: "内存" },
] as const;
const pageSize = 10;
const sort = ref<ProcessSort>("cpu_usage");
const query = ref("");
const page = ref(1);

const matches = computed(() => rankProcesses(props.processes ?? [], query.value, sort.value));
const pageCount = computed(() => Math.max(1, Math.ceil(matches.value.length / pageSize)));
const rows = computed(() =>
  matches.value.slice((page.value - 1) * pageSize, page.value * pageSize),
);
const countLabel = computed(() => {
  if (!props.processes) return "";
  return query.value.trim()
    ? `${matches.value.length} / ${props.processes.length} 个进程`
    : `${props.processes.length} 个进程`;
});
const emptyLabel = computed(() => {
  if (props.processes === null) return props.error ? "暂时无法读取进程" : "正在读取进程";
  return props.processes.length === 0 ? "未发现可读取的进程" : "没有匹配的进程";
});

function updateQuery(value: string) {
  query.value = value;
  page.value = 1;
}

function sortBy(value: ProcessSort) {
  sort.value = value;
  page.value = 1;
}

function movePage(offset: number) {
  page.value = Math.min(pageCount.value, Math.max(1, page.value + offset));
}

function cpuLabel(value: number | null): string {
  if (value === null) return "—";
  if (value > 0 && value < 0.1) return "<0.1%";
  return `${value.toFixed(1)}%`;
}

watch(pageCount, (count) => {
  page.value = Math.min(page.value, count);
});
</script>

<template>
  <div>
    <header class="flex flex-wrap items-center justify-between gap-10px px-18px py-14px">
      <div class="flex min-w-0 items-center gap-10px">
        <span
          class="grid h-34px w-34px shrink-0 place-items-center rounded-sm bg-elevated-2 text-fg"
          aria-hidden="true"
        >
          <span class="i-ph-list-bullets-bold text-17px" />
        </span>
        <div>
          <h2 id="monitor-processes-title" class="m-0 text-15px font-650">进程占用</h2>
          <div v-if="countLabel" class="text-11px text-fg-2">
            {{ countLabel }}
            <span v-if="error" class="text-amber"> · 更新失败，显示上次读数</span>
          </div>
        </div>
      </div>
      <NInput
        class="!w-200px max-w-full"
        :value="query"
        :input-props="{ 'aria-label': '搜索进程名或 PID' }"
        placeholder="搜索进程名或 PID"
        size="small"
        clearable
        @update:value="updateQuery"
      >
        <template #prefix>
          <span class="i-ph-magnifying-glass-bold text-13px" aria-hidden="true" />
        </template>
      </NInput>
    </header>

    <table
      class="w-full table-fixed border-collapse border-y border-hairline text-12px"
      aria-labelledby="monitor-processes-title"
      aria-describedby="monitor-processes-units"
    >
      <thead class="bg-elevated text-fg-2">
        <tr class="border-b border-hairline">
          <th scope="col" class="py-8px pl-18px text-left font-550">进程名</th>
          <th scope="col" class="w-64px py-8px text-right font-550">PID</th>
          <th
            v-for="metric in metrics"
            :key="metric.key"
            scope="col"
            class="w-104px py-4px pr-14px text-right font-550"
            :aria-sort="sort === metric.key ? 'descending' : undefined"
          >
            <button
              type="button"
              class="inline-flex items-center gap-4px rounded-sm border-none bg-transparent px-4px py-4px text-inherit text-12px font-550 cursor-pointer hover:bg-elevated-2 hover:text-fg"
              :class="sort === metric.key && 'bg-elevated-2 !text-fg'"
              @click="sortBy(metric.key)"
            >
              {{ metric.label }}
              <span
                class="i-ph-arrow-down-bold text-11px"
                :class="sort === metric.key ? 'opacity-100' : 'opacity-45'"
                aria-hidden="true"
              />
            </button>
          </th>
        </tr>
      </thead>
      <tbody v-if="rows.length" class="divide-y divide-hairline">
        <tr v-for="row in rows" :key="row.pid" class="hover:bg-elevated">
          <th
            scope="row"
            class="truncate py-7px pl-18px pr-8px text-left font-550"
            :title="row.name"
          >
            {{ row.name }}
          </th>
          <td class="text-right font-mono text-fg-2">{{ row.pid }}</td>
          <td
            class="pr-18px text-right font-mono text-[var(--c-monitor-cpu)]"
            :title="row.cpu_usage === null ? '等待 CPU 采样' : undefined"
          >
            {{ cpuLabel(row.cpu_usage) }}
          </td>
          <td class="pr-18px text-right font-mono text-[var(--c-monitor-memory)]">
            {{ formatCapacity(row.memory, true) }}
          </td>
        </tr>
      </tbody>
      <tbody v-else>
        <tr>
          <td colspan="4" class="px-18px py-20px text-center text-fg-2">{{ emptyLabel }}</td>
        </tr>
      </tbody>
    </table>

    <footer class="flex flex-wrap items-center justify-between gap-8px px-18px py-10px">
      <p id="monitor-processes-units" class="m-0 text-11px text-fg-2">
        CPU 按整机总算力计 · 内存为驻留物理内存
      </p>
      <nav
        v-if="pageCount > 1"
        class="flex items-center gap-8px font-mono text-11px text-fg-2"
        aria-label="进程列表分页"
      >
        <button
          type="button"
          class="ghost-btn h-26px w-26px disabled:cursor-default disabled:opacity-40"
          aria-label="上一页"
          :disabled="page === 1"
          @click="movePage(-1)"
        >
          <span class="i-ph-caret-left-bold" aria-hidden="true" />
        </button>
        {{ page }} / {{ pageCount }}
        <button
          type="button"
          class="ghost-btn h-26px w-26px disabled:cursor-default disabled:opacity-40"
          aria-label="下一页"
          :disabled="page === pageCount"
          @click="movePage(1)"
        >
          <span class="i-ph-caret-right-bold" aria-hidden="true" />
        </button>
      </nav>
    </footer>
  </div>
</template>
