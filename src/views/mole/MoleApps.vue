<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { formatCapacity } from "@/views/monitor/shared";

interface InstalledApp {
  name: string;
  bundle_id: string;
  source: string;
  path: string;
  size: string | number | null;
}

const emit = defineEmits<{ uninstall: [] }>();

const apps = shallowRef<InstalledApp[]>([]);
const query = ref("");
const pending = ref(false);
const error = ref("");
const filteredApps = computed(() => {
  const needle = query.value.trim().toLowerCase();
  if (!needle) return apps.value;
  return apps.value.filter((app) =>
    [app.name, app.bundle_id, app.path].some((value) => value.toLowerCase().includes(needle)),
  );
});

async function load() {
  if (pending.value) return;
  pending.value = true;
  error.value = "";
  try {
    const result = await invoke<InstalledApp[]>("mole_list_apps");
    if (!Array.isArray(result)) throw new Error("Mole 的应用列表格式无法识别");
    apps.value = result;
  } catch (cause) {
    error.value = String(cause);
  } finally {
    pending.value = false;
  }
}

function displaySize(size: InstalledApp["size"]): string {
  if (typeof size === "number") {
    return formatCapacity(size, false);
  }
  return size && size !== "--" ? size : "—";
}

onMounted(load);
</script>

<template>
  <section class="card flex flex-col gap-12px p-14px">
    <div class="flex flex-wrap items-center gap-8px">
      <NInput
        v-model:value="query"
        clearable
        size="small"
        placeholder="按应用名称或标识符搜索"
        aria-label="搜索已安装应用"
        class="max-w-300px min-w-190px flex-1"
      />
      <NButton size="small" :loading="pending" @click="load">刷新</NButton>
      <NButton size="small" class="ml-auto" @click="emit('uninstall')">打开卸载工具</NButton>
    </div>

    <p v-if="error" role="alert" class="m-0 break-words text-12px text-tlclose">{{ error }}</p>
    <div v-if="pending && !apps.length" class="flex items-center gap-8px p-12px text-fg-2">
      <NSpin size="small" />
      Mole 正在读取应用列表
    </div>
    <p v-else-if="!error && !filteredApps.length" class="m-0 p-12px text-13px text-fg-2">
      {{ query ? "没有匹配的应用" : "Mole 未找到已安装应用" }}
    </p>

    <div v-if="filteredApps.length" class="max-h-[435px] overflow-auto">
      <table class="w-full border-collapse text-left text-12px">
        <thead>
          <tr class="text-fg-2">
            <th scope="col" class="px-8px py-7px font-550">应用</th>
            <th scope="col" class="px-8px py-7px font-550">来源</th>
            <th scope="col" class="px-8px py-7px text-right font-550">大小</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="app in filteredApps" :key="app.path" class="border-t border-hairline">
            <td class="max-w-0 w-full px-8px py-8px">
              <div class="text-13px font-600">{{ app.name }}</div>
              <div class="truncate text-11px text-fg-2 font-mono" :title="app.path">
                {{ app.path }}
              </div>
            </td>
            <td class="px-8px py-8px whitespace-nowrap text-fg-2">
              {{ app.source === "App" ? "应用程序" : app.source || "—" }}
            </td>
            <td class="px-8px py-8px text-right whitespace-nowrap text-fg-2 font-mono">
              {{ displaySize(app.size) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div v-if="apps.length" class="px-8px text-11px text-fg-2">
      显示 {{ filteredApps.length }} / {{ apps.length }} 个应用
    </div>
  </section>
</template>
