# deka-modules

deka's module-resolution contracts, extracted from the `runtime_core` crate
in [`dekaruntime/deka`](https://github.com/dekaruntime/deka) (deka#881).

This crate is the shared vocabulary for how DekaScript modules are named,
scanned, and resolved on disk:

- **`module_spec`** — the module specifier vocabulary: bare vs. relative vs.
  `@deka/`-scoped specs, summoned JavaScript (`@js/`), the closed stdlib module list, DekaScript source
  extensions (`.ds`, `.dsx`) and the file-resolution candidates they produce.
- **`project_gate`** — the shared lock + declared + installed + fsGraph
  integrity gate for package imports, with explicit toolchain and local-link rules.
- **`ds_imports`** — the single compiler-free static import scanner for both
  consumers' gates (comments, strings, multiline declarations, and re-exports).
- **`integrity`** — dsc-compatible SHA-256 hashing of installed package trees.
- **`modules`** — `ds_modules/` resolution: locating the module root for a
  project, installing/linking packages into it, and reading it back for the
  gate and the runtime loader.

## 0.3.0 reconciliation (dsc#167)

`module_spec::STDLIB_MODULE_NAMES` is the closed name vocabulary, extended only
by `STDLIB_SPEC_PREFIXES`: `component/`, `deka/`, `encoding/`, and `db/`.
`is_stdlib_module_spec` strips one `@deka/` alias before matching the same
vocabulary. Unknown `@deka/*`, `ui`, and `ui/*` are not stdlib. The old
`project_gate::is_stdlib_module_spec` path re-exports that same function.
`CLOSED_STDLIB_MODULES` remains a separate compiler-provided export contract:
`math` / `@deka/math` needs no package declaration or installation.

Use `project_gate::validate_project_source(root, source, options)` for one
source. For a module graph, scan each source with `ds_imports::paths`, combine
those results, and call `validate_project`. This scanner is the gate contract;
consumers keep their real parser for compilation. The scanner collects static
quoted imports and re-exports, skips comments/string/template text, and does
not collect dynamic `import(...)` expressions. It is not a syntax validator;
quoted paths retain their source spelling (including escapes). Fixtures under
`tests/fixtures/import_scan` pin the common gate inputs without compiler deps.

All bare package imports (including foreign scopes and unknown `@deka/*`)
require declaration and installation; relative, project-root `@/`, URL, and
compiler-provided math imports are excluded. Local links still require a
declaration and a valid target, but waive installed-package lock/hash checks.
An external module root supplies only recognized stdlib, never arbitrary
packages. `require_lockfile: false` permits an absent lock; it does not disable
checks against a lock that exists.

Installed packages require a tuple entry in `deka.lock` `packages` (or legacy
`php.packages`). If tuple metadata records `fsGraph.hash` (or `fs_graph.hash`),
the resolved package tree must match it. Hashing uses dsc's sorted relative
paths + NUL + file bytes + newline, with the same dependency/build/cache and
macOS metadata exclusions. Missing fsGraph is accepted for older locks;
malformed recorded hashes and integrity mismatches fail with an install hint.
This is installed-tree integrity, distinct from package archive integrity.

## Summoned JavaScript (`@js/`)

`@js/three-js` is a reserved routing class for foreign JavaScript, distinct
from scoped registry packages and stdlib aliases (rfd#39). Its name uses
`is_valid_package_name`: ASCII letters, digits, hyphens and underscores;
subpaths and nested scopes are not supported. `is_summoned_js_module_spec`
recognizes the prefix even for invalid names so callers can reject them;
`summoned_js_package_name` returns only a validated vendor name. Registry
canonicalization and the DekaScript file resolver exclude this class.

`resolve_summoned_js_module_file(project_root, "@js/three-js")` resolves under
`js_modules/three-js/`. Entry precedence is **package.json `module`, then
`main`, then `index.mjs`**. The fallback applies when neither field exists or
package.json is absent. An explicit entry must name an existing file; invalid
JSON, non-string or empty entries, missing files, directories and paths escaping
the package (including entry symlinks) fail rather than falling back. There is
no extension probing or exports-map interpretation.

The project gate requires the exact `@js/three-js` identity in `deka.json`
`dependencies` or `devDependencies`, the matching `deka.lock` `packages` entry
(using pm's existing `[version, source, metadata, integrity]` tuple), and a
resolvable vendored entry. Summoned archive integrity verification remains the package manager's
responsibility; the fsGraph rule above applies to installed DekaScript packages. Another `@js/` dependency, an
unprefixed name, `ds_modules/` copy, or local link does not satisfy the import.
External stdlib roots and `require_lockfile: false` do not waive these checks:
summoned JavaScript remains a project-owned, declared-and-vendored dependency.

## Who uses this

`dekaruntime/deka` depends on this crate for its module resolver and project
gate. `dsc` (the standalone compiler) already consumes matching resolution APIs.
Both consumers will converge on these 0.3.0 gate contracts in follow-up changes
after publication.

## Versioning

This crate is published independently to crates.io and follows its own
semver — it is **not** lockstepped with `deka`'s release cadence. A breaking
change here is a major bump on `deka-modules`, and consumers pin the version
range they've tested against.

## License

Apache-2.0. See [LICENSE](./LICENSE).
