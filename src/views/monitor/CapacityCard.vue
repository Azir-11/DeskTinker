<script setup lang="ts">
import { formatCapacity, usedPercent } from "./shared";

const props = defineProps<{
  title: string;
  icon: string;
  accent: string;
  total?: number;
  available?: number;
  detailLabel?: string;
  detailValue?: string;
  emptyText: string;
  binary?: boolean;
}>();

const capacity = computed(() => {
  if (!props.total || props.available === undefined) return null;
  const available = Math.min(props.total, props.available);
  const format = (bytes: number) => formatCapacity(bytes, props.binary ?? false);
  return {
    total: format(props.total),
    used: format(props.total - available),
    available: format(available),
    usedPercent: usedPercent(props.total, available),
  };
});
</script>

<template>
  <article class="card min-w-0 overflow-hidden" :style="{ '--monitor-accent': accent }">
    <header class="flex items-center gap-9px px-18px pt-16px">
      <span class="text-21px text-[var(--monitor-accent)]" :class="icon" aria-hidden="true" />
      <h2 class="m-0 text-15px font-650">{{ title }}</h2>
    </header>

    <div v-if="capacity" class="flex items-center gap-14px px-18px py-17px min-[1120px]:gap-20px">
      <div
        class="relative grid h-102px w-102px shrink-0 place-items-center min-[1120px]:h-116px min-[1120px]:w-116px"
      >
        <div
          class="absolute inset-0 rounded-full mask-monitor-ring"
          :style="{
            background: `conic-gradient(var(--monitor-accent) ${capacity.usedPercent}%, var(--c-hairline-strong) 0)`,
          }"
          aria-hidden="true"
        />
        <div class="relative text-center leading-tight">
          <strong class="block font-mono text-14px font-650 min-[1120px]:text-15px">{{
            capacity.available
          }}</strong>
          <span class="text-11px text-fg-2">可用</span>
        </div>
      </div>

      <dl class="m-0 flex min-w-0 flex-1 flex-col gap-8px text-12px">
        <div class="flex flex-wrap items-baseline justify-between gap-x-6px">
          <dt class="inline-flex items-center gap-6px text-fg-2">
            <span class="h-6px w-6px rounded-full bg-[var(--monitor-accent)]" aria-hidden="true" />
            已用
          </dt>
          <dd class="m-0 whitespace-nowrap font-mono font-550">{{ capacity.used }}</dd>
        </div>
        <div
          class="flex flex-wrap items-baseline justify-between gap-x-6px border-t border-hairline pt-8px"
        >
          <dt class="text-fg-2">总容量</dt>
          <dd class="m-0 whitespace-nowrap font-mono font-550">{{ capacity.total }}</dd>
        </div>
      </dl>
    </div>
    <p
      v-else
      class="m-0 grid min-h-136px place-items-center px-18px text-center text-12px text-fg-2"
    >
      {{ emptyText }}
    </p>

    <footer
      v-if="detailLabel"
      class="flex min-h-38px items-center justify-between gap-8px border-t border-hairline px-18px py-7px text-12px"
    >
      <span class="text-fg-2">{{ detailLabel }}</span>
      <span class="truncate font-mono" :title="detailValue">{{ detailValue }}</span>
    </footer>
  </article>
</template>
