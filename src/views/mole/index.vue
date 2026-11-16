<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import MoleAnalyze from "./MoleAnalyze.vue";
import MoleApps from "./MoleApps.vue";
import MoleDiagnostics from "./MoleDiagnostics.vue";
import MoleHistory from "./MoleHistory.vue";
import { moleActions } from "./maintenance";
import type { MoleAction, MoleInfo, MoleRequest } from "./maintenance";

const MoleTerminal = defineAsyncComponent(() => import("./MoleTerminal.vue"));

const dialog = useDialog();
const info = shallowRef<MoleInfo | null>(null);
const loading = ref(false);
const error = ref("");
const request = shallowRef<MoleRequest | null>(null);
const terminalRunning = ref(false);
const activeTab = ref("maintenance");
const uninstallAction = moleActions[1];

async function detect() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    info.value = await invoke<MoleInfo>("mole_info");
  } catch (cause) {
    info.value = null;
    error.value = String(cause);
  } finally {
    loading.value = false;
  }
}

function preview(action: MoleAction) {
  terminalRunning.value = true;
  request.value = { action, dryRun: true };
}

function execute(action: MoleAction) {
  dialog.warning({
    title: `运行${action.label}？`,
    content: action.confirmation,
    positiveText: "启动 Mole",
    negativeText: "取消",
    onPositiveClick() {
      terminalRunning.value = true;
      request.value = { action, dryRun: false };
    },
  });
}

function closeTerminal() {
  request.value = null;
}

function finishTerminal() {
  terminalRunning.value = false;
}

onBeforeRouteLeave(() => {
  if (!request.value || request.value.dryRun || !terminalRunning.value) return true;

  return new Promise<boolean>((resolve) => {
    dialog.warning({
      title: "停止任务并离开？",
      content: "Mole 仍在运行。离开会停止当前任务，已完成的操作会保留。",
      positiveText: "停止并离开",
      negativeText: "继续任务",
      onPositiveClick: () => resolve(true),
      onAfterLeave: () => resolve(false),
    });
  });
});

onMounted(detect);
</script>

<template>
  <div class="min-w-0 flex flex-col gap-14px">
    <section
      class="card flex flex-wrap items-center justify-between gap-x-14px gap-y-8px px-14px py-10px"
    >
      <div class="min-w-0 flex flex-1 items-center gap-10px">
        <span
          class="grid h-34px w-34px shrink-0 place-items-center rounded-sm bg-accent-soft text-17px text-primary"
          aria-hidden="true"
          ><span class="i-ph-toolbox-duotone"
        /></span>
        <div class="min-w-0">
          <div class="flex flex-wrap items-baseline gap-x-9px">
            <span class="text-13px font-650">Mole CLI</span>
            <template v-if="info?.path">
              <span class="text-11px text-fg-2">{{ info.bundled ? "内置" : "外部" }}</span>
              <span v-if="info.version" class="text-11px text-fg-2 font-mono">
                {{ info.version }}
              </span>
            </template>
            <span v-else-if="loading" class="text-11px text-fg-2">正在检测</span>
            <span v-else-if="error || info" class="text-11px text-amber">不可用</span>
          </div>
          <div
            v-if="info?.path && !info.bundled"
            class="truncate text-11px text-fg-2 font-mono"
            :title="info.path"
          >
            {{ info.path }}
          </div>
        </div>
      </div>
      <NButton size="tiny" :loading="loading" @click="detect">重新检测</NButton>
    </section>

    <section
      v-if="error || (info && !info.path)"
      role="alert"
      class="card flex flex-col items-start gap-8px p-18px"
    >
      <h2 class="m-0 text-17px font-650">内置 Mole 不可用</h2>
      <p class="m-0 text-13px text-fg-2">请重新安装 HostGuard，或点击「重新检测」重试。</p>
      <p v-if="error" class="m-0 break-words text-12px text-tlclose">{{ error }}</p>
    </section>

    <NTabs v-if="info?.path" v-model:value="activeTab" type="line" size="medium">
      <NTabPane name="maintenance" tab="维护">
        <div class="grid grid-cols-1 min-[820px]:grid-cols-2 gap-10px">
          <article
            v-for="action in moleActions"
            :key="action.id"
            class="card flex flex-col justify-between gap-10px p-13px"
          >
            <header class="flex items-start gap-10px">
              <span
                class="grid h-33px w-33px shrink-0 place-items-center rounded-sm bg-accent-soft text-17px text-primary"
                aria-hidden="true"
                ><span :class="action.icon"
              /></span>
              <div class="min-w-0">
                <h2 class="m-0 text-14px font-650">{{ action.label }}</h2>
                <p class="mt-3px mb-0 text-12px leading-18px text-fg-2">{{ action.description }}</p>
              </div>
            </header>
            <div class="flex items-center justify-end gap-8px">
              <NButton
                size="small"
                type="primary"
                secondary
                :aria-label="`预览${action.label}`"
                @click="preview(action)"
              >
                预览
              </NButton>
              <NButton size="small" :aria-label="`执行${action.label}`" @click="execute(action)">
                执行
              </NButton>
            </div>
          </article>
        </div>
      </NTabPane>

      <NTabPane name="diagnostics" tab="扩展监控">
        <MoleDiagnostics v-if="activeTab === 'diagnostics'" />
      </NTabPane>

      <NTabPane name="analyze" tab="磁盘分析">
        <MoleAnalyze v-if="activeTab === 'analyze'" :home="info.home" />
      </NTabPane>

      <NTabPane name="apps" tab="应用列表">
        <MoleApps v-if="activeTab === 'apps'" @uninstall="execute(uninstallAction)" />
      </NTabPane>

      <NTabPane name="history" tab="操作历史">
        <MoleHistory v-if="activeTab === 'history'" />
      </NTabPane>
    </NTabs>

    <MoleTerminal
      v-if="request"
      :request="request"
      @close="closeTerminal"
      @finished="finishTerminal"
    />
  </div>
</template>
