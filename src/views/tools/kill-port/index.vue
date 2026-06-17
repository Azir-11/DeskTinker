<script setup lang="ts">
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useDialog, useMessage } from "naive-ui";

interface PortProcess {
  pid: number;
  name: string;
  proto: string;
  local_addr: string;
}

const message = useMessage();
const dialog = useDialog();

// 开发服务器固定占用 1420（vite strictPort），项目菜单。
const DEV_PORT = 1420;
const port = ref<number | null>(DEV_PORT);
const busy = ref(false);

const validPort = computed(() => !!port.value && port.value >= 1 && port.value <= 65535);

async function detect() {
  if (!validPort.value) {
    message.warning("请输入有效端口（1–65535）");
    return;
  }
  const p = port.value!;
  busy.value = true;
  try {
    const list = await invoke<PortProcess[]>("check_port", { port: p });
    if (list.length === 0) {
      message.success(`端口 ${p} 未被占用`);
    } else {
      confirmKill(p, list);
    }
  } catch (e) {
    message.error(`检测失败：${String(e)}`);
  } finally {
    busy.value = false;
  }
}

function confirmKill(p: number, list: PortProcess[]) {
  const names = list.map((o) => `${o.name} (PID ${o.pid})`).join("、");
  dialog.warning({
    title: `端口 ${p} 被占用`,
    content: `检测到 ${list.length} 个进程占用该端口：${names}。确定结束并释放端口 ${p} 吗？`,
    positiveText: "结束进程",
    negativeText: "取消",
    // 返回 Promise，弹窗会等其 resolve 再关闭；内部已吞异常，保证弹窗正常关闭。
    onPositiveClick: () => killAll(p, list),
  });
}

async function killAll(p: number, list: PortProcess[]) {
  await Promise.allSettled(list.map((o) => invoke("kill_process", { pid: o.pid })));
  // 以「端口是否仍被占用」复检为准来判定结果：规避「进程在确认期间已自行退出 / 被作为
  // 子进程连带回收」导致 taskkill 报错、实则端口已释放的误报。
  let remaining: PortProcess[] = [];
  try {
    remaining = await invoke<PortProcess[]>("check_port", { port: p });
  } catch {
    /* 复检失败：保守地不下成功结论 */
  }
  if (remaining.length === 0) {
    message.success(`端口 ${p} 已释放`);
  } else {
    message.error(`端口 ${p} 仍被 ${remaining.length} 个进程占用，可能需要管理员权限`);
  }
}
</script>

<template>
  <div class="h-full">
    <section class="card p-18px flex flex-col gap-14px w-full max-w-[440px]">
      <div class="flex items-center gap-10px">
        <span class="i-ph-plug-duotone text-20px text-accent-2 shrink-0" />
        <span class="text-14px font-600">端口工具</span>
      </div>

      <div class="flex items-end gap-10px flex-wrap">
        <div class="flex flex-col gap-6px">
          <label class="text-12px text-fg-3">端口号</label>
          <NInputNumber
            v-model:value="port"
            :min="1"
            :max="65535"
            :show-button="false"
            placeholder="1–65535"
            class="w-150px"
            @keyup.enter="detect"
          />
        </div>
        <NButton type="primary" :loading="busy" :disabled="!validPort" @click="detect">
          <template #icon><span class="i-ph-magnifying-glass-duotone" /></template>
          检测端口
        </NButton>
      </div>

      <p class="m-0 text-12px text-fg-3 leading-[1.5]">
        检测端口是否被占用；被占用时可在二次确认后结束相关进程以释放端口
      </p>
    </section>
  </div>
</template>
