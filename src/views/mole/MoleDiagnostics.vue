<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { formatCapacity } from "@/views/monitor/shared";
import {
  clockTime,
  memoryPressure,
  percent,
  processMemory,
  rate,
  rounded,
  watts,
  type MoleProcess,
  type MoleSnapshot,
} from "./diagnostics";

const snapshot = shallowRef<MoleSnapshot>();
const loading = ref(false);
const error = ref("");
const order = ref<"cpu" | "memory">("cpu");

const capturedAt = computed(() => clockTime(snapshot.value?.collected_at));
const processesAt = computed(() => clockTime(snapshot.value?.process_collected_at));
const processes = computed(() => {
  const state = snapshot.value;
  const memoryTotal = state?.memory?.total ?? 100;
  const value =
    order.value === "cpu"
      ? (row: MoleProcess) => row.cpu ?? 0
      : (row: MoleProcess) => row.memory_bytes ?? ((row.memory ?? 0) * memoryTotal) / 100;
  return [...(state?.top_processes ?? [])].sort((left, right) => value(right) - value(left));
});
const drive = computed(() => snapshot.value?.disks?.find(({ mount }) => mount === "/"));

async function refresh() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    snapshot.value = await invoke<MoleSnapshot>("mole_read_status");
  } catch (cause) {
    error.value = String(cause);
  } finally {
    loading.value = false;
  }
}

function sortCpu() {
  order.value = "cpu";
}

function sortMemory() {
  order.value = "memory";
}

onMounted(refresh);
</script>

<template>
  <div class="space-y-14px" :aria-busy="loading">
    <header class="flex items-center justify-between gap-12px">
      <div class="min-w-0">
        <p v-if="snapshot?.hardware?.model" class="truncate text-15px font-650">
          {{ snapshot.hardware.model }}
        </p>
        <p class="text-12px text-fg-2" role="status">
          {{ loading ? "正在采样系统状态…" : capturedAt ? `采样于 ${capturedAt}` : "运行诊断" }}
        </p>
      </div>
      <NButton size="small" :loading="loading" @click="refresh">
        <template #icon><span class="i-ph-arrow-clockwise-bold" aria-hidden="true" /></template>
        刷新诊断
      </NButton>
    </header>

    <NAlert v-if="error" type="error" title="诊断读取失败" class="break-words">
      {{ error }}
    </NAlert>
    <div v-if="loading && !snapshot" class="card px-16px py-24px text-13px text-fg-2">
      正在采集 GPU、硬件和进程数据。
    </div>

    <template v-if="snapshot">
      <div class="grid grid-cols-[repeat(auto-fit,minmax(min(100%,244px),1fr))] gap-14px">
        <section class="card min-w-0 p-16px" aria-labelledby="mole-gpu-title">
          <h3 id="mole-gpu-title" class="mb-10px flex items-center gap-7px text-14px font-650">
            <span class="i-ph-graphics-card-duotone text-18px text-violet" aria-hidden="true" />
            GPU 与温度
          </h3>
          <ul v-if="snapshot.gpu?.length" class="mb-8px flex flex-col gap-6px">
            <li v-for="(gpu, index) in snapshot.gpu" :key="index" class="text-12px">
              <div class="flex items-center justify-between gap-10px">
                <strong class="truncate font-600" :title="gpu.name">{{ gpu.name }}</strong>
                <span class="shrink-0 font-mono">{{ percent(gpu.usage) }}</span>
              </div>
              <p v-if="gpu.core_count || gpu.memory_total" class="mt-1px text-fg-2">
                <span v-if="gpu.core_count">{{ gpu.core_count }} 核</span>
                <span v-if="gpu.core_count && gpu.memory_total"> · </span>
                <span v-if="gpu.memory_total">
                  显存
                  {{ gpu.memory_used === undefined ? "—" : formatCapacity(gpu.memory_used, true) }}
                  /
                  {{ formatCapacity(gpu.memory_total, true) }}
                </span>
              </p>
            </li>
          </ul>
          <p v-else class="mb-8px text-12px text-fg-2">暂无 GPU 读数。</p>
          <dl
            class="grid grid-cols-[1fr_auto] gap-x-10px gap-y-5px border-t border-hairline pt-8px text-12px"
          >
            <dt class="text-fg-2">GPU 温度</dt>
            <dd class="text-right font-mono">
              {{ rounded(snapshot.thermal?.gpu_temp, " °C", 1) }}
            </dd>
            <dt class="text-fg-2">CPU 温度</dt>
            <dd class="text-right font-mono">
              {{ rounded(snapshot.thermal?.cpu_temp, " °C", 1) }}
            </dd>
          </dl>
        </section>

        <section class="card min-w-0 p-16px" aria-labelledby="mole-power-title">
          <h3 id="mole-power-title" class="mb-10px flex items-center gap-7px text-14px font-650">
            <span class="i-ph-lightning-duotone text-18px text-amber" aria-hidden="true" />
            电源与风扇
          </h3>
          <ul v-if="snapshot.batteries?.length" class="mb-8px flex flex-col gap-6px">
            <li v-for="(battery, index) in snapshot.batteries" :key="index" class="text-12px">
              <div class="flex justify-between gap-10px">
                <strong class="font-600"
                  >电池 {{ snapshot.batteries.length > 1 ? index + 1 : "" }}</strong
                >
                <span class="font-mono">{{ percent(battery.percent) }}</span>
              </div>
              <p class="mt-1px flex flex-wrap gap-x-8px text-fg-2">
                <span v-if="battery.status">{{ battery.status }}</span>
                <span v-if="battery.time_left">剩余 {{ battery.time_left }}</span>
                <span v-if="battery.capacity">健康 {{ rounded(battery.capacity, "%", 1) }}</span>
                <span v-else-if="battery.health">健康 {{ battery.health }}</span>
                <span v-if="battery.cycle_count !== undefined"
                  >循环 {{ battery.cycle_count }} 次</span
                >
              </p>
            </li>
          </ul>
          <p v-else class="mb-8px text-12px text-fg-2">暂无电池数据。</p>
          <dl
            class="grid grid-cols-[1fr_auto] gap-x-10px gap-y-5px border-t border-hairline pt-8px text-12px"
          >
            <dt class="text-fg-2">系统功耗</dt>
            <dd class="text-right font-mono">{{ watts(snapshot.thermal?.system_power) }}</dd>
            <dt class="text-fg-2">风扇转速</dt>
            <dd class="text-right font-mono">
              {{
                rounded(snapshot.thermal?.fan_speed, " RPM", snapshot.thermal?.fan_count ? 0 : 1)
              }}
            </dd>
            <dt v-if="snapshot.batteries?.length" class="text-fg-2">电池温度</dt>
            <dd v-if="snapshot.batteries?.length" class="text-right font-mono">
              {{ rounded(snapshot.thermal?.battery_temp, " °C", 1) }}
            </dd>
          </dl>
        </section>

        <section class="card min-w-0 p-16px" aria-labelledby="mole-disk-title">
          <h3 id="mole-disk-title" class="mb-10px flex items-center gap-7px text-14px font-650">
            <span class="i-ph-hard-drives-duotone text-18px text-accent" aria-hidden="true" />
            磁盘与内存
          </h3>
          <dl class="grid grid-cols-[1fr_auto] gap-x-10px gap-y-6px text-12px">
            <dt class="text-fg-2">磁盘读取</dt>
            <dd class="text-right font-mono">{{ rate(snapshot.disk_io?.read_rate) }}</dd>
            <dt class="text-fg-2">磁盘写入</dt>
            <dd class="text-right font-mono">{{ rate(snapshot.disk_io?.write_rate) }}</dd>
            <dt class="text-fg-2">启动卷 SMART 状态</dt>
            <dd class="text-right">{{ drive?.smart_status || "—" }}</dd>
            <dt class="text-fg-2">内存压力</dt>
            <dd class="text-right">{{ memoryPressure(snapshot.memory?.pressure) }}</dd>
          </dl>
        </section>

        <section class="card min-w-0 p-16px" aria-labelledby="mole-network-title">
          <h3 id="mole-network-title" class="mb-10px flex items-center gap-7px text-14px font-650">
            <span class="i-ph-network-duotone text-18px text-info" aria-hidden="true" />
            网络流量
          </h3>
          <table v-if="snapshot.network?.length" class="w-full table-fixed text-12px">
            <thead class="text-fg-2">
              <tr>
                <th scope="col" class="text-left font-400">接口</th>
                <th scope="col" class="w-[37%] text-right font-400">下载</th>
                <th scope="col" class="w-[37%] text-right font-400">上传</th>
              </tr>
            </thead>
            <tbody class="font-mono">
              <tr v-for="network in snapshot.network" :key="network.name">
                <th scope="row" class="truncate py-2px text-left font-500" :title="network.name">
                  {{ network.name }}
                </th>
                <td class="whitespace-nowrap text-right">{{ rate(network.rx_rate_mbs) }}</td>
                <td class="whitespace-nowrap text-right">{{ rate(network.tx_rate_mbs) }}</td>
              </tr>
            </tbody>
          </table>
          <p v-else class="text-12px text-fg-2">暂无网络接口读数。</p>
        </section>
      </div>

      <section class="card min-w-0 p-16px" aria-labelledby="mole-process-title">
        <header class="mb-8px flex flex-wrap items-center justify-between gap-6px">
          <h3 id="mole-process-title" class="flex items-center gap-7px text-14px font-650">
            <span class="i-ph-list-bullets-duotone text-18px text-accent" aria-hidden="true" />
            进程排行
          </h3>
          <p v-if="snapshot.process_stale || processesAt" class="text-12px text-fg-2">
            {{ snapshot.process_stale ? "上次进程采样" : "进程采样" }} {{ processesAt }}
          </p>
        </header>
        <table v-if="processes.length" class="w-full table-fixed text-12px">
          <thead class="text-fg-2">
            <tr>
              <th scope="col" class="text-left font-500">进程</th>
              <th scope="col" class="w-64px text-right font-500">PID</th>
              <th
                scope="col"
                class="w-72px text-right font-500"
                :aria-sort="order === 'cpu' ? 'descending' : 'none'"
              >
                <button type="button" class="hover:text-fg" @click="sortCpu">
                  CPU <span v-if="order === 'cpu'" aria-hidden="true">↓</span>
                </button>
              </th>
              <th
                scope="col"
                class="w-94px text-right font-500"
                :aria-sort="order === 'memory' ? 'descending' : 'none'"
              >
                <button type="button" class="hover:text-fg" @click="sortMemory">
                  内存 <span v-if="order === 'memory'" aria-hidden="true">↓</span>
                </button>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="process in processes" :key="process.pid" class="border-t border-hairline">
              <th
                scope="row"
                class="truncate py-5px pr-8px text-left font-500"
                :title="process.name"
              >
                {{ process.name || "未知进程" }}
              </th>
              <td class="text-right font-mono text-fg-2">{{ process.pid }}</td>
              <td class="text-right font-mono">{{ percent(process.cpu) }}</td>
              <td class="whitespace-nowrap text-right font-mono">{{ processMemory(process) }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="text-12px text-fg-2">暂无进程采样。</p>
        <div class="mt-10px border-t border-hairline pt-8px text-12px">
          <p>
            <span class="text-fg-2">僵尸进程</span>
            <span class="ml-8px font-mono" :class="snapshot.zombie_count ? 'text-amber' : ''">
              {{ snapshot.zombie_count ?? "暂无数据" }}
            </span>
            <span v-if="snapshot.zombie_parents_complete === false" class="ml-8px text-fg-2"
              >父进程归属不完整</span
            >
          </p>
          <ul v-if="snapshot.zombie_parents?.length" class="mt-4px flex flex-col gap-2px text-fg-2">
            <li v-for="parent in snapshot.zombie_parents" :key="parent.pid" class="truncate">
              {{ parent.name || "未知进程" }} · PID {{ parent.pid }} · {{ parent.count }} 个子进程
            </li>
          </ul>
        </div>
      </section>
    </template>
  </div>
</template>
