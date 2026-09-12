//! deka-modules: deka's module-resolution contracts.
//!
//! Extracted from deka's `runtime_core` crate (deka#881). Covers the
//! specifier vocabulary (`module_spec`), the project-boundary gate
//! (`project_gate`), DS import scanning (`ds_imports`), and `ds_modules/`
//! resolution (`modules`).

pub mod ds_imports;
pub mod module_spec;
pub mod modules;
pub mod project_gate;
