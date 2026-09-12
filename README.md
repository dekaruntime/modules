# deka-modules

deka's module-resolution contracts, extracted from the `runtime_core` crate
in [`dekaruntime/deka`](https://github.com/dekaruntime/deka) (deka#881).

This crate is the shared vocabulary for how DekaScript modules are named,
scanned, and resolved on disk:

- **`module_spec`** — the module specifier vocabulary: bare vs. relative vs.
  `@deka/`-scoped specs, the closed stdlib module list, DekaScript source
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
