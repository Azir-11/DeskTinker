<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { parentPath, sizeShare, sizeText, type MoleScan } from "./analyze";

const props = defineProps<{ home: string }>();

const pageSize = 40;
const path = ref(props.home || "/");
const result = shallowRef<MoleScan>();
const activeScan = shallowRef<{ id: string; path: string }>();
const cancelling = ref(false);
const error = ref("");
const limit = ref(pageSize);

const loading = computed(() => Boolean(activeScan.value));
const parent = computed(() => parentPath(result.value?.path ?? ""));
const entries = computed(() => [...(result.value?.entries ?? [])].sort((a, b) => b.size - a.size));
const visible = computed(() => entries.value.slice(0, limit.value));
const largeFiles = computed(() => (result.value?.large_files ?? []).slice(0, 20));
const scanTargets = computed(() => [
  { label: "个人目录", path: props.home },
  { label: "应用程序", path: "/Applications" },
  { label: "外接卷", path: "/Volumes" },
]);

async function scan(target: string) {
  if (loading.value || !target.trim()) return;
  const random = crypto.getRandomValues(new Uint32Array(4));
  const requestId = Array.from(random, (value) => value.toString(16).padStart(8, "0")).join("");
  path.value = target;
  activeScan.value = { id: requestId, path: target };
  error.value = "";
  try {
    const data = await invoke<MoleScan>("mole_analyze", { path: target, requestId });
    if (activeScan.value?.id !== requestId || cancelling.value) return;
    result.value = data;
    path.value = data.path;
    limit.value = pageSize;
  } catch (cause) {
    if (activeScan.value?.id === requestId && !cancelling.value) error.value = String(cause);
  } finally {
    if (activeScan.value?.id === requestId) {
      activeScan.value = undefined;
      cancelling.value = false;
    }
  }
}

function submitScan() {
  void scan(path.value);
}

async function cancelScan() {
  const requestId = activeScan.value?.id;
  if (!requestId || cancelling.value) return;
  cancelling.value = true;
  try {
    await invoke("mole_cancel_analyze", { requestId });
  } catch (cause) {
    if (activeScan.value?.id !== requestId) return;
    cancelling.value = false;
    error.value = `无法停止扫描：${String(cause)}`;
  }
}

function showMore() {
  limit.value += pageSize;
}

onBeforeUnmount(() => {
  if (!activeScan.value) return;
  void cancelScan();
  activeScan.value = undefined;
});
</script>

<template>
  <div class="space-y-14px">
    <section class="card p-16px" aria-labelledby="mole-scan-title">
      <h3 id="mole-scan-title" class="mb-9px flex items-center gap-7px text-14px font-650">
        <span class="i-ph-folder-open-duotone text-18px text-accent" aria-hidden="true" />
        扫描目录
      </h3>
      <form class="flex items-center gap-8px" @submit.prevent="submitScan">
        <NInput
          v-model:value="path"
          class="min-w-0 flex-1 font-mono"
          placeholder="/路径 或 ~/目录"
          :input-props="{ 'aria-label': '扫描路径', spellcheck: false }"
          :disabled="loading"
        />
        <NButton v-if="loading" type="warning" secondary :loading="cancelling" @click="cancelScan">
          停止扫描
        </NButton>
        <NButton v-else attr-type="submit" type="primary" :disabled="!path.trim()">
          开始扫描
        </NButton>
      </form>
      <div class="mt-9px flex flex-wrap gap-6px">
        <NButton
          v-for="target in scanTargets"
          :key="target.path"
          size="tiny"
          tertiary
          :disabled="loading || !target.path"
          @click="scan(target.path)"
        >
          {{ target.label }}
        </NButton>
      </div>
    </section>

    <p v-if="activeScan" role="status" class="break-all text-12px text-fg-2">
      {{ cancelling ? "正在停止扫描…" : `正在扫描 ${activeScan.path}…` }}
    </p>
    <NAlert v-if="error" type="error" title="磁盘扫描失败" class="break-words">{{ error }}</NAlert>

    <section
      v-if="result"
      class="card min-w-0"
      aria-labelledby="mole-scan-result-title"
      :aria-busy="loading"
    >
      <header
        class="flex items-start justify-between gap-12px border-b border-hairline px-16px py-12px"
      >
        <div class="min-w-0">
          <h3 id="mole-scan-result-title" class="text-14px font-650">目录用量</h3>
          <p class="truncate font-mono text-12px text-fg-2" :title="result.path">
            {{ result.path }}
          </p>
        </div>
        <div class="shrink-0 text-right">
          <strong class="font-mono text-17px font-650">{{
            sizeText(result.total_size, result.scan_status)
          }}</strong>
          <p v-if="typeof result.total_files === 'number'" class="text-11px text-fg-2">
            {{ result.total_files.toLocaleString("zh-CN") }} 个已统计文件
          </p>
        </div>
      </header>

      <div
        v-if="parent || result.scan_status === 'partial' || result.scan_status === 'unavailable'"
        class="flex flex-wrap items-center gap-8px px-16px pt-8px"
      >
        <NButton v-if="parent" size="tiny" tertiary :disabled="loading" @click="scan(parent)">
          <template #icon><span class="i-ph-arrow-up-bold" aria-hidden="true" /></template>
          上一级
        </NButton>
        <span v-if="result.scan_status === 'partial'" class="text-12px text-amber">
          部分扫描完成，带 + 的大小仅包含可统计的数据。
        </span>
        <span v-else-if="result.scan_status === 'unavailable'" class="text-12px text-amber">
          无法读取此目录的用量。
        </span>
      </div>

      <ul v-if="visible.length" class="divide-y divide-hairline px-16px py-4px">
        <li
          v-for="entry in visible"
          :key="entry.path"
          class="grid grid-cols-[minmax(0,1fr)_100px] items-center gap-x-10px gap-y-4px py-7px text-12px"
        >
          <button
            v-if="entry.is_dir"
            type="button"
            class="flex min-w-0 items-center gap-7px rounded-sm text-left text-fg hover:text-accent disabled:opacity-55 focus-visible:outline focus-visible:outline-accent"
            :disabled="loading"
            :title="entry.path"
            @click="scan(entry.path)"
          >
            <span class="i-ph-folder-duotone shrink-0 text-16px text-violet" aria-hidden="true" />
            <span class="truncate">{{ entry.name }}</span>
            <span class="i-ph-caret-right-bold shrink-0 text-10px text-fg-2" aria-hidden="true" />
          </button>
          <span v-else class="flex min-w-0 items-center gap-7px" :title="entry.path">
            <span class="i-ph-file-duotone shrink-0 text-16px text-fg-2" aria-hidden="true" />
            <span class="truncate">{{ entry.name }}</span>
          </span>
          <span
            class="text-right font-mono"
            :class="entry.scan_status === 'partial' ? 'text-amber' : 'text-fg-2'"
          >
            {{ sizeText(entry.size, entry.scan_status) }}
          </span>
          <div
            v-if="result.total_size > 0 && entry.scan_status !== 'unavailable'"
            class="col-span-2 h-2px overflow-hidden rounded-full bg-elevated"
            aria-hidden="true"
          >
            <div
              class="h-full rounded-full"
              :class="entry.is_dir ? 'bg-violet' : 'bg-info'"
              :style="{ width: `${sizeShare(entry, result.total_size)}%` }"
            />
          </div>
        </li>
      </ul>
      <p
        v-else-if="result.scan_status !== 'unavailable'"
        class="px-16px py-12px text-12px text-fg-2"
      >
        {{
          result.scan_status === "partial" ? "未统计到可展示的条目。" : "此目录没有可展示的条目。"
        }}
      </p>
      <div v-if="entries.length > limit" class="px-16px pb-8px">
        <NButton size="small" tertiary @click="showMore"
          >显示更多（{{ entries.length - limit }} 项）</NButton
        >
      </div>
    </section>

    <section
      v-if="largeFiles.length"
      class="card min-w-0 px-16px py-12px"
      aria-labelledby="mole-large-files-title"
    >
      <h3 id="mole-large-files-title" class="mb-6px flex items-center gap-7px text-14px font-650">
        <span class="i-ph-files-duotone text-18px text-info" aria-hidden="true" />
        大文件
      </h3>
      <ul class="divide-y divide-hairline">
        <li
          v-for="file in largeFiles"
          :key="file.path"
          class="grid grid-cols-[minmax(0,1fr)_100px] items-center gap-10px py-6px text-12px"
        >
          <span class="min-w-0" :title="file.path">
            <span class="block truncate">{{ file.name }}</span>
            <span class="block truncate text-11px text-fg-2">{{ file.path }}</span>
          </span>
          <span class="text-right font-mono text-fg-2">{{ sizeText(file.size) }}</span>
        </li>
      </ul>
    </section>
  </div>
</template>
