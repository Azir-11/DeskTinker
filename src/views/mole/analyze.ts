import { formatCapacity } from "@/views/monitor/shared";

type ScanStatus = "complete" | "partial" | "unavailable";

export interface MoleFile {
  name: string;
  path: string;
  size: number;
}

export interface MoleEntry extends MoleFile {
  is_dir: boolean;
  scan_status?: ScanStatus;
}

export interface MoleScan {
  path: string;
  scan_status?: ScanStatus;
  entries: MoleEntry[] | null;
  large_files?: MoleFile[] | null;
  total_size: number;
  total_files?: number;
}

export function parentPath(path: string): string {
  const withoutSlash = path.replace(/\/+$/, "");
  if (!withoutSlash) return "";
  return withoutSlash.slice(0, withoutSlash.lastIndexOf("/")) || "/";
}

export function sizeText(bytes: number, status?: ScanStatus): string {
  if (status === "unavailable" || !Number.isFinite(bytes) || bytes < 0) return "未知";
  const size = formatCapacity(bytes, false);
  return status === "partial" ? `${size}+` : size;
}

export function sizeShare(entry: MoleEntry, total: number): number {
  return total > 0 && entry.scan_status !== "unavailable"
    ? Math.min(100, Math.max(0, (entry.size / total) * 100))
    : 0;
}
