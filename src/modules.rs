use std::path::{Path, PathBuf};

pub fn detect_phpx_module_root(handler_path: &str) -> Option<PathBuf> {
    let path = Path::new(handler_path);
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    let mut current = start;
    loop {
        let candidate = current.join("deka.lock");
        if candidate.exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }

    // Fallback for ad-hoc execution outside a project: derive root from
    // binary path (.../target/release/cli -> repo root).
    if let Ok(exe) = std::env::current_exe() {
        let resolved_exe = exe.canonicalize().unwrap_or(exe);
        if let Some(release_dir) = resolved_exe.parent() {
            if let Some(target_dir) = release_dir.parent() {
                if let Some(repo_root) = target_dir.parent() {
                    let lock_path = repo_root.join("deka.lock");
                    if lock_path.exists() {
                        return Some(repo_root.to_path_buf());
                    }
                }
            }
        }
    }

    None
}

pub fn ensure_phpx_module_root_env(handler_path: &str) {
    if std::env::var("PHPX_MODULE_ROOT").is_ok() {
        return;
    }
    if let Some(root) = detect_phpx_module_root(handler_path) {
        unsafe {
            std::env::set_var("PHPX_MODULE_ROOT", root);
        }
    }
}
