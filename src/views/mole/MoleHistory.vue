<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { formatCapacity } from "@/views/monitor/shared";
import { moleActionLabel } from "./maintenance";

interface OperationSession {
  command: string;
  run_id?: string;
  attribution?: string;
  started_at: string;
  ended_at: string;
  items: number;
  size: string;
  actions: {
    failed: number;
  };
}

interface Deletion {
  timestamp: string;
  mode: string;
  status: string;
  size_kb: number | null;
  path: string;
}

interface History {
  sessions: OperationSession[];
  deletions: Deletion[];
}

const history = shallowRef<History | null>(null);
const pending = ref(false);
const error = ref("");

async function load() {
  if (pending.value) return;
  pending.value = true;
  error.value = "";
  try {
    const result = await invoke<History>("mole_history");
    if (!Array.isArray(result?.sessions) || !Array.isArray(result.deletions)) {
      throw new Error("Mole 的历史记录格式无法识别");
    }
    history.value = result;
  } catch (cause) {
    error.value = String(cause);
  } finally {
    pending.value = false;
  }
}

function fileSize(kib: number | null): string {
  return kib === null ? "—" : formatCapacity(kib * 1024, true);
}

onMounted(load);
</script>

<template>
  <div class="flex flex-col gap-12px">
    <header class="flex items-center justify-between gap-12px">
      <p class="m-0 text-12px text-fg-2">显示 Mole 保存的近期记录。</p>
      <NButton size="small" :loading="pending" @click="load">刷新</NButton>
    </header>

    <p v-if="error" role="alert" class="m-0 break-words text-12px text-tlclose">{{ error }}</p>
    <div v-if="pending && !history" class="card flex items-center gap-8px p-14px text-fg-2">
      <NSpin size="small" />
      正在读取历史记录
    </div>

    <template v-if="history">
      <section class="card overflow-hidden">
        <h2 class="m-0 px-14px py-10px text-13px font-650">维护任务</h2>
        <p v-if="!history.sessions.length" class="m-0 border-t border-hairline p-14px text-fg-2">
          暂无维护任务记录。
        </p>
        <ul v-else class="m-0 list-none p-0">
          <li
            v-for="(session, index) in history.sessions"
            :key="session.run_id || index"
            class="flex flex-wrap items-start justify-between gap-8px border-t border-hairline px-14px py-10px"
          >
            <div class="min-w-0">
              <div class="text-13px font-600">{{ moleActionLabel(session.command) }}</div>
              <time v-if="session.started_at" class="text-11px text-fg-2 font-mono">
                {{ session.started_at }}
              </time>
              <span v-if="!session.ended_at" class="ml-6px text-11px text-amber">
                缺少结束记录
              </span>
            </div>
            <span v-if="session.attribution === 'ambiguous'" class="text-12px text-amber">
              历史记录无法准确归属于单次任务
            </span>
            <div v-else class="flex flex-wrap gap-x-10px gap-y-3px text-12px text-fg-2">
              <span>{{ session.items }} 项</span>
              <span v-if="session.size" class="font-mono">{{ session.size }}</span>
              <span v-if="session.actions.failed" class="text-tlclose">
                {{ session.actions.failed }} 项失败
              </span>
            </div>
          </li>
        </ul>
      </section>

      <section class="card overflow-hidden">
        <h2 class="m-0 px-14px py-10px text-13px font-650">文件操作</h2>
        <p v-if="!history.deletions.length" class="m-0 border-t border-hairline p-14px text-fg-2">
          暂无文件操作记录。
        </p>
        <ul v-else class="m-0 list-none p-0">
          <li
            v-for="(file, index) in history.deletions"
            :key="index"
            class="flex items-start justify-between gap-12px border-t border-hairline px-14px py-10px"
          >
            <div class="min-w-0">
              <div class="truncate text-12px font-mono" :title="file.path">{{ file.path }}</div>
              <time class="text-11px text-fg-2 font-mono">{{ file.timestamp }}</time>
            </div>
            <div class="shrink-0 flex flex-col items-end text-11px text-fg-2 font-mono">
              <span>{{ fileSize(file.size_kb) }}</span>
              <span>{{ file.mode }} · {{ file.status }}</span>
            </div>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>
