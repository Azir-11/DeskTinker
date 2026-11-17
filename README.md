# DeskTinker

面向 macOS 和 Windows 的桌面工具箱，提供系统配置、系统监控和常用开发工具。

项目原名 HostGuard。已发布的旧版安装包仍会使用 HostGuard 文件名。

> Tauri 2 · Vue 3.5 · TypeScript · Vite · Pinia · Vue Router · Naive UI · UnoCSS · CodeMirror 6 · oxlint / oxfmt

## 功能

- **Hosts 管理**：读取 / 编辑系统 Hosts 文件，启用停用、搜索、一次性授权后直接保存（自动备份）。
- **Shell 配置**：语法高亮编辑与保存前备份，支持 macOS 的 Zsh 配置以及 Windows 的 PowerShell 配置和 cmd 启动脚本。
- **系统监控**：查看 CPU 历史、温度、可用内存及各卷的存储用量。进程列表支持按 CPU 或内存占用排序，并可按进程名或 PID 搜索。
- **macOS 菜单栏**：运行 DeskTinker 时显示 CPU 占用、可用内存及 CPU 温度。关闭窗口后继续采样，从菜单栏可重新打开监控或退出。
- **Mole 工具（macOS 12+）**：随应用附带。扩展监控、按目录分析存储、已安装应用和操作历史。在应用内终端预览或执行清理、卸载、维护、开发产物清理、安装包清理和通用应用瘦身。
- **开发工具**：查询和结束占用端口的进程、时间戳转换、人民币金额大小写转换、网页预览和 SSL 证书申请。
- 概览仪表盘、深 / 浅色主题和系统配置入口。

DeskTinker 运行时约每 2 秒采样 CPU 和内存。CPU 历史保存在本机，默认保留 7 天，可在「设置 → 系统监控」调整为 1 至 30 天。趋势图默认显示最近 5 分钟，可切换到 1、3 或 6 小时，也可通过「按日期查看」选择起止时间查询保留期内的记录。退出或休眠期间留空。macOS 温度约每 6 秒读取一次。如果原生读取不到 CPU 温度，DeskTinker 会尝试调用内置 [Mole](https://github.com/tw93/Mole) 的只读状态命令 `mo status --json`，每 30 秒最多查询一次。

macOS 每 5 秒检查一次应用 CPU 用量。同一应用（含其辅助进程）持续 30 秒达到单个逻辑核心的 80% 时，菜单栏显示红点并发送系统通知。同一次高占用只提醒一次。通知需在 macOS 中允许，当前告警也可直接在菜单栏菜单查看。

macOS 安装包自带 Mole CLI。安装 DeskTinker 后，从侧边栏打开「Mole 工具」即可使用。扩展监控和磁盘分析按需运行，维护页将 `--dry-run` 预览与实际执行分开，并保留 Mole 的交互提示。

详细路线图见 [`TASKS.md`](./TASKS.md)。

## 开发

```bash
pnpm install
pnpm tauri dev      # 启动桌面应用（热更新）
```

常用脚本：`pnpm build`（前端构建 + 类型检查）、`pnpm lint`（oxlint）、`pnpm format`（oxfmt）。

## 打包为 macOS 应用

```bash
pnpm tauri build    # 或 pnpm app:build
```

产物位于：

- 应用：`src-tauri/target/release/bundle/macos/DeskTinker.app`
- 安装包：`src-tauri/target/release/bundle/dmg/DeskTinker_<版本>_<架构>.dmg`
  （Apple Silicon 为 `aarch64`，Intel 为 `x64`）

macOS 构建需要 Go 1.26.10 生成 Mole 的依赖源码包。构建会校验并打包固定版本的 Mole，随包资源位于 `DeskTinker.app/Contents/Resources/mole/`，包含 Mole CLI、GPL-3.0 许可和对应源码。安装后的 DeskTinker 可直接运行这些工具。

## 安装

从 [GitHub Releases](https://github.com/Azir-11/DeskTinker/releases/latest) 下载对应系统的安装包。

### Windows

运行 `HostGuard_<版本>_x64-setup.exe`。安装器沿用原名称，以便识别并升级已有安装。

### macOS

**方式一（推荐，DMG）**：双击打开 `.dmg`，把 `DeskTinker` 拖到 `Applications` 文件夹。

**方式二**：直接把 `DeskTinker.app` 拷贝到「应用程序」。

### 关于 Gatekeeper（未签名应用）

当前未做 Apple Developer 签名 / 公证。**本机自行构建**的应用可直接打开。若是从别处**下载**的 `.dmg`，macOS 会拦截，按需任选其一：

- 右键 App →「打开」→ 在弹窗中再次「打开」；或
- 终端执行：`xattr -cr /Applications/DeskTinker.app`

如需面向他人分发，请配置 Apple Developer ID 签名 + 公证（在 `src-tauri/tauri.conf.json` 的 `bundle.macOS` 中设置 `signingIdentity`，并配合 `tauri build` 的公证环境变量）。

## 权限说明

修改系统 Hosts 文件需要管理员权限：应用会**一次性**弹出系统授权（为当前用户添加写入 ACL），之后保存无需重复输入密码。`~` 下的 Shell 配置为用户文件，无需授权。
