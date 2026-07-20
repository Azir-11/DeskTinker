<script setup lang="ts">
import { computed, h, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useDialog, useMessage } from "naive-ui";

interface PortProcess {
  pid: number;
  name: string;
  proto: string;
  local_addr: string;
  executable_path?: string | null;
  command_line?: string | null;
  cwd?: string | null;
  parent_pid?: number | null;
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
  dialog.warning({
    title: `端口 ${p} 被占用`,
    content: () => renderProcessDetails(list),
    positiveText: "结束进程",
    negativeText: "取消",
    style: { width: "min(640px, calc(100vw - 48px))" },
    // 返回 Promise，弹窗会等其 resolve 再关闭；内部已吞异常，保证弹窗正常关闭。
    onPositiveClick: () => killAll(p, list),
  });
}

function renderProcessDetails(list: PortProcess[]) {
  return h("div", { class: "flex flex-col gap-10px" }, [
    h(
      "p",
      { class: "m-0 text-13px text-fg-2" },
      `检测到 ${list.length} 个进程占用该端口。确认信息后可结束进程并释放端口。`,
    ),
    h(
      "div",
      { class: "flex flex-col gap-8px max-h-[420px] overflow-y-auto pr-2px" },
      list.map((process) => renderProcess(process)),
    ),
  ]);
}

function renderProcess(process: PortProcess) {
  const details: Array<[string, string]> = [["监听", `${process.proto} ${process.local_addr}`]];
  if (process.command_line) details.push(["启动命令", process.command_line]);
  if (process.cwd) details.push(["工作目录", process.cwd]);
  if (process.executable_path) details.push(["可执行文件", process.executable_path]);
  if (process.parent_pid) details.push(["父进程", `PID ${process.parent_pid}`]);

  return h("section", { class: "rounded-md border border-hairline bg-elevated px-12px py-10px" }, [
    h("div", { class: "flex items-center gap-8px mb-8px min-w-0" }, [
      h("span", { class: "i-ph-terminal-window-duotone text-17px text-accent-2 shrink-0" }),
      h("strong", { class: "text-13px font-600 truncate" }, process.name || "未知进程"),
      h("code", { class: "ml-auto text-11px text-fg-3 font-mono shrink-0" }, `PID ${process.pid}`),
    ]),
    h(
      "dl",
      { class: "m-0 flex flex-col gap-6px" },
      details.map(([label, value]) =>
        h("div", { class: "grid grid-cols-[64px_minmax(0,1fr)] gap-8px items-start" }, [
          h("dt", { class: "text-11px text-fg-3 leading-[1.55]" }, label),
          h(
            "dd",
            {
              class: "m-0 text-11px text-fg-2 leading-[1.55] break-all select-text font-mono",
            },
            value,
          ),
        ]),
      ),
    ),
  ]);
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
