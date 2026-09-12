//! dsc-compatible installed-package fsGraph hashing.
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// SHA-256 of sorted relative paths, NUL, file bytes, and newline per file.
/// Excludes dependency/build/cache trees and macOS metadata, as in dsc#167.
pub fn compute_fs_graph_hash(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_integrity_files(root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for path in files {
        let rel = path
            .strip_prefix(root)
            .map_err(|_| "failed to normalize integrity path".to_string())?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        hasher.update(rel_str.as_bytes());
        hasher.update(b"\0");
        let mut file = std::fs::File::open(&path)
            .map_err(|err| format!("failed to open {}: {err}", path.display()))?;
        let mut buf = [0u8; 8192];
        loop {
            let read = file
                .read(&mut buf)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        hasher.update(b"\n");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_integrity_files(current: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = std::fs::read_dir(current)
        .map_err(|err| format!("failed to read {}: {err}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("failed to read {}: {err}", current.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(
                name.as_ref(),
                "ds_modules"
                    | "php_modules"
                    | ".git"
                    | "target"
                    | "node_modules"
                    | "dist"
                    | ".deka"
                    | ".cache"
            ) {
                continue;
            }
            collect_integrity_files(&path, out)?;
        } else if path.is_file() {
            if name == ".DS_Store" || name.starts_with("._") {
                continue;
            }
            out.push(path);
        }
    }
    Ok(())
}
