export const moleActions = [
  {
    id: "clean",
    label: "深度清理",
    icon: "i-ph-broom-duotone",
    description: "扫描系统和应用缓存、日志、废纸篓及卸载残留。",
    confirmation: "Mole 可能永久删除符合清理规则的文件，包括废纸篓内容。请先查看预览。",
  },
  {
    id: "uninstall",
    label: "应用卸载",
    icon: "i-ph-app-window-duotone",
    description: "选择已安装应用，检查可一并移除的关联文件。",
    confirmation: "在 Mole 中选择应用并审核关联文件。确认后，Mole 会卸载所选应用。",
  },
  {
    id: "optimize",
    label: "系统维护",
    icon: "i-ph-wrench-duotone",
    description: "维护 Finder、Quick Look、网络缓存及系统数据库。",
    confirmation: "Mole 可能立即变更系统设置、刷新缓存或维护数据库。请先查看预览。",
  },
  {
    id: "purge",
    label: "开发产物清理",
    icon: "i-ph-code-duotone",
    description: "查找 node_modules、target 等可重新生成的项目产物。",
    confirmation: "在 Mole 中选择并审核项目产物。确认后，所选目录会被永久删除。",
  },
  {
    id: "installer",
    label: "安装包清理",
    icon: "i-ph-package-duotone",
    description: "查找 DMG、PKG、XIP 等安装文件并显示所占空间。",
    confirmation: "在 Mole 中选择并审核安装包。确认后，所选文件会被永久删除。",
  },
  {
    id: "slim",
    label: "应用瘦身",
    icon: "i-ph-scissors-duotone",
    description: "在 Apple Silicon 上移除所选通用应用的 Intel 代码。",
    confirmation: "Mole 会替换你审核并确认的通用应用，原版将移至废纸篓。",
  },
] as const;

export type MoleAction = (typeof moleActions)[number];

export interface MoleRequest {
  action: MoleAction;
  dryRun: boolean;
}

export interface MoleInfo {
  path: string | null;
  version: string | null;
  home: string;
  bundled: boolean;
}

export function moleActionLabel(command: string): string {
  return moleActions.find((action) => action.id === command)?.label ?? command;
}
