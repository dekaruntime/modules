use crate::{
    ds_imports::paths,
    integrity::compute_fs_graph_hash,
    module_spec::{STDLIB_MODULE_NAMES, STDLIB_SPEC_PREFIXES, is_stdlib_module_spec},
    project_gate::{GateOptions, validate_project, validate_project_source},
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
        r#"{"dependencies":{"@deka/crypto":"1"}}"#,
    );
    write(
        tmp.path(),
        "ds_modules/@deka/crypto/index.ds",
        "export fn noop() {}\n",
    );
    lock(
        tmp.path(),
        serde_json::json!({"fsGraph": {"hash": compute_fs_graph_hash(&tmp.path().join("ds_modules/@deka/crypto")).unwrap()}}),
    );
    tmp
}

fn lock(root: &Path, metadata: serde_json::Value) {
    write(
        root,
        "deka.lock",
        &serde_json::json!({"packages":{"@deka/crypto":["1","registry",metadata,"tarball"]}})
            .to_string(),
    );
}

fn gate(root: &Path) -> Result<(), String> {
    validate_project_source(
        root,
        "import { noop } from 'crypto'",
        &GateOptions::default(),
    )
}

#[test]
fn dsc_vocabulary_is_closed_and_alias_symmetric() {
    assert_eq!(
        STDLIB_SPEC_PREFIXES,
        &["component/", "deka/", "encoding/", "db/"]
    );
    for name in STDLIB_MODULE_NAMES.iter().copied().chain([
        "component/router",
        "deka/core",
        "encoding/json",
        "db/postgres",
    ]) {
        assert!(is_stdlib_module_spec(name), "{name}");
        assert!(is_stdlib_module_spec(&format!("@deka/{name}")), "{name}");
    }
    for name in [
        "anything",
        "ui",
        "ui/button",
        "json/extra",
        "http/extra",
        "",
    ] {
        assert!(!is_stdlib_module_spec(name), "{name}");
        assert!(!is_stdlib_module_spec(&format!("@deka/{name}")), "{name}");
    }
    assert!(!is_stdlib_module_spec("@user/json"));
    assert!(!is_stdlib_module_spec("./json"));
}

#[test]
fn wildcard_and_ui_imports_do_not_bypass_package_gate() {
    let tmp = project();
    for spec in ["@deka/anything", "ui", "ui/button", "@deka/ui/button"] {
        let err =
            validate_project(tmp.path(), &[spec.into()], &GateOptions::default()).unwrap_err();
        assert!(err.contains("not declared"), "{err}");
    }
}

#[test]
fn gate_rejects_modified_added_and_removed_package_files() {
    for action in ["modify", "add", "remove"] {
        let tmp = project();
        write(tmp.path(), "ds_modules/@deka/crypto/extra.ds", "original");
        lock(
            tmp.path(),
            serde_json::json!({"fsGraph":{"hash":compute_fs_graph_hash(&tmp.path().join("ds_modules/@deka/crypto")).unwrap()}}),
        );
        gate(tmp.path()).unwrap();
        match action {
            "modify" => write(tmp.path(), "ds_modules/@deka/crypto/extra.ds", "modified"),
            "add" => write(tmp.path(), "ds_modules/@deka/crypto/new.ds", "new"),
            _ => std::fs::remove_file(tmp.path().join("ds_modules/@deka/crypto/extra.ds")).unwrap(),
        }
        assert!(gate(tmp.path()).unwrap_err().contains("integrity mismatch"));
    }
}

#[test]
fn lock_entry_required_and_recorded_hash_cannot_be_waived() {
    let tmp = project();
    write(tmp.path(), "deka.lock", r#"{"packages":{}}"#);
    assert!(gate(tmp.path()).unwrap_err().contains("no deka.lock entry"));
    for graph in [
        serde_json::json!({}),
        serde_json::json!({"hash":"invalid"}),
        serde_json::Value::Null,
    ] {
        lock(tmp.path(), serde_json::json!({"fsGraph":graph}));
        assert!(gate(tmp.path()).unwrap_err().contains("invalid fsGraph"));
    }
    lock(
        tmp.path(),
        serde_json::json!({"fsGraph":{"hash":"0".repeat(64)}}),
    );
    assert!(
        validate_project_source(
            tmp.path(),
            "import 'crypto'",
            &GateOptions {
                require_lockfile: false,
                ..GateOptions::default()
            }
        )
        .unwrap_err()
        .contains("integrity mismatch")
    );
    lock(tmp.path(), serde_json::json!({}));
    gate(tmp.path()).unwrap(); // Historical lock without fsGraph.
}

#[test]
fn fs_graph_matches_dsc_byte_contract_and_exclusions() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "z.ds", "z");
    write(tmp.path(), "a/index.ds", "a");
    // Independently computed SHA-256 of b"a/index.ds\0a\nz.ds\0z\n".
    assert_eq!(
        compute_fs_graph_hash(tmp.path()).unwrap(),
        "7807374a91ed0ff97e105b0016949811bd4555ba5e4c38d2cc34decf286f64a6"
    );
    for name in [
        "ds_modules/x.ds",
        "php_modules/x.ds",
        "node_modules/x.js",
        "target/x",
        "dist/x",
        ".git/x",
        ".cache/x",
        ".deka/x",
        ".DS_Store",
        "._x",
    ] {
        write(tmp.path(), name, "ignored");
    }
    assert_eq!(
        compute_fs_graph_hash(tmp.path()).unwrap(),
        "7807374a91ed0ff97e105b0016949811bd4555ba5e4c38d2cc34decf286f64a6"
    );
}

#[test]
fn scanner_parity_fixtures_are_shared_gate_inputs() {
    let cases: &[(&str, &[&str])] = &[
        (
            include_str!("../tests/fixtures/import_scan/declarations.ds"),
            &[
                "./side.ds",
                "io",
                "json",
                "@deka/crypto",
                "@vendor/package/subpath",
                "./point.ds",
                "./all.ds",
                "./commented.ds",
                "bytes",
                "buffer",
            ],
        ),
        (
            include_str!("../tests/fixtures/import_scan/noise.ds"),
            &["crypto"],
        ),
    ];
    let tmp = project();
    for (source, expected) in cases {
        assert_eq!(paths(source), *expected);
        assert_eq!(
            validate_project_source(tmp.path(), source, &GateOptions::default()),
            validate_project(
                tmp.path(),
                &expected.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                &GateOptions::default()
            )
        );
    }
}

#[test]
fn third_party_subpaths_preserve_scope_and_check_integrity() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "ds_modules/@vendor/tool/sub.ds",
        "export const x = 1",
    );
    let hash = compute_fs_graph_hash(&tmp.path().join("ds_modules/@vendor/tool")).unwrap();
    write(tmp.path(), "deka.lock", &serde_json::json!({"php":{"packages":{"@vendor/tool":["1","registry",{"fs_graph":{"hash":hash}},"tarball"]}}}).to_string());
    write(
        tmp.path(),
        "deka.json",
        r#"{"dependencies":{"@other/tool":"1"}}"#,
    );
    let source = "import { x } from '@vendor/tool/sub'";
    assert!(
        validate_project_source(tmp.path(), source, &GateOptions::default())
            .unwrap_err()
            .contains("not declared")
    );
    write(
        tmp.path(),
        "deka.json",
        r#"{"dependencies":{"@vendor/tool":"1"}}"#,
    );
    validate_project_source(tmp.path(), source, &GateOptions::default()).unwrap();
    write(tmp.path(), "ds_modules/@vendor/tool/sub.ds", "changed");
    assert!(
        validate_project_source(tmp.path(), source, &GateOptions::default())
            .unwrap_err()
            .contains("integrity mismatch")
    );
}

#[test]
fn declared_local_link_overrides_installed_hash_but_stale_link_fails() {
    use crate::modules::{LinkEntry, LinkManifest, write_links_at};
    let tmp = project();
    let linked = tempfile::tempdir().unwrap();
    write(linked.path(), "deka.json", r#"{"name":"@deka/crypto"}"#);
    write(linked.path(), "index.ds", "different working copy");
    write_links_at(
        tmp.path(),
        &LinkManifest {
            packages: [(
                "@deka/crypto".into(),
                LinkEntry {
                    path: linked.path().to_path_buf(),
                },
            )]
            .into(),
            ..LinkManifest::default()
        },
    )
    .unwrap();
    lock(
        tmp.path(),
        serde_json::json!({"fsGraph":{"hash":"0".repeat(64)}}),
    );
    gate(tmp.path()).unwrap();
    write(tmp.path(), "deka.json", "{}");
    assert!(gate(tmp.path()).unwrap_err().contains("not declared"));
    write(
        tmp.path(),
        "deka.json",
        r#"{"dependencies":{"crypto":"1"}}"#,
    );
    linked.close().unwrap();
    assert!(gate(tmp.path()).unwrap_err().contains("link is unusable"));
}

#[test]
fn integrity_checks_the_resolved_alias_not_a_shadow_copy() {
    let tmp = project();
    write(tmp.path(), "ds_modules/crypto/index.ds", "tampered shadow");
    assert!(gate(tmp.path()).unwrap_err().contains("integrity mismatch"));
    validate_project_source(tmp.path(), "import '@deka/crypto'", &GateOptions::default()).unwrap();
}
