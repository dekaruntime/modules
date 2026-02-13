use std::path::{Path, PathBuf};

pub fn detect_phpx_module_root_with<Exists, CurrentExe>(
    handler_path: &str,
    lock_exists: &Exists,
    current_exe: &CurrentExe,
) -> Option<PathBuf>
where
    Exists: Fn(&Path) -> bool,
    CurrentExe: Fn() -> Option<PathBuf>,
{
    let path = Path::new(handler_path);
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    let mut current = start;
    loop {
        let candidate = current.join("deka.lock");
        if lock_exists(&candidate) {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }

    // Fallback for ad-hoc execution outside a project: derive root from
    // binary path (.../target/release/cli -> repo root).
    if let Some(exe) = current_exe() {
        let resolved_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(release_dir) = resolved_exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                if let Some(repo_root) = target_dir.parent() {
                    let lock_path = repo_root.join("deka.lock");
                    if lock_exists(&lock_path) {
                        return Some(repo_root.to_path_buf());
                    }
                }
            }
        }
    }

    None
}

pub fn ensure_phpx_module_root_env_with<Exists, CurrentExe, Get, Set>(
    handler_path: &str,
    lock_exists: &Exists,
    current_exe: &CurrentExe,
    env_get: &Get,
    env_set: &mut Set,
)
where
    Exists: Fn(&Path) -> bool,
    CurrentExe: Fn() -> Option<PathBuf>,
    Get: Fn(&str) -> Option<String>,
    Set: FnMut(&str, &str),
{
    if env_get("PHPX_MODULE_ROOT").is_some() {
        return;
    }
    if let Some(root) = detect_phpx_module_root_with(handler_path, lock_exists, current_exe) {
        if let Some(root_str) = root.to_str() {
            env_set("PHPX_MODULE_ROOT", root_str);
        }
    }
}
