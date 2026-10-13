#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, clippy::all)]

//! Raw WinRT projection of `Microsoft.WSL.Containers`, generated at build time from the
//! SDK's metadata (`sdk/Microsoft.WSL.Containers.rdl`, converted from the NuGet package's
//! `.winmd`). This is the FFI layer; use the `wsl` crate for an idiomatic API.
//!
//! Call [`init`] before activating any class from [`bindings`].

#[cfg(feature = "embed-runtime")]
mod activation;

#[cfg(feature = "embed-runtime")]
pub use activation::init;

/// Without the `embed-runtime` feature nothing is embedded, so there is nothing to set up: the
/// application must make `wslcsdk.dll` and its activation manifest available itself.
#[cfg(not(feature = "embed-runtime"))]
pub fn init() -> windows_core::Result<()> {
    Ok(())
}

// Classes pulled in from `Windows.Storage.Streams` keep factories whose methods were filtered out.
#[allow(dead_code)]
pub mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

// Re-exported so dependents use exactly the versions the bindings were generated against.
pub use {windows_collections, windows_core, windows_future, windows_reference, windows_time};
