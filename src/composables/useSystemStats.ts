import { invoke } from "@tauri-apps/api/core";

/** 实时指标（CPU/内存/交换/负载/频率），由后端 `read_system_stats` 轮询。 */
export interface SystemStats {
  sampled_at: number;
  cpu_usage: number;
  per_core: number[];
  cpu_freq: number; // MHz
  mem_total: number;
  mem_used: number;
  mem_available: number;
  swap_total: number;
  swap_used: number;
  load_one: number;
  load_five: number;
  load_fifteen: number;
  uptime: number;
}

export interface ProcessStats {
  readonly pid: number;
  readonly name: string;
  readonly cpu_usage: number | null;
  readonly memory: number;
}

export interface CpuSample {
  readonly time: number;
  readonly usage: number;
  readonly starts_segment: boolean;
}

export const CPU_HISTORY_LIMIT_MS = 6 * 60 * 60_000;

/** 静态/缓变主机信息，只取一次。 */
export interface HostInfo {
  os: string;
  os_version: string;
  kernel_version: string;
  long_os_version: string;
  arch: string;
  host_name: string;
  cpu_brand: string;
  core_count: number;
  physical_core_count: number | null;
  boot_time: number;
}

export interface DiskInfo {
  name: string;
  mount: string;
  fs: string;
  total: number;
  available: number;
  used: number;
}

/** 同一种 IPC 在定时刷新、手动刷新和窗口恢复时共用进行中的请求。 */
function singleFlight(run: () => Promise<void>): () => Promise<void> {
  let pending: Promise<void> | undefined;
  return () => {
    pending ??= run().finally(() => {
      pending = undefined;
    });
    return pending;
  };
}

/** 仅在使用它的页面挂载且窗口可见时轮询，温度和磁盘分别降频。 */
export function useSystemStats({ includeMonitorDetails = false } = {}) {
  const stats = shallowRef<SystemStats | null>(null);
  const processes = shallowRef<readonly ProcessStats[] | null>(null);
  const processError = ref(false);
  const cpuHistory = shallowRef<readonly CpuSample[]>([]);
  const historyError = ref(false);
  const host = shallowRef<HostInfo | null>(null);
  const disks = shallowRef<DiskInfo[]>([]);
  const cpuTemp = ref<number | null>(null);
  const error = ref("");
  const diskStatus = ref<"loading" | "ready" | "error">("loading");

  let timers: number[] = [];

  const pollLive = singleFlight(async () => {
    try {
      const sample = await invoke<SystemStats>("read_system_stats");
      if (stats.value?.sampled_at !== sample.sampled_at) stats.value = sample;
      error.value = "";
    } catch (e) {
      error.value = String(e);
    }
  });

  const pollProcesses = singleFlight(async () => {
    try {
      processes.value = await invoke<readonly ProcessStats[]>("read_process_stats");
      processError.value = false;
    } catch {
      processError.value = true;
    }
  });

  const pollHistory = singleFlight(async () => {
    try {
      const previous = cpuHistory.value;
      const since = previous[previous.length - 1]?.time;
      const updates = await invoke<readonly CpuSample[]>("read_cpu_history", { since });
      if (updates.length) {
        const cutoff = updates[updates.length - 1].time - CPU_HISTORY_LIMIT_MS;
        // 时钟回拨后，后端会返回新一段完整历史，替换旧时钟下的缓存。
        const retained = since !== undefined && updates[0].time > since ? previous : [];
        cpuHistory.value = [...retained.filter((sample) => sample.time >= cutoff), ...updates];
      }
      historyError.value = false;
    } catch {
      historyError.value = true;
    }
  });

  const pollTemp = singleFlight(async () => {
    try {
      cpuTemp.value = await invoke<number | null>("read_cpu_temp");
    } catch {
      cpuTemp.value = null;
    }
  });

  const loadHost = singleFlight(async () => {
    if (host.value) return;
    try {
      host.value = await invoke<HostInfo>("read_host_info");
    } catch {
      /* 非 Tauri 环境忽略 */
    }
  });

  const loadDisks = singleFlight(async () => {
    try {
      disks.value = await invoke<DiskInfo[]>("read_disks");
      diskStatus.value = "ready";
    } catch {
      diskStatus.value = "error";
    }
  });

  const polls = [
    { run: pollLive, interval: 2_000 },
    ...(includeMonitorDetails
      ? [
          { run: pollProcesses, interval: 2_000 },
          { run: pollHistory, interval: 2_000 },
        ]
      : []),
    { run: pollTemp, interval: 6_000 },
    { run: loadDisks, interval: 30_000 },
  ];

  async function refresh() {
    if (document.hidden) return;
    await Promise.all([loadHost(), ...polls.map(({ run }) => run())]);
  }

  function stopTimers() {
    timers.forEach((timer) => window.clearInterval(timer));
    timers = [];
  }

  function onVisibility() {
    stopTimers();
    if (document.hidden) return;
    void refresh();
    timers = polls.map(({ run, interval }) => window.setInterval(run, interval));
  }

  onMounted(() => {
    onVisibility();
    document.addEventListener("visibilitychange", onVisibility);
  });
  onUnmounted(() => {
    stopTimers();
    document.removeEventListener("visibilitychange", onVisibility);
  });

  return {
    stats,
    processes,
    processError,
    cpuHistory,
    historyError,
    host,
    disks,
    cpuTemp,
    error,
    diskStatus,
    refresh,
  };
}
