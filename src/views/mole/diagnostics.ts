import { formatCapacity } from "@/views/monitor/shared";

export interface MoleProcess {
  pid: number;
  name: string;
  cpu?: number;
  memory?: number;
  memory_bytes?: number;
}

interface MoleZombieParent {
  pid: number;
  name: string;
  count: number;
}

interface MoleGpu {
  name: string;
  usage?: number;
  core_count?: number;
  memory_used?: number;
  memory_total?: number;
}

interface MoleBattery {
  percent?: number;
  status?: string;
  time_left?: string;
  health?: string;
  cycle_count?: number;
  capacity?: number;
}

interface MoleThermal {
  cpu_temp?: number;
  gpu_temp?: number;
  battery_temp?: number;
  fan_speed?: number;
  fan_count?: number;
  system_power?: number;
}

export interface MoleSnapshot {
  collected_at?: string;
  hardware?: { model?: string };
  gpu?: MoleGpu[] | null;
  batteries?: MoleBattery[] | null;
  thermal?: MoleThermal;
  memory?: { pressure?: string; total?: number };
  disks?: { mount: string; smart_status?: string }[] | null;
  disk_io?: { read_rate?: number; write_rate?: number };
  network?: { name: string; rx_rate_mbs?: number; tx_rate_mbs?: number }[] | null;
  top_processes?: MoleProcess[] | null;
  process_collected_at?: string;
  process_stale?: boolean;
  zombie_count?: number;
  zombie_parents?: MoleZombieParent[] | null;
  zombie_parents_complete?: boolean;
}

function hasMeasurement(value: number | undefined): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

export function percent(value: number | undefined): string {
  return hasMeasurement(value) ? `${value.toFixed(1)}%` : "—";
}

export function rounded(value: number | undefined, suffix = "", min = 0): string {
  return hasMeasurement(value) && value >= min ? `${Math.round(value)}${suffix}` : "—";
}

export function rate(value: number | undefined): string {
  if (!hasMeasurement(value)) return "—";
  return value < 1 ? `${(value * 1024).toFixed(1)} KiB/s` : `${value.toFixed(1)} MiB/s`;
}

export function watts(value: number | undefined): string {
  return hasMeasurement(value) && value > 0 ? `${value.toFixed(1)} W` : "—";
}

export function processMemory(process: MoleProcess): string {
  if (hasMeasurement(process.memory_bytes)) return formatCapacity(process.memory_bytes, true);
  return percent(process.memory);
}

export function clockTime(iso: string | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "" : date.toLocaleTimeString("zh-CN", { hour12: false });
}

export function memoryPressure(pressure: string | undefined): string {
  if (pressure === "normal") return "正常";
  if (pressure === "warn" || pressure === "warning") return "偏高";
  if (pressure === "critical") return "紧张";
  return "—";
}
