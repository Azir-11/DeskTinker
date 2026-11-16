import { CPU_HISTORY_LIMIT_MS } from "@/composables/useSystemStats";
import type { CpuSample, ProcessStats } from "@/composables/useSystemStats";

export type ProcessSort = "cpu_usage" | "memory";

export function rankProcesses(
  processes: readonly ProcessStats[],
  query: string,
  sort: ProcessSort,
): ProcessStats[] {
  const keyword = query.trim().toLowerCase();
  const matches = keyword
    ? processes.filter(
        ({ name, pid }) => name.toLowerCase().includes(keyword) || String(pid) === keyword,
      )
    : [...processes];

  return matches.sort((a, b) => {
    const usageDiff = (b[sort] ?? -1) - (a[sort] ?? -1);
    return usageDiff || b.memory - a.memory || a.pid - b.pid;
  });
}

export const cpuRanges = [
  { label: "5 分钟", duration: 5 * 60_000 },
  { label: "1 小时", duration: 60 * 60_000 },
  { label: "3 小时", duration: 3 * 60 * 60_000 },
  { label: "6 小时", duration: CPU_HISTORY_LIMIT_MS },
] as const;

export type CpuRange = (typeof cpuRanges)[number];

export function cpuSegments(samples: readonly CpuSample[], start: number): CpuSample[][] {
  const segments: CpuSample[][] = [];
  let segment: CpuSample[] | undefined;
  for (const sample of samples) {
    if (sample.time < start) continue;
    // 重启、休眠和采样中断都会断开曲线。
    if (
      !segment ||
      sample.starts_segment ||
      sample.time - segment[segment.length - 1].time > 6_000
    ) {
      segment = [];
      segments.push(segment);
    }
    segment.push(sample);
  }
  return segments;
}

export function clampPercent(value: number): number {
  return Math.max(0, Math.min(100, value));
}

export function usedPercent(total: number, available: number): number {
  return total > 0 ? clampPercent(((total - available) / total) * 100) : 0;
}

export function formatCapacity(bytes: number, binary: boolean): string {
  const base = binary ? 1024 : 1000;
  const units = binary ? ["B", "KiB", "MiB", "GiB", "TiB"] : ["B", "KB", "MB", "GB", "TB"];
  const unitIndex = units.findIndex((_, index) => bytes < base ** (index + 1));
  const exponent = unitIndex < 0 ? units.length - 1 : unitIndex;
  const value = bytes / base ** exponent;
  return `${value.toFixed(exponent < 3 ? 0 : 1)} ${units[exponent]}`;
}
