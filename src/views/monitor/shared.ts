export interface CpuSample {
  time: number;
  usage: number;
}

export const CPU_HISTORY_MS = 60_000;

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
