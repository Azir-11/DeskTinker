<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { useFabStore } from "@/store/fab";

// 记住的地址持久化到 localStorage：本页不做 keep-alive，靠它在「切走再回来 /
// 退出重开」后恢复预览态；只有点「回溯」才清除。
const STORAGE_KEY = "hostguard.web-preview.url";

const message = useMessage();
const fab = useFabStore();

const input = ref("");
const url = ref(localStorage.getItem(STORAGE_KEY) ?? "");
const loaded = computed(() => !!url.value);

/** 把用户输入规整成可用 URL：缺协议时补 https://，非法返回 null。 */
function normalize(raw: string): string | null {
  const s = raw.trim();
  if (!s) return null;
  const withProto = /^https?:\/\//i.test(s) ? s : `https://${s}`;
  try {
    const u = new URL(withProto);
    return u.hostname ? u.toString() : null;
  } catch {
    return null;
  }
}

function open(): boolean {
  const normalized = normalize(input.value);
  if (!normalized) {
    message.warning("请输入有效的网址或域名");
    return false;
  }
  url.value = normalized;
  localStorage.setItem(STORAGE_KEY, normalized);
  applyFab();
  return true;
}

function goBack(): boolean {
  input.value = url.value; // 预填上次地址，方便编辑后再次预览
  url.value = "";
  localStorage.removeItem(STORAGE_KEY);
  applyFab();
  return true;
}

// FAB 随状态变形：输入态「预览」，预览态「回溯」。
function applyFab() {
  if (loaded.value) {
    fab.set({ label: "回溯", icon: "i-ph-arrow-arc-left-duotone", run: goBack });
  } else {
    fab.set({ label: "预览", icon: "i-ph-eye-duotone", run: open });
  }
}

onMounted(() => {
  if (url.value && !input.value) input.value = url.value;
  applyFab();
});
onUnmounted(() => fab.clear());
</script>

<template>
  <!-- 预览态：页面铺满版面 -->
  <div
    v-if="loaded"
    class="h-full w-full rounded-lg overflow-hidden border border-hairline bg-[var(--c-bg-0)]"
  >
    <iframe :src="url" class="block w-full h-full border-none" />
  </div>

  <!-- 输入态：居中输入框 -->
  <div v-else class="h-full grid place-items-center">
    <div class="w-full max-w-[460px] flex flex-col items-center gap-18px text-center">
      <span class="i-ph-monitor-play-duotone text-46px text-accent-2" />
      <div>
        <h2 class="m-0 text-20px font-700 font-display">网页预览</h2>
        <p class="mt-7px mb-0 text-13px text-fg-2 leading-[1.6]">
          输入网址或域名，整个版面将直接展示该页面。
        </p>
      </div>
      <NInput
        v-model:value="input"
        placeholder="例如 example.com"
        size="large"
        clearable
        class="w-full"
        @keyup.enter="fab.trigger()"
      >
        <template #prefix><span class="i-ph-globe-duotone text-fg-3" /></template>
      </NInput>
      <p class="m-0 text-12px text-fg-3">按回车，或点击下方「预览」按钮</p>
    </div>
  </div>
</template>
