use deka_modules::module_spec::{
    canonical_php_package_spec, is_summoned_js_module_spec, is_valid_package_name,
    module_spec_aliases, resolve_summoned_js_module_file, summoned_js_package_name,
};
use deka_modules::project_gate::{
    GateOptions, is_stdlib_module_spec, resolve_module_file, validate_project,
};
use std::path::Path;

fn write(root: &Path, name: &str, body: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "deka.json",
        r#"{"dependencies":{"@js/three-js":"url:https://example.test/three.mjs"}}"#,
    );
    write(
        tmp.path(),
        "deka.lock",
        r#"{"lockfileVersion":1,"packages":{"@js/three-js":["1.0.0","url:https://example.test/three.mjs",{},"sha256-test"]}}"#,
    );
    write(
        tmp.path(),
        "js_modules/three-js/index.mjs",
        "export const scene = () => ({});",
    );
    tmp
}

fn gate(root: &Path, spec: &str, options: &GateOptions) -> Result<(), String> {
    validate_project(root, &[spec.to_string()], options)
}

#[test]
fn summoned_class_is_distinct_and_shares_name_validation() {
    for spec in ["@js/three-js", "@js/Name_2"] {
        assert!(is_summoned_js_module_spec(spec));
        assert!(is_valid_package_name(spec));
        assert!(summoned_js_package_name(spec).is_some());
        assert!(!is_stdlib_module_spec(spec));
        assert_eq!(canonical_php_package_spec(spec), None);
        assert_eq!(module_spec_aliases(spec), vec![spec]);
    }
    assert_eq!(summoned_js_package_name(" @js/three-js "), Some("three-js"));
    for spec in [
        "@js/",
        "@js/..",
        "@js/../escape",
        "@js/three/sub",
        "@js/@scope/name",
        "@js/a\\b",
        "@js/a.b",
    ] {
        assert!(is_summoned_js_module_spec(spec));
        assert!(!is_valid_package_name(spec));
        assert_eq!(summoned_js_package_name(spec), None);
        assert_eq!(canonical_php_package_spec(spec), None);
    }
    for spec in [
        "three-js",
        "@other/three-js",
        "@deka/three-js",
        "./@js/three-js",
    ] {
        assert!(!is_summoned_js_module_spec(spec));
        assert_eq!(summoned_js_package_name(spec), None);
    }
}

#[test]
fn resolution_precedence_is_module_main_index() {
    let tmp = project();
    let root = tmp.path();
    let resolve = || resolve_summoned_js_module_file(root, "@js/three-js").unwrap();
    assert_eq!(resolve(), root.join("js_modules/three-js/index.mjs"));
    write(root, "js_modules/three-js/package.json", "{}");
    assert_eq!(resolve(), root.join("js_modules/three-js/index.mjs"));
    write(root, "js_modules/three-js/main.js", "export {};");
    write(root, "js_modules/three-js/esm/module.mjs", "export {};");
    write(
        root,
        "js_modules/three-js/package.json",
        r#"{"main":"./main.js"}"#,
    );
    assert_eq!(resolve(), root.join("js_modules/three-js/./main.js"));
    write(
        root,
        "js_modules/three-js/package.json",
        r#"{"main":"main.js","module":"esm/module.mjs"}"#,
    );
    assert_eq!(resolve(), root.join("js_modules/three-js/esm/module.mjs"));
}

#[test]
fn invalid_explicit_entries_never_fall_back() {
    let tmp = project();
    for manifest in [
        "broken",
        "[]",
        r#"{"module":null}"#,
        r#"{"module":""}"#,
        r#"{"module":12}"#,
        r#"{"main":"missing.js"}"#,
        r#"{"module":"missing.mjs","main":"index.mjs"}"#,
        r#"{"main":"../outside.mjs"}"#,
        r#"{"main":"/tmp/outside.mjs"}"#,
        r#"{"main":"..\\outside.mjs"}"#,
        r#"{"main":"."}"#,
    ] {
        write(tmp.path(), "js_modules/three-js/package.json", manifest);
        assert!(
            resolve_summoned_js_module_file(tmp.path(), "@js/three-js").is_err(),
            "{manifest}"
        );
    }
}

#[cfg(unix)]
#[test]
fn entry_symlinks_cannot_escape_package() {
    let tmp = project();
    write(tmp.path(), "outside.mjs", "export {};");
    std::fs::remove_file(tmp.path().join("js_modules/three-js/index.mjs")).unwrap();
    std::os::unix::fs::symlink(
        tmp.path().join("outside.mjs"),
        tmp.path().join("js_modules/three-js/index.mjs"),
    )
    .unwrap();
    assert!(resolve_summoned_js_module_file(tmp.path(), "@js/three-js").is_err());
}

#[test]
fn declared_locked_and_vendored_passes_without_ds_modules() {
    let tmp = project();
    gate(tmp.path(), " @js/three-js ", &GateOptions::default()).unwrap();
    write(
        tmp.path(),
        "deka.json",
        r#"{"devDependencies":{"@js/three-js":"1.0.0"}}"#,
    );
    gate(tmp.path(), "@js/three-js", &GateOptions::default()).unwrap();
}

#[test]
fn declaration_requires_exact_js_identity() {
    let tmp = project();
    for name in [
        "@js/other",
        "three-js",
        "@deka/three-js",
        "@registry/three-js",
    ] {
        write(
            tmp.path(),
            "deka.json",
            &format!(r#"{{"dependencies":{{"{name}":"1.0.0"}}}}"#),
        );
        let err = gate(tmp.path(), "@js/three-js", &GateOptions::default()).unwrap_err();
        assert!(err.contains("not declared"), "{err}");
    }
}

#[test]
fn lock_requires_exact_package_entry_and_valid_json() {
    let tmp = project();
    for lock in [
        "broken",
        "{}",
        r#"{"packages":{"@js/other":{}}}"#,
        r#"{"packages":{"three-js":{}}}"#,
        r#"{"packages":{"@js/three-js":null}}"#,
    ] {
        write(tmp.path(), "deka.lock", lock);
        let err = gate(tmp.path(), "@js/three-js", &GateOptions::default()).unwrap_err();
        assert!(err.contains("deka.lock"), "{err}");
    }
}

#[test]
fn gate_requires_resolvable_vendor_entry_and_rejects_ds_copy() {
    let tmp = project();
    std::fs::remove_dir_all(tmp.path().join("js_modules")).unwrap();
    write(
        tmp.path(),
        "ds_modules/@js/three-js/index.ds",
        "export fn scene() {}",
    );
    assert!(resolve_module_file(&tmp.path().join("ds_modules"), "@js/three-js").is_none());
    let err = gate(tmp.path(), "@js/three-js", &GateOptions::default()).unwrap_err();
    assert!(err.contains("not vendored"), "{err}");
    std::fs::create_dir_all(tmp.path().join("js_modules/three-js")).unwrap();
    assert!(gate(tmp.path(), "@js/three-js", &GateOptions::default()).is_err());
    write(
        tmp.path(),
        "js_modules/three-js/package.json",
        r#"{"module":"missing.mjs"}"#,
    );
    assert!(gate(tmp.path(), "@js/three-js", &GateOptions::default()).is_err());
}

#[test]
fn stdlib_bypasses_do_not_waive_js_checks() {
    let tmp = project();
    let opts = GateOptions {
        module_root: Some(tmp.path().join("external")),
        require_lockfile: false,
        context: "deka build",
    };
    gate(tmp.path(), "@js/three-js", &opts).unwrap();
    std::fs::remove_file(tmp.path().join("deka.lock")).unwrap();
    for module_root in [None, opts.module_root.clone()] {
        let opts = GateOptions {
            module_root,
            ..opts.clone()
        };
        let err = gate(tmp.path(), "@js/three-js", &opts).unwrap_err();
        assert!(err.starts_with("deka build:"), "{err}");
        assert!(err.contains("deka.lock"), "{err}");
    }
    let err = gate(tmp.path(), "@js/../escape", &opts).unwrap_err();
    assert!(err.contains("invalid summoned"), "{err}");
}

#[test]
fn mixed_imports_keep_stdlib_declaration_checks() {
    let tmp = project();
    let err = validate_project(
        tmp.path(),
        &["@js/three-js".into(), "crypto".into()],
        &GateOptions::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("not declared") && err.contains("crypto"),
        "{err}"
    );
}
