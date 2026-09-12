//! One gate for "is this project valid to run", shared by every entry point.
//!
//! This replaces three separate `ensure_project_layout` implementations — in
//! `pool::esm_loader`, `cli::build` and `runtime::js_pipeline` — that had the
//! same name and job but three different bypass rules and three copies of the
//! stdlib-specifier list. A fix applied to one was invisible to the others,
//! which is how the declared-dependency check ended up missing from all three
//! (deka#403, deka#414), and how `deka build` ended up not checking
//! `@deka/http` at all (see `is_stdlib_module_spec`).
//!
//! Bypasses are explicit parameters here. The `esm_loader` copy keyed its
//! bypass off a `DEKA_MODULE_ROOT` environment variable read in the middle of
//! the function, so one ambient variable disabled every check including the
//! lockfile requirement (deka#229).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::module_spec::{
    ds_source_candidates, is_bare_module_specifier, is_closed_stdlib_module_spec,
    is_summoned_js_module_spec, module_spec_aliases, resolve_summoned_js_module_file,
    summoned_js_package_name,
};
use crate::modules::resolve_modules_dir;

/// How strict to be, stated by the caller rather than inferred from ambient state.
#[derive(Debug, Clone)]
pub struct GateOptions {
    /// An external module root supplying the stdlib. When set to something other
    /// than the project root, the runtime provides these modules and the local
    /// `ds_modules/` tree is not required.
    pub module_root: Option<PathBuf>,
    /// Whether `deka.lock` must be present. Stdlib-only tenants deploy without
    /// one (deka#220); everything else requires it.
    pub require_lockfile: bool,
    /// What to call this entry point in error messages: "deka build",
    /// "deka run", "deka runtime".
    pub context: &'static str,
}

impl Default for GateOptions {
    fn default() -> Self {
        Self {
            module_root: None,
            require_lockfile: true,
            context: "deka",
        }
    }
}

// Retain the 0.2 public path while keeping vocabulary in module_spec.
pub use crate::module_spec::is_stdlib_module_spec;

/// Scan one source with the shared compiler-free scanner and apply the gate.
/// Consumers walking a graph should collect `ds_imports::paths` for each source
/// and pass the combined specifiers to [`validate_project`]. Compilation may
/// use a parser, but must not substitute a parser walk for this gate scan.
pub fn validate_project_source(
    project_root: &Path,
    source: &str,
    opts: &GateOptions,
) -> Result<(), String> {
    validate_project(project_root, &crate::ds_imports::paths(source), opts)
}

/// Locate the file a stdlib specifier resolves to under a modules directory.
///
/// Prefixed specifiers (`encoding/json`) also try the scoped layout
/// (`@deka/encoding/json`), since `deka install` writes packages under
/// `@deka/<pkg>/`.
pub fn resolve_module_file(modules_dir: &Path, spec: &str) -> Option<PathBuf> {
    if is_summoned_js_module_spec(spec) {
        return None;
    }
    let mut aliases = module_spec_aliases(spec);
    if spec.contains('/')
        && !spec.starts_with('@')
        && !spec.starts_with("./")
        && !spec.starts_with("../")
    {
        aliases.push(format!("@deka/{spec}"));
    }

    let mut candidates = Vec::new();
    for alias in aliases {
        candidates.extend(ds_source_candidates(&modules_dir.join(alias.as_str())));
    }

    candidates.into_iter().find(|path| path.is_file())
}

/// Validate a project against the imports its module graph actually contains.
///
/// Rules, in order:
///   1. summoned JS imports are declared, locked, and vendored (never waived)
///   2. `deka.lock` exists, unless the caller waived it
///   3. every package import is **declared** in `deka.json` dependencies
///   4. every package import resolves under ds_modules or a declared local link
///   5. installed packages have lock entries and match any recorded fsGraph hash
///
/// Compiler-provided math is exempt from package checks. External module roots
/// supply only recognized stdlib. A waived absent lock permits unlocked packages;
/// a present lock is always checked. Links waive installed-package lock/hash checks.
///
/// The stdlib declaration rule did not originally exist. Resolution was
/// satisfied by a directory happening to be present, so a package could import something it never
/// declared and install fine for whoever happened to have it.
pub fn validate_project(
    project_root: &Path,
    imports: &[String],
    opts: &GateOptions,
) -> Result<(), String> {
    validate_summoned_js_imports(project_root, imports, opts.context)?;

    let external_stdlib = opts
        .module_root
        .as_deref()
        .is_some_and(|root| root != project_root);

    let who = opts.context;

    if opts.require_lockfile && !external_stdlib {
        let lock_path = project_root.join("deka.lock");
        if !lock_path.is_file() {
            return Err(format!(
                "{who} requires deka.lock at project root: {}",
                lock_path.display()
            ));
        }
    }

    let package_imports: BTreeSet<String> = imports
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| {
            is_bare_module_specifier(s) && !s.starts_with("@/") && !is_summoned_js_module_spec(s)
        })
        .filter(|s| !(external_stdlib && is_stdlib_module_spec(s)))
        // Closed, toolchain-provided modules (dsc#142's `math`) ship inside
        // the compiler: there is intentionally no package to declare or
        // install, so stdlib declaration and installation do not apply to them.
        .filter(|s| !is_closed_stdlib_module_spec(s))
        .collect();

    if package_imports.is_empty() {
        return Ok(());
    }

    // Package declaration — declared in deka.json.
    let declared = declared_dependencies(project_root);
    let undeclared: Vec<&String> = package_imports
        .iter()
        .filter(|spec| !is_declared(spec, &declared))
        .collect();

    if !undeclared.is_empty() {
        let list: Vec<String> = undeclared.iter().map(|s| (*s).clone()).collect();
        let add: Vec<String> = list.iter().map(|s| bare_name(s).to_string()).collect();
        return Err(format!(
            "{who}: imported but not declared in deka.json: {}. Add {} with `deka add {}`.",
            list.join(", "),
            if list.len() == 1 { "it" } else { "them" },
            add.join(" ")
        ));
    }

    // A `deka link`ed package satisfies an import from a working tree, with
    // nothing installed (deka#470). Drop those before the on-disk rules below;
    // they are deliberately still subject to declaration, because a link changes
    // *where* a dependency comes from, not whether it is a dependency.
    //
    // A manifest naming a target that has been moved or deleted is an error,
    // not a fall-through to the installed copy — a stale link must never look
    // like it worked.
    let linked = crate::modules::read_linked_modules(project_root)
        .map_err(|error| format!("{who}: local package link is unusable: {error}"))?;
    let package_imports: BTreeSet<String> = package_imports
        .into_iter()
        .filter(|spec| !is_satisfied_by_link(spec, &linked))
        .collect();

    if package_imports.is_empty() {
        return Ok(());
    }

    // Package installation — present on disk.
    let modules_dir = resolve_modules_dir(project_root);
    if !modules_dir.is_dir() {
        return Err(format!(
            "{who} requires {} at project root when using package imports ({}). Run `deka install`.",
            modules_dir.display(),
            package_imports
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let missing: Vec<String> = package_imports
        .iter()
        .filter(|spec| resolve_module_file(&modules_dir, spec).is_none())
        .cloned()
        .collect();

    if missing.is_empty() {
        validate_package_integrity(project_root, &modules_dir, &package_imports, opts)
    } else {
        Err(format!(
            "{who}: declared but not installed under {}: {}. Run `deka install`.",
            modules_dir.display(),
            missing.join(", ")
        ))
    }
}

/// Lock package identity preserves foreign scopes and drops import subpaths.
fn lock_package_name(spec: &str) -> String {
    if spec.starts_with('@') {
        spec.split('/').take(2).collect::<Vec<_>>().join("/")
    } else {
        format!("@deka/{}", spec.split('/').next().unwrap_or(spec))
    }
}

fn validate_package_integrity(
    project_root: &Path,
    modules_dir: &Path,
    imports: &BTreeSet<String>,
    opts: &GateOptions,
) -> Result<(), String> {
    let who = opts.context;
    let lock_path = project_root.join("deka.lock");
    let raw = match std::fs::read_to_string(&lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !opts.require_lockfile => {
            return Ok(());
        }
        Err(error) => {
            return Err(format!(
                "{who}: cannot read {}: {error}",
                lock_path.display()
            ));
        }
    };
    let lock: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("{who}: invalid {}: {error}", lock_path.display()))?;
    let packages = lock
        .get("packages")
        .and_then(|value| value.as_object())
        .or_else(|| lock.get("php")?.get("packages")?.as_object());
    let identities: BTreeSet<String> = imports.iter().map(|spec| lock_package_name(spec)).collect();
    for package in identities {
        let aliases = module_spec_aliases(&package);
        let entry = packages
            .and_then(|packages| aliases.iter().find_map(|alias| packages.get(alias)))
            .ok_or_else(|| {
                format!("{who}: module '{package}' has no deka.lock entry. Run `deka install`.")
            })?;
        let (_, _, metadata, _): (String, String, serde_json::Value, String) =
            serde_json::from_value(entry.clone()).map_err(|error| {
                format!("{who}: invalid deka.lock entry for '{package}': {error}")
            })?;
        // Older locks can omit fsGraph. If present, malformed metadata must not
        // silently disable integrity verification.
        let Some(graph) = metadata.get("fsGraph").or_else(|| metadata.get("fs_graph")) else {
            continue;
        };
        let expected = graph
            .get("hash")
            .and_then(|hash| hash.as_str())
            .map(str::trim)
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|ch| ch.is_ascii_hexdigit()))
            .ok_or_else(|| format!("{who}: invalid fsGraph hash for '{package}' in deka.lock"))?;
        // Hash the same alias tree that resolution selected, even if both bare
        // and scoped package layouts exist.
        for spec in imports
            .iter()
            .filter(|spec| lock_package_name(spec) == package)
        {
            let resolved = resolve_module_file(modules_dir, spec).ok_or_else(|| {
                format!(
                    "{who}: module '{spec}' is no longer installed under {}",
                    modules_dir.display()
                )
            })?;
            let package_dir = module_spec_aliases(spec)
                .into_iter()
                .map(|alias| modules_dir.join(bare_package_path(&alias)))
                .find(|dir| dir.is_dir() && resolved.starts_with(dir))
                .ok_or_else(|| {
                    format!("{who}: module '{package}' has no installed package directory")
                })?;
            let actual = crate::integrity::compute_fs_graph_hash(&package_dir)
                .map_err(|error| format!("{who}: integrity mismatch for '{package}': {error}"))?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "{who}: integrity mismatch: module '{package}' in ds_modules/ does not match deka.lock. Run `deka install`."
                ));
            }
        }
    }
    Ok(())
}

fn bare_package_path(spec: &str) -> String {
    spec.split('/')
        .take(if spec.starts_with('@') { 2 } else { 1 })
        .collect::<Vec<_>>()
        .join("/")
}

/// Summoned dependencies are project-owned, even when a runtime supplies the
/// stdlib or waives its lockfile. Exact @js identities prevent scope aliases
/// and local links from satisfying a different trust class.
fn validate_summoned_js_imports(
    project_root: &Path,
    imports: &[String],
    who: &str,
) -> Result<(), String> {
    let imports: BTreeSet<&str> = imports
        .iter()
        .map(|spec| spec.trim())
        .filter(|spec| is_summoned_js_module_spec(spec))
        .collect();
    if imports.is_empty() {
        return Ok(());
    }
    for spec in &imports {
        if summoned_js_package_name(spec).is_none() {
            return Err(format!(
                "{who}: invalid summoned JavaScript specifier: {spec}"
            ));
        }
    }
    let declared = declared_dependencies(project_root);
    for spec in &imports {
        if !declared.contains(*spec) {
            return Err(format!(
                "{who}: {spec} imported but not declared in deka.json. Use `deka summon <source>`."
            ));
        }
    }
    let lock_path = project_root.join("deka.lock");
    let raw = std::fs::read_to_string(&lock_path).map_err(|error| {
        format!(
            "{who}: summoned JavaScript requires {}: {error}",
            lock_path.display()
        )
    })?;
    let lock: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("{who}: invalid {}: {error}", lock_path.display()))?;
    for spec in imports {
        if !lock
            .get("packages")
            .and_then(|value| value.as_object())
            .and_then(|packages| packages.get(spec))
            .is_some_and(|entry| {
                serde_json::from_value::<(String, String, serde_json::Value, String)>(entry.clone())
                    .is_ok()
            })
        {
            return Err(format!(
                "{who}: {spec} missing package entry in deka.lock. Use `deka summon <source>`."
            ));
        }
        resolve_summoned_js_module_file(project_root, spec)
            .map_err(|error| format!("{who}: {spec}: {error}. Use `deka summon <source>`."))?;
    }
    Ok(())
}

/// Dependency names from `deka.json`, normalised to their bare form so
/// `@deka/crypto` and `crypto` compare equal.
fn declared_dependencies(project_root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(raw) = std::fs::read_to_string(project_root.join("deka.json")) else {
        return out;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return out;
    };
    for key in ["dependencies", "devDependencies"] {
        if let Some(map) = json.get(key).and_then(|v| v.as_object()) {
            for name in map.keys() {
                out.insert(bare_name(name).to_string());
            }
        }
    }
    out
}

/// `@deka/crypto` and `crypto` are the same package; `component/foo` keeps its
/// first segment, which is what a manifest would name.
fn bare_name(spec: &str) -> &str {
    let spec = spec.trim();
    if is_summoned_js_module_spec(spec) {
        return spec;
    }
    if let Some(rest) = spec.strip_prefix("@deka/") {
        return rest.split('/').next().unwrap_or(rest);
    }
    if spec.starts_with('@') {
        return spec
            .match_indices('/')
            .nth(1)
            .map(|(end, _)| &spec[..end])
            .unwrap_or(spec);
    }
    spec.split('/').next().unwrap_or(spec)
}

fn is_declared(spec: &str, declared: &BTreeSet<String>) -> bool {
    declared.contains(bare_name(spec))
}

/// True when `spec` is provided by a `deka link`ed package — either the package
/// itself or a subpath within it.
fn is_satisfied_by_link(
    spec: &str,
    linked: &std::collections::BTreeMap<String, std::path::PathBuf>,
) -> bool {
    let spec = spec.trim();
    linked.keys().any(|package| {
        crate::module_spec::module_spec_aliases(package)
            .into_iter()
            .any(|alias| spec == alias || spec.starts_with(&format!("{alias}/")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn setup(manifest: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "deka.json", manifest);
        write(
            tmp.path(),
            "deka.lock",
            r#"{"lockfileVersion":1,"packages":{"@deka/crypto":["0.2.0","registry",{},"tarball"]}}"#,
        );
        let crypto = tmp.path().join("ds_modules/@deka/crypto");
        std::fs::create_dir_all(&crypto).unwrap();
        std::fs::write(crypto.join("index.ds"), "export fn noop() {}\n").unwrap();
        tmp
    }

    fn run(root: &Path, imports: &[&str]) -> Result<(), String> {
        let owned: Vec<String> = imports.iter().map(|s| s.to_string()).collect();
        validate_project(root, &owned, &GateOptions::default())
    }

    #[test]
    fn undeclared_import_is_rejected() {
        let tmp = setup(r#"{"name":"t","dependencies":{"@deka/crypto":"0.2.0"}}"#);
        let err = run(tmp.path(), &["crypto", "io"]).unwrap_err();
        assert!(err.contains("not declared"), "{err}");
        assert!(err.contains("io"), "{err}");
    }

    #[test]
    fn declared_and_installed_passes() {
        let tmp = setup(r#"{"name":"t","dependencies":{"@deka/crypto":"0.2.0"}}"#);
        run(tmp.path(), &["crypto"]).expect("should pass");
    }

    #[test]
    fn scoped_and_bare_names_are_the_same_package() {
        let tmp = setup(r#"{"name":"t","dependencies":{"crypto":"0.2.0"}}"#);
        run(tmp.path(), &["@deka/crypto"]).expect("bare declaration covers scoped import");
    }

    #[test]
    fn declared_but_not_installed_says_install() {
        let tmp = setup(r#"{"name":"t","dependencies":{"@deka/bytes":"0.2.1"}}"#);
        let err = run(tmp.path(), &["bytes"]).unwrap_err();
        assert!(err.contains("not installed"), "{err}");
    }

    #[test]
    fn missing_lockfile_is_rejected_but_waivable() {
        let tmp = setup(r#"{"name":"t","dependencies":{"@deka/crypto":"0.2.0"}}"#);
        std::fs::remove_file(tmp.path().join("deka.lock")).unwrap();
        assert!(run(tmp.path(), &["crypto"]).is_err());

        let opts = GateOptions {
            require_lockfile: false,
            ..GateOptions::default()
        };
        let owned = vec!["crypto".to_string()];
        validate_project(tmp.path(), &owned, &opts).expect("waived lockfile should pass");
    }

    #[test]
    fn external_module_root_skips_local_checks() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "deka.json", r#"{"name":"t"}"#);
        let opts = GateOptions {
            module_root: Some(PathBuf::from("/elsewhere")),
            ..GateOptions::default()
        };
        let owned = vec!["crypto".to_string()];
        validate_project(tmp.path(), &owned, &opts)
            .expect("external module root supplies the stdlib");
    }

    #[test]
    fn context_appears_in_the_message() {
        let tmp = setup(r#"{"name":"t","dependencies":{}}"#);
        std::fs::remove_file(tmp.path().join("deka.lock")).unwrap();
        let opts = GateOptions {
            context: "deka build",
            ..GateOptions::default()
        };
        let err = validate_project(tmp.path(), &[], &opts).unwrap_err();
        assert!(err.starts_with("deka build requires deka.lock"), "{err}");
    }

    #[test]
    fn closed_stdlib_module_needs_no_declaration_or_install() {
        let tmp = setup(r#"{"name":"t","dependencies":{}}"#);
        assert!(is_stdlib_module_spec("math"), "registered as stdlib");
        assert!(is_stdlib_module_spec("@deka/math"));
        run(tmp.path(), &["math"]).expect("toolchain-provided math must not require a package");
        run(tmp.path(), &["@deka/math"]).expect("scoped spelling is the same module");
    }

    // The three copies disagreed about these two specifiers; the shared gate
    // must treat both as stdlib or `deka build` keeps skipping them.
    #[test]
    fn converged_specifiers_are_stdlib() {
        assert!(is_stdlib_module_spec("http"), "js_pipeline listed it");
        assert!(is_stdlib_module_spec("@deka/http"), "build.rs skipped it");
        assert!(!is_stdlib_module_spec("@deka/anything"));
        assert!(!is_stdlib_module_spec("@user/thing"));
        assert!(!is_stdlib_module_spec("./local"));
    }
}
