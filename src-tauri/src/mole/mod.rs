pub(crate) mod read;
pub(crate) mod terminal;

use std::{
    env,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};

const INSTALL_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];
const BUNDLE_ERROR: &str = "内置 Mole 文件缺失或无法执行。请重新安装 HostGuard。";

struct MoleExecutable {
    path: PathBuf,
    bundled: bool,
}

pub(crate) fn executable(app: &AppHandle) -> Result<PathBuf, String> {
    discover(app)?
        .map(|mole| mole.path)
        .ok_or_else(|| "找不到 Mole。请使用包含 Mole 的 HostGuard 安装包。".to_string())
}

fn discover(app: &AppHandle) -> Result<Option<MoleExecutable>, String> {
    if env::current_exe().is_ok_and(|path| is_packaged_app(&path)) {
        let resources = app
            .path()
            .resource_dir()
            .map_err(|_| BUNDLE_ERROR.to_string())?;
        let path = resources.join("mole/mo");
        if !is_executable(&path) {
            return Err(BUNDLE_ERROR.to_string());
        }
        // Mole 官方发行版的采集器从 macOS 12 起受支持。
        if sysinfo::System::os_version()
            .as_deref()
            .and_then(|version| version.split('.').next())
            .and_then(|major| major.parse::<u32>().ok())
            .is_some_and(|major| major < 12)
        {
            return Err("内置 Mole 需要 macOS 12 或更高版本。".to_string());
        }
        return Ok(Some(MoleExecutable {
            path,
            bundled: true,
        }));
    }

    Ok(external_executable(app).map(|path| MoleExecutable {
        path,
        bundled: false,
    }))
}

fn is_packaged_app(executable: &Path) -> bool {
    executable
        .parent()
        .filter(|directory| directory.ends_with("Contents/MacOS"))
        .and_then(|directory| directory.ancestors().nth(2))
        .and_then(Path::extension)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn external_executable(app: &AppHandle) -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    let home = app.path().home_dir().ok();
    // Finder 的 PATH 不一定包含 Homebrew 或脚本安装目录。
    env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .chain(INSTALL_DIRS.map(PathBuf::from))
        .chain(
            home.iter()
                .flat_map(|home| [home.join(".local/bin"), home.join("bin")]),
        )
        .map(|dir| dir.join("mo"))
        .find(|path| is_executable(path))
}

pub(crate) fn command_path(executable: &Path) -> OsString {
    let inherited = env::var_os("PATH").unwrap_or_default();
    let mut dirs = env::split_paths(&inherited)
        .filter(|dir| dir.is_absolute())
        .collect::<Vec<_>>();
    if let Some(directory) = executable.parent() {
        if directory.ends_with("Contents/Resources/mole") {
            dirs.retain(|existing| existing != directory);
            dirs.insert(0, directory.to_owned());
        }
    }
    for dir in executable
        .parent()
        .into_iter()
        .chain(INSTALL_DIRS.map(Path::new))
        .chain(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(Path::new))
    {
        if !dirs.iter().any(|existing| existing == dir) {
            dirs.push(dir.to_owned());
        }
    }
    env::join_paths(dirs).unwrap_or(inherited)
}

#[cfg(test)]
mod tests {
    use super::is_packaged_app;
    use std::path::Path;

    #[test]
    fn packaged_app_is_identified_even_when_resources_are_missing() {
        assert!(is_packaged_app(Path::new(
            "/Applications/HostGuard.app/Contents/MacOS/hostguard"
        )));
        assert!(!is_packaged_app(Path::new(
            "/project/src-tauri/target/debug/hostguard"
        )));
    }
}
