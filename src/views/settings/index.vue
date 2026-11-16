<script setup lang="ts">
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useMessage } from "naive-ui";
import { onMounted, ref } from "vue";
import { useUpdater } from "@/composables/useUpdater";
import { useAppStore } from "@/store/app";

const appStore = useAppStore();
const message = useMessage();
const { status, newVersion, releaseNotes, progress, errorMsg, checkForUpdate, downloadAndInstall } =
  useUpdater();

const minRetentionDays = 1;
const maxRetentionDays = 30;
const version = ref("");
const savedRetentionDays = ref<number | null>(null);
const draftRetentionDays = ref<number | string>("");
const retentionLoading = ref(false);
const retentionSaving = ref(false);
const retentionError = ref("");
const nextRetentionDays = computed(() => {
  const days = draftRetentionDays.value;
  if (
    typeof days !== "number" ||
    !Number.isInteger(days) ||
    days < minRetentionDays ||
    days > maxRetentionDays ||
    savedRetentionDays.value === null ||
    days === savedRetentionDays.value ||
    retentionSaving.value
  ) {
    return null;
  }
  return days;
});

async function loadRetention() {
  retentionLoading.value = true;
  retentionError.value = "";
  try {
    const days = await invoke<number>("read_cpu_history_retention");
    savedRetentionDays.value = days;
    draftRetentionDays.value = days;
  } catch (error) {
    retentionError.value = `读取 CPU 历史保留期失败：${error}`;
  } finally {
    retentionLoading.value = false;
  }
}

async function saveRetention() {
  const days = nextRetentionDays.value;
  if (days === null) return;
  retentionSaving.value = true;
  retentionError.value = "";
  try {
    await invoke("set_cpu_history_retention", { days });
    savedRetentionDays.value = days;
    message.success("已保存 CPU 历史保留期");
  } catch (error) {
    retentionError.value = `保存 CPU 历史保留期失败：${error}`;
  } finally {
    retentionSaving.value = false;
  }
}

async function onCheck() {
  await checkForUpdate();
  if (status.value === "uptodate") message.success("已是最新版本");
  else if (status.value === "error") message.error(`检查更新失败：${errorMsg.value}`);
}

async function onInstall() {
  await downloadAndInstall();
  if (status.value === "error") message.error(`更新失败：${errorMsg.value}`);
}

async function openRepo() {
  try {
    await openUrl("https://github.com/Azir-11/HostGuard");
  } catch {
    /* noop outside Tauri */
  }
}

onMounted(async () => {
  void loadRetention();
  try {
    version.value = await getVersion();
  } catch {
    /* web dev 环境下无 Tauri，忽略 */
  }
});
</script>

<template>
  <div class="flex flex-col gap-18px max-w-680px">
    <section class="rounded-lg border border-hairline bg-elevated overflow-hidden">
      <h3 class="px-18px pt-15px pb-9px text-13px font-600 text-fg-2 tracking-[0.3px]">外观</h3>
      <div
        class="flex items-center justify-between gap-16px px-18px py-14px border-t border-hairline"
      >
        <div class="flex flex-col gap-3px min-w-0">
          <span class="text-14px font-500">主题</span>
          <span class="text-[12.5px] text-fg-3">在深色与浅色界面之间切换</span>
        </div>
        <div class="flex gap-4px p-4px rounded-md border border-hairline bg-[var(--c-bg-0)]">
          <button
            class="inline-flex items-center gap-6px px-14px py-7px rounded-sm border-none cursor-pointer text-13px transition-colors"
            :class="
              appStore.isDark
                ? 'bg-grad-accent !text-[#04130b] font-600'
                : 'bg-transparent text-fg-2'
            "
            @click="appStore.setDark(true)"
          >
            <span class="i-ph-moon-stars-duotone" /> 深色
          </button>
          <button
            class="inline-flex items-center gap-6px px-14px py-7px rounded-sm border-none cursor-pointer text-13px transition-colors"
            :class="
              !appStore.isDark
                ? 'bg-grad-accent !text-[#04130b] font-600'
                : 'bg-transparent text-fg-2'
            "
            @click="appStore.setDark(false)"
          >
            <span class="i-ph-sun-duotone" /> 浅色
          </button>
        </div>
      </div>
    </section>

    <section
      class="rounded-lg border border-hairline bg-elevated overflow-hidden"
      aria-labelledby="monitor-settings-heading"
    >
      <h3
        id="monitor-settings-heading"
        class="px-18px pt-15px pb-9px text-13px font-600 text-fg-2 tracking-[0.3px]"
      >
        系统监控
      </h3>
      <form
        class="flex flex-wrap items-center justify-between gap-12px px-18px py-14px border-t border-hairline"
        @submit.prevent="saveRetention"
      >
        <div class="flex flex-col gap-3px min-w-0">
          <label class="text-14px font-500" for="cpu-history-retention">CPU 历史保留期</label>
          <span id="cpu-history-retention-help" class="text-[12.5px] text-fg-3">
            可设置 {{ minRetentionDays }}–{{ maxRetentionDays }} 天，过期记录自动删除。
            趋势图最多显示最近 6 小时。
          </span>
        </div>
        <div class="flex items-center gap-8px">
          <input
            id="cpu-history-retention"
            v-model="draftRetentionDays"
            type="number"
            :min="minRetentionDays"
            :max="maxRetentionDays"
            step="1"
            placeholder="1–30"
            aria-describedby="cpu-history-retention-help"
            :disabled="savedRetentionDays === null || retentionSaving"
            class="w-64px px-8px py-7px rounded-sm border border-hairline bg-[var(--c-bg-0)] text-fg font-mono text-13px disabled:opacity-50"
          />
          <span class="text-13px text-fg-2">天</span>
          <NButton
            v-if="retentionError && savedRetentionDays === null"
            size="small"
            attr-type="button"
            @click="loadRetention"
          >
            重试
          </NButton>
          <NButton
            v-else
            size="small"
            attr-type="submit"
            :loading="retentionLoading || retentionSaving"
            :disabled="nextRetentionDays === null"
          >
            保存
          </NButton>
        </div>
        <p v-if="retentionError" class="m-0 w-full text-12px text-red-400" role="alert">
          {{ retentionError }}
        </p>
      </form>
    </section>

    <section class="rounded-lg border border-hairline bg-elevated overflow-hidden">
      <h3 class="px-18px pt-15px pb-9px text-13px font-600 text-fg-2 tracking-[0.3px]">关于</h3>
      <div
        class="flex items-center justify-between gap-16px px-18px py-14px border-t border-hairline"
      >
        <div class="flex flex-col gap-3px min-w-0">
          <span class="text-14px font-500">HostGuard</span>
          <span class="text-[12.5px] text-fg-3">hosts、Shell 与系统配置管理工具</span>
        </div>
        <span class="px-10px py-4px rounded-[8px] font-mono text-13px text-fg-2 bg-elevated-2">
          v{{ version || "—" }}
        </span>
      </div>

      <div class="flex flex-col gap-10px px-18px py-14px border-t border-hairline">
        <div class="flex items-center justify-between gap-16px">
          <div class="flex flex-col gap-3px min-w-0">
            <span class="flex items-center gap-7px text-14px font-500">
              <span class="i-ph-arrows-clockwise-duotone" /> 软件更新
            </span>
            <span class="text-[12.5px] text-fg-3">
              {{
                status === "available"
                  ? `发现新版本 v${newVersion}`
                  : status === "downloading"
                    ? `正在下载更新 ${progress}%`
                    : "从 GitHub 检查最新版本"
              }}
            </span>
          </div>
          <button
            v-if="status !== 'available' && status !== 'downloading'"
            class="inline-flex items-center gap-6px px-14px py-7px rounded-sm border border-hairline bg-[var(--c-bg-0)] cursor-pointer text-13px text-fg-2 transition-colors hover:bg-elevated-2 disabled:opacity-50 disabled:cursor-default"
            :disabled="status === 'checking'"
            @click="onCheck"
          >
            <span
              :class="
                status === 'checking'
                  ? 'i-ph-spinner-duotone animate-spin'
                  : 'i-ph-arrows-clockwise-bold'
              "
            />
            {{ status === "checking" ? "检查中…" : "检查更新" }}
          </button>
          <button
            v-else-if="status === 'available'"
            class="inline-flex items-center gap-6px px-14px py-7px rounded-sm border-none cursor-pointer text-13px font-600 bg-grad-accent !text-[#04130b] transition-opacity hover:opacity-90"
            @click="onInstall"
          >
            <span class="i-ph-download-simple-bold" /> 下载并安装
          </button>
          <span v-else class="inline-flex items-center gap-6px px-14px py-7px text-13px text-fg-3">
            <span class="i-ph-spinner-duotone animate-spin" /> {{ progress }}%
          </span>
        </div>

        <div
          v-if="status === 'available' && releaseNotes"
          class="rounded-md border border-hairline bg-[var(--c-bg-0)] px-12px py-9px text-[12.5px] text-fg-3 whitespace-pre-wrap max-h-160px overflow-auto"
        >
          {{ releaseNotes }}
        </div>
      </div>

      <div
        class="flex items-center justify-between gap-16px px-18px py-14px border-t border-hairline cursor-pointer transition-colors hover:bg-elevated-2"
        @click="openRepo"
      >
        <div class="flex flex-col gap-3px min-w-0">
          <span class="flex items-center gap-7px text-14px font-500">
            <span class="i-ph-github-logo-duotone" /> 源代码
          </span>
          <span class="text-[12.5px] text-fg-3">github.com/Azir-11/HostGuard</span>
        </div>
        <span class="i-ph-arrow-up-right-bold text-15px text-fg-3" />
      </div>
    </section>
  </div>
</template>
