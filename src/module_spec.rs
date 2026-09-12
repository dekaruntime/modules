use std::path::{Path, PathBuf};

pub fn is_bare_module_specifier(spec: &str) -> bool {
    !spec.is_empty()
        && !spec.starts_with("./")
        && !spec.starts_with("../")
        && !spec.starts_with('/')
        && !spec.starts_with("http://")
        && !spec.starts_with("https://")
        && !spec.starts_with("file://")
}

/// Authoritative dsc prefix vocabulary (dsc#167), owned here for both consumers.
/// Bare-specifier prefixes that name stdlib module families. Both the project
/// gate (`is_stdlib_module_spec`) and the browser import map emitted by
/// `deka build` derive their prefix lists from here so the two cannot drift:
/// the gate decides which specifiers are stdlib, the map decides where the
/// browser loads them from.
pub const STDLIB_SPEC_PREFIXES: &[&str] = &["component/", "deka/", "encoding/", "db/"];

/// Exhaustive stdlib names, apart from the families in [`STDLIB_SPEC_PREFIXES`].
/// dsc#167: `@deka/` is an alias, never a wildcard; `ui` is no longer stdlib.
/// This vocabulary is distinct from the compiler-provided export contracts in
/// [`CLOSED_STDLIB_MODULES`].
pub const STDLIB_MODULE_NAMES: &[&str] = &[
    "json", "postgres", "mysql", "sqlite", "bytes", "buffer", "http", "tcp", "tls", "fs", "crypto",
    "jwt", "test", "cookies", "auth", "db", "time", "io", "math",
];

/// Shared compiler/gate stdlib membership (dsc#167).
pub fn is_stdlib_module_spec(spec: &str) -> bool {
    let spec = spec.trim();
    if !is_bare_module_specifier(spec) {
        return false;
    }
    let bare = spec.strip_prefix("@deka/").unwrap_or(spec);
    STDLIB_MODULE_NAMES.contains(&bare)
        || STDLIB_SPEC_PREFIXES
            .iter()
            .any(|prefix| bare.starts_with(prefix))
}

/// Closed, toolchain-provided stdlib modules and the exact named exports each
/// one guarantees. Unlike every other stdlib module, these have no package
/// under `ds_modules/`: the dsc compiler lowers their imports to local
/// bindings in the emitted JavaScript, so requiring a package here would
/// recreate the ambient-global hole they exist to close. Keep in lockstep
/// with the compiler's stdlib surface (dsc#142: `math` exposes exactly `PI`).
pub const CLOSED_STDLIB_MODULES: &[(&str, &[&str])] = &[("math", &["PI"])];

/// Canonical graph id (`math`) when `spec` names a closed stdlib module,
/// accepting both the bare and `@deka/` spellings.
pub fn closed_stdlib_module_id(spec: &str) -> Option<&'static str> {
    let bare = spec.trim().strip_prefix("@deka/").unwrap_or(spec.trim());
    CLOSED_STDLIB_MODULES
        .iter()
        .find(|(name, _)| *name == bare)
        .map(|(name, _)| *name)
}

/// Whether `spec` names a closed, toolchain-provided stdlib module.
pub fn is_closed_stdlib_module_spec(spec: &str) -> bool {
    closed_stdlib_module_id(spec).is_some()
}

/// Guaranteed named exports of a closed stdlib module, by canonical id.
pub fn closed_stdlib_module_exports(module_id: &str) -> Option<&'static [&'static str]> {
    CLOSED_STDLIB_MODULES
        .iter()
        .find(|(name, _)| *name == module_id)
        .map(|(_, exports)| *exports)
}

pub fn module_spec_aliases(spec: &str) -> Vec<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(2);
    out.push(trimmed.to_string());

    if let Some(rest) = trimmed.strip_prefix("@deka/") {
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
        return out;
    }

    if is_bare_module_specifier(trimmed) && !trimmed.starts_with('@') {
        out.push(format!("@deka/{}", trimmed));
    }

    out
}

pub fn canonical_php_package_spec(spec: &str) -> Option<String> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_summoned_js_module_spec(trimmed) {
        return None;
    }
    if trimmed.starts_with('@') {
        return Some(trimmed.to_string());
    }
    // Package specs are @scope/name. For convenience, only simple unscoped
    // tokens map to @deka/<name>; nested import paths (e.g. component/router)
    // are import-time aliases, not package names.
    if is_bare_module_specifier(trimmed) && !trimmed.contains('/') {
        return Some(format!("@deka/{}", trimmed));
    }
    None
}

/// Whether `name` is a package identity that can safely be used as a module
/// directory or a local-link key.
pub fn is_valid_package_name(name: &str) -> bool {
    if !name.starts_with('@') {
        return false;
    }

    let mut parts = name.split('/');
    let Some(scope) = parts.next() else {
        return false;
    };
    let Some(package) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || scope.len() <= 1 || package.is_empty() {
        return false;
    }

    scope[1..]
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && package
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// Reserved routing prefix for vendored, summoned JavaScript (rfd#39).
pub const SUMMONED_JS_SPEC_PREFIX: &str = "@js/";

/// Recognize the routing class even when its name is invalid, so malformed
/// summoned imports cannot fall through to registry or DekaScript resolution.
pub fn is_summoned_js_module_spec(spec: &str) -> bool {
    spec.trim().starts_with(SUMMONED_JS_SPEC_PREFIX)
}

/// Validated unscoped vendor name. The full identity shares the package-name
/// validator; subpaths, nested scopes, and traversal are not supported.
pub fn summoned_js_package_name(spec: &str) -> Option<&str> {
    let spec = spec.trim();
    is_valid_package_name(spec)
        .then(|| spec.strip_prefix(SUMMONED_JS_SPEC_PREFIX))
        .flatten()
}

/// Resolve `@js/<name>` beneath the project's `js_modules/<name>/`.
///
/// A package.json `module` entry takes precedence over `main`; `index.mjs`
/// is used only when neither field exists (or package.json is absent).
/// Explicit entries must be nonempty strings naming existing files: a broken
/// entry or malformed manifest is an error, never a silent fallback. Entries
/// must stay within the vendor directory, including after symlink resolution.
/// No extension probing, exports-map lookup, or network resolution occurs.
pub fn resolve_summoned_js_module_file(project_root: &Path, spec: &str) -> Result<PathBuf, String> {
    let name = summoned_js_package_name(spec)
        .ok_or_else(|| format!("invalid summoned JavaScript specifier: {spec}"))?;
    let dir = project_root.join("js_modules").join(name);
    let manifest = dir.join("package.json");
    let entry = match std::fs::read_to_string(&manifest) {
        Ok(raw) => {
            let json: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|error| format!("invalid {}: {error}", manifest.display()))?;
            let object = json
                .as_object()
                .ok_or_else(|| format!("{} must be an object", manifest.display()))?;
            match object.get("module").or_else(|| object.get("main")) {
                Some(value) => value
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        format!("{} entry must be a nonempty string", manifest.display())
                    })?
                    .to_string(),
                None => "index.mjs".to_string(),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "index.mjs".to_string(),
        Err(error) => return Err(format!("cannot read {}: {error}", manifest.display())),
    };
    let entry_path = Path::new(&entry);
    if entry.contains('\\')
        || entry_path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "JavaScript entry escapes {}: {entry}",
            dir.display()
        ));
    }
    let path = dir.join(entry_path);
    let root = dir.canonicalize().map_err(|error| {
        format!(
            "JavaScript package not vendored at {}: {error}",
            dir.display()
        )
    })?;
    let resolved = path.canonicalize().map_err(|error| {
        format!(
            "JavaScript entry not vendored at {}: {error}",
            path.display()
        )
    })?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err(format!(
            "JavaScript entry is not a file within {}: {entry}",
            dir.display()
        ));
    }
    Ok(path)
}

/// Source extensions DekaScript resolution recognizes, in search order.
///
/// `ds_source_candidates`, the missing-import help scanner, module-graph
/// ids, and the integrity walker all derive from this list so they cannot
/// disagree on what a module file is (deka#622 finding G). `.phpx` is not
/// on it: that layer was deleted in #601.
pub const DS_SOURCE_EXTENSIONS: &[&str] = &["ds", "dsx"];

pub fn is_ds_source_extension(ext: &str) -> bool {
    DS_SOURCE_EXTENSIONS.contains(&ext)
}

pub fn is_ds_source_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_ds_source_extension)
}

/// Graph / help-text id for a path relative to `ds_modules`.
///
/// Strips `/index.{ext}` then `.{ext}` for each [`DS_SOURCE_EXTENSIONS`]
/// entry so `core/index.ds` and `core.ds` both become `core`. Leftover
/// `.phpx` is not a source extension and is left as-is.
pub fn ds_module_id_from_rel(rel: &str) -> String {
    let normalized = rel.replace('\\', "/");
    for ext in DS_SOURCE_EXTENSIONS {
        let index_suffix = format!("/index.{ext}");
        if let Some(stripped) = normalized.strip_suffix(&index_suffix) {
            return stripped.to_string();
        }
    }
    for ext in DS_SOURCE_EXTENSIONS {
        let suffix = format!(".{ext}");
        if let Some(stripped) = normalized.strip_suffix(&suffix) {
            return stripped.to_string();
        }
    }
    normalized
}

/// DekaScript source files for an import path (`foo` or `foo.ds`).
///
/// Resolution algorithm (deka#241; keep every resolver on this list):
///
/// 1. Relative (`./foo`, `../bar`) is joined to the importing file's directory.
/// 2. `@/foo` is joined to the project root (parent of `ds_modules`).
/// 3. Bare / `@deka/` specs are joined to `ds_modules` (plus the `@deka/` alias).
/// 4. Then:
///    - `foo.ds` → that file
///    - extensionless `foo` → `foo.ds`, then `foo/index.ds`
///    - any other extension (including `.phpx`) → no DekaScript source
///
/// Check-time (`validate_module_resolution`) and run-time (dsc bundle stage,
/// ESM loader, `deka build`, WASM project, style-graph walker) must use this
/// list so they cannot disagree. The candidate files are derived from
/// [`DS_SOURCE_EXTENSIONS`].
pub fn ds_source_candidates(base: &Path) -> Vec<PathBuf> {
    match base.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if is_ds_source_extension(ext) => vec![base.to_path_buf()],
        Some(_) => Vec::new(),
        None => {
            let mut out = Vec::with_capacity(DS_SOURCE_EXTENSIONS.len() * 2);
            for ext in DS_SOURCE_EXTENSIONS {
                out.push(base.with_extension(*ext));
            }
            for ext in DS_SOURCE_EXTENSIONS {
                out.push(base.join(format!("index.{ext}")));
            }
            out
        }
    }
}

/// First existing file from [`ds_source_candidates`].
pub fn resolve_ds_source_file(base: &Path) -> Option<PathBuf> {
    ds_source_candidates(base)
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::{
        STDLIB_SPEC_PREFIXES, canonical_php_package_spec, closed_stdlib_module_exports,
        closed_stdlib_module_id, ds_source_candidates, is_closed_stdlib_module_spec,
        is_valid_package_name, module_spec_aliases,
    };
    use std::path::Path;

    #[test]
    fn bare_spec_includes_deka_alias() {
        assert_eq!(module_spec_aliases("json"), vec!["json", "@deka/json"]);
    }

    #[test]
    fn deka_scope_includes_bare_alias() {
        assert_eq!(
            module_spec_aliases("@deka/json"),
            vec!["@deka/json", "json"]
        );
    }

    #[test]
    fn scoped_non_deka_has_no_alias() {
        assert_eq!(module_spec_aliases("@sami/tool"), vec!["@sami/tool"]);
    }

    #[test]
    fn canonicalizes_bare_packages_to_deka_scope() {
        assert_eq!(
            canonical_php_package_spec("json"),
            Some("@deka/json".to_string())
        );
    }

    #[test]
    fn does_not_map_nested_import_paths_to_package_specs() {
        assert_eq!(canonical_php_package_spec("component/router"), None);
    }

    #[test]
    fn package_names_are_scoped_and_path_safe() {
        assert!(is_valid_package_name("@deka/crypto"));
        assert!(is_valid_package_name("@sami/my-package_2"));
        assert!(!is_valid_package_name("crypto"));
        assert!(!is_valid_package_name("@deka/../escape"));
        assert!(!is_valid_package_name("@deka/crypto/extra"));
    }

    #[test]
    fn ds_candidates_are_file_then_index() {
        let base = Path::new("src/foo");
        // .dsx joined the list in 516ede7af (RFD 24 phase 2) and this
        // assertion was not updated, so runtime_core has been red on main
        // since. Order matters: file before index, .ds before .dsx.
        assert_eq!(
            ds_source_candidates(base),
            vec![
                Path::new("src/foo.ds").to_path_buf(),
                Path::new("src/foo.dsx").to_path_buf(),
                Path::new("src/foo/index.ds").to_path_buf(),
                Path::new("src/foo/index.dsx").to_path_buf(),
            ]
        );
    }

    #[test]
    fn ds_candidates_keep_explicit_ds() {
        let base = Path::new("src/foo.ds");
        assert_eq!(
            ds_source_candidates(base),
            vec![Path::new("src/foo.ds").to_path_buf()]
        );
    }

    #[test]
    fn ds_candidates_reject_phpx() {
        assert!(ds_source_candidates(Path::new("src/foo.phpx")).is_empty());
        assert!(ds_source_candidates(Path::new("src/foo.php")).is_empty());
    }

    #[test]
    fn closed_stdlib_module_accepts_bare_and_scoped_spellings() {
        assert_eq!(closed_stdlib_module_id("math"), Some("math"));
        assert_eq!(closed_stdlib_module_id("@deka/math"), Some("math"));
        assert_eq!(closed_stdlib_module_id("  math  "), Some("math"));
        assert_eq!(closed_stdlib_module_id("math/extra"), None);
        assert_eq!(closed_stdlib_module_id("json"), None);
        assert!(is_closed_stdlib_module_spec("math"));
        assert!(!is_closed_stdlib_module_spec("io"));
    }

    #[test]
    fn closed_stdlib_module_exports_match_the_compiler_contract() {
        assert_eq!(closed_stdlib_module_exports("math"), Some(&["PI"][..]));
        assert_eq!(closed_stdlib_module_exports("io"), None);
    }

    #[test]
    fn stdlib_prefixes_carry_deka_alias() {
        // `deka build`'s browser import map resolves each prefix through the
        // @deka alias (the layout `deka install` writes); a prefix without an
        // alias would panic the map emitter, so pin the invariant here.
        for prefix in STDLIB_SPEC_PREFIXES {
            let aliases = module_spec_aliases(prefix);
            assert!(
                aliases.iter().any(|alias| alias.starts_with("@deka/")),
                "{prefix} must derive a @deka alias: {aliases:?}"
            );
        }
    }
}
