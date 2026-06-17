import { onBeforeUnmount, onMounted } from "vue";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";

// 透明边距点击穿透。
//
// 窗口是异形的：左侧与底部留有透明边距以露出下层窗口。这些透明区不仅要“看得见”
// 下层窗口，还要能把鼠标点击透传过去；而实体区域（基础窗口 / 侧边栏 / FAB）必须
// 照常可交互。单个 Tauri 窗口的 setIgnoreCursorEvents 只能整体开关、无法按像素分区，
// 所以策略是：
//   1. 指针位于实体区域 → 关闭穿透（正常接收事件）；位于透明区 → 打开穿透。
//   2. 打开穿透后窗口收不到任何鼠标事件，无法再靠 mousemove 感知“指针移回实体区”，
//      因此改为轮询系统全局光标坐标（cursorPosition 走系统查询，与窗口事件无关），
//      一旦光标回到实体区或离开窗口即关闭穿透、停止轮询。
//
// 平时完全由免费的 DOM mousemove 驱动；只有指针真的落在透明区时才会短暂轮询。
// 实体区域用 `data-ct-solid` 标记（见 layouts/default/index.vue）。
export function useClickThrough() {
  let appWindow: ReturnType<typeof getCurrentWindow>;
  try {
    appWindow = getCurrentWindow();
  } catch {
    return; // 非 Tauri 环境（纯 vite dev）：直接跳过
  }

  let ignoring = false;
  let switching = false;
  let pollId: number | null = null;
  // 进入穿透时缓存窗口位置/缩放：穿透期间窗口无法被拖动，故保持不变。
  let originX = 0;
  let originY = 0;
  let scale = 1;

  function solidAt(clientX: number, clientY: number): boolean {
    const el = document.elementFromPoint(clientX, clientY);
    return !!el?.closest("[data-ct-solid]");
  }

  async function setIgnore(next: boolean) {
    if (next === ignoring || switching) return;
    switching = true;
    ignoring = next;
    try {
      await appWindow.setIgnoreCursorEvents(next);
    } catch {
      /* noop */
    }
    switching = false;
    if (next) void startPolling();
    else stopPolling();
  }

  async function startPolling() {
    if (pollId !== null) return;
    try {
      const pos = await appWindow.innerPosition();
      originX = pos.x;
      originY = pos.y;
      scale = await appWindow.scaleFactor();
    } catch {
      /* noop */
    }
    pollId = window.setInterval(async () => {
      const cur = await cursorPosition().catch(() => null);
      if (!cur) return;
      const x = (cur.x - originX) / scale;
      const y = (cur.y - originY) / scale;
      const outside = x < 0 || y < 0 || x > window.innerWidth || y > window.innerHeight;
      // 光标离开窗口或回到实体区 → 恢复正常事件模式（mousemove 接管）。
      if (outside || solidAt(x, y)) void setIgnore(false);
    }, 50);
  }

  function stopPolling() {
    if (pollId !== null) {
      clearInterval(pollId);
      pollId = null;
    }
  }

  function onMouseMove(e: MouseEvent) {
    if (ignoring) return; // 穿透中收不到事件，交由轮询处理
    const target = e.target as Element | null;
    if (!target?.closest("[data-ct-solid]")) void setIgnore(true);
  }

  onMounted(() => {
    window.addEventListener("mousemove", onMouseMove, { passive: true });
  });
  onBeforeUnmount(() => {
    window.removeEventListener("mousemove", onMouseMove);
    stopPolling();
    if (ignoring) void appWindow.setIgnoreCursorEvents(false);
  });
}
