# deka-modules

deka's module-resolution contracts, extracted from the `runtime_core` crate
in [`dekaruntime/deka`](https://github.com/dekaruntime/deka) (deka#881).

This crate is the shared vocabulary for how DekaScript modules are named,
scanned, and resolved on disk:

- **`module_spec`** — the module specifier vocabulary: bare vs. relative vs.
  `@deka/`-scoped specs, summoned JavaScript (`@js/`), the closed stdlib module list, DekaScript source
  extensions (`.ds`, `.dsx`) and the file-resolution candidates they produce.
- **`project_gate`** — the project-boundary gate: given a set of imports and a
  project's declared dependencies plus `ds_modules/` contents, decides whether
  every import is either stdlib, declared-and-installed, or satisfied by a
  `deka link`.
- **`ds_imports`** — a compiler-free scanner that extracts `import`/`from`
  specifiers out of raw DekaScript source text (comment- and string-aware, no
  parser dependency).
- **`modules`** — `ds_modules/` resolution: locating the module root for a
  project, installing/linking packages into it, and reading it back for the
  gate and the runtime loader.

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
resolvable vendored entry. Lock presence is checked here; integrity verification
remains the package manager's responsibility. Another `@js/` dependency, an
unprefixed name, `ds_modules/` copy, or local link does not satisfy the import.
External stdlib roots and `require_lockfile: false` do not waive these checks:
summoned JavaScript remains a project-owned, declared-and-vendored dependency.

## Who uses this

`dekaruntime/deka` depends on this crate for its module resolver and project
gate. `dsc` (the standalone compiler) is expected to pick it up next, so both
tools agree on the same specifier and resolution rules without importing the
whole runtime.

## Versioning

This crate is published independently to crates.io and follows its own
semver — it is **not** lockstepped with `deka`'s release cadence. A breaking
change here is a major bump on `deka-modules`, and consumers pin the version
range they've tested against.

## License

Apache-2.0. See [LICENSE](./LICENSE).
