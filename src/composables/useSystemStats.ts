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
export function useSystemStats() {
  const stats = shallowRef<SystemStats | null>(null);
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

  return { stats, host, disks, cpuTemp, error, diskStatus, refresh };
}
