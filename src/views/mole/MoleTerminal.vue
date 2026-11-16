<script setup lang="ts">
import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { MoleRequest } from "./maintenance";

const props = defineProps<{ request: MoleRequest }>();
const emit = defineEmits<{ close: []; finished: [] }>();

type TerminalEvent = { type: "data"; data: number[] } | { type: "exit"; code: number | null };
type Phase = "starting" | "running" | "stopping" | "exited" | "failed";

const container = shallowRef<HTMLElement | null>(null);
const phase = ref<Phase>("starting");
const status = ref("正在启动");
const error = ref("");
const active = computed(
  () => phase.value === "starting" || phase.value === "running" || phase.value === "stopping",
);

let terminal: Terminal | undefined;
let resizeObserver: ResizeObserver | undefined;
let sessionId: number | undefined;
let pendingInput = Promise.resolve();
let disposed = false;

function writeInput(data: string) {
  const id = sessionId;
  if (id === undefined || phase.value !== "running") return;

  pendingInput = pendingInput
    .then(async () => {
      if (disposed || sessionId !== id || phase.value !== "running") return;
      await invoke("mole_terminal_write", { id, data });
    })
    .catch((cause: unknown) => {
      if (!disposed && sessionId === id) error.value = String(cause);
    });
}

function receive(event: TerminalEvent) {
  if (disposed) return;
  if (event.type === "data") {
    terminal?.write(Uint8Array.from(event.data));
    return;
  }

  sessionId = undefined;
  status.value =
    phase.value === "stopping"
      ? "已停止"
      : event.code === 0
        ? "已完成"
        : event.code === null
          ? "已结束"
          : `退出状态 ${event.code}`;
  phase.value = "exited";
  emit("finished");
}

async function requestStop(id: number) {
  try {
    await invoke("mole_terminal_stop", { id });
  } catch (cause) {
    if (!disposed && sessionId === id) {
      status.value = "仍在运行";
      phase.value = "running";
      error.value = String(cause);
    }
  }
}

function stop() {
  if (!active.value || phase.value === "stopping") return;
  status.value = "正在停止";
  phase.value = "stopping";
  if (sessionId !== undefined) void requestStop(sessionId);
}

function isStopping() {
  return phase.value === "stopping";
}

function focusTerminal() {
  if (!disposed && active.value && !isStopping()) terminal?.focus();
}

async function start() {
  if (disposed || !container.value || terminal) return;
  if (isStopping()) {
    status.value = "已停止";
    phase.value = "exited";
    emit("finished");
    return;
  }

  try {
    const fit = new FitAddon();
    const instance = new Terminal({
      fontFamily: '"JetBrains Mono Variable", Menlo, "PingFang SC", monospace',
      fontSize: 13,
      lineHeight: 1.2,
      scrollback: 1200,
      theme: { background: "#111620", foreground: "#e4ebf4", cursor: "#4bda95" },
      linkHandler: { activate() {} },
    });
    terminal = instance;
    instance.parser.registerOscHandler(52, () => true);
    instance.loadAddon(fit);
    instance.open(container.value);
    fit.fit();
    instance.onData(writeInput);
    instance.onResize(({ cols, rows }) => {
      const id = sessionId;
      if (id !== undefined && phase.value === "running") {
        void invoke("mole_terminal_resize", { id, cols, rows }).catch(() => {
          // 进程可能刚刚结束。
        });
      }
    });
    resizeObserver = new ResizeObserver(() => fit.fit());
    resizeObserver.observe(container.value);
    instance.focus();

    const onEvent = new Channel<TerminalEvent>(receive);
    const id = await invoke<number>("mole_terminal_start", {
      action: props.request.action.id,
      dryRun: props.request.dryRun,
      cols: instance.cols,
      rows: instance.rows,
      onEvent,
    });
    if (disposed) {
      await requestStop(id);
      return;
    }
    // 快速退出的命令可能在 start 响应之前发出 exit。
    if (phase.value === "exited") return;
    sessionId = id;
    if (isStopping()) {
      await requestStop(id);
      return;
    }
    phase.value = "running";
    status.value = "运行中";
    focusTerminal();
    await invoke("mole_terminal_resize", { id, cols: instance.cols, rows: instance.rows }).catch(
      () => {},
    );
  } catch (cause) {
    if (disposed) return;
    error.value = String(cause);
    status.value = "启动失败";
    phase.value = "failed";
    emit("finished");
  }
}

watch(
  container,
  (element) => {
    if (element) void start();
  },
  { flush: "post" },
);

onBeforeUnmount(() => {
  disposed = true;
  if (sessionId !== undefined) void requestStop(sessionId);
  resizeObserver?.disconnect();
  terminal?.dispose();
});
</script>

<template>
  <NModal
    :show="true"
    :mask-closable="false"
    :close-on-esc="false"
    :auto-focus="false"
    to="#overlay-host"
    @after-enter="focusTerminal"
  >
    <!-- Modal 的焦点锁以 div 为内容边界，终端输入框必须在此边界内。 -->
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="mole-terminal-title"
      data-ct-solid
      class="w-[min(90vw,920px)] overflow-hidden rounded-lg border border-hairline bg-[var(--c-bg-1)] shadow-pop"
    >
      <header class="flex items-center justify-between gap-16px px-18px py-14px">
        <div class="min-w-0 flex items-center gap-12px">
          <span
            class="grid h-38px w-38px shrink-0 place-items-center rounded-md bg-accent-soft text-19px text-primary"
            aria-hidden="true"
            ><span class="i-ph-terminal-window-duotone"
          /></span>
          <div class="min-w-0">
            <h2 id="mole-terminal-title" class="m-0 text-15px font-650">
              {{ request.action.label }} · {{ request.dryRun ? "预览" : "执行" }}
            </h2>
            <code class="text-11px text-fg-2 font-mono">
              mo {{ request.action.id }}{{ request.dryRun ? " --dry-run" : "" }}
            </code>
          </div>
        </div>
        <span aria-live="polite" class="shrink-0 flex items-center gap-7px text-12px text-fg-2">
          <span
            class="h-7px w-7px rounded-full"
            :class="active ? 'bg-accent-2' : phase === 'failed' ? 'bg-tlclose' : 'bg-fg-3'"
            aria-hidden="true"
          />
          {{ status }}
        </span>
      </header>

      <div class="border-y border-hairline bg-[#111620] p-12px" aria-label="Mole 交互式终端">
        <div ref="container" class="h-[min(52vh,480px)] min-h-190px select-text" />
      </div>
      <p v-if="error" role="alert" class="m-0 px-18px pt-10px break-words text-12px text-tlclose">
        {{ error }}
      </p>

      <footer class="flex flex-wrap items-center justify-between gap-10px px-18px py-12px">
        <span v-if="active" class="text-12px text-fg-2">
          按终端中的提示操作，操作前请核对清单。
        </span>
        <span v-else class="text-12px text-fg-2">可滚动查看终端输出</span>
        <NButton v-if="active" size="small" :disabled="phase === 'stopping'" @click="stop">
          停止任务
        </NButton>
        <NButton v-else size="small" @click="emit('close')">关闭</NButton>
      </footer>
    </div>
  </NModal>
</template>
