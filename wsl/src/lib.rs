#![forbid(unsafe_code)]

//! Safe, idiomatic Rust API for creating and running Linux containers in WSL.
//!
//! Objects are layered [`Session`](session::Session) → [`Container`](container::Container) → [`Process`](process::Process), mirroring the
//! `Microsoft::WSL::Containers` C++/WinRT API this crate wraps. The WinRT projection itself lives in the `wsl-sys` crate.
//!
//! See [`session`] for a diagram of how the pieces fit together.
//!
//! **Note**: This crate is Windows specific
//!
//!

// region: Internal modules
mod error;
mod service;
// endregion

// region: Publicly exported modules

/// Controls execution instances
///
/// A [`session::Session`] does not run anything per-say: it acts as a scaffold and delegates the running part
/// to its internal [`container::Container`]. Each session must therefore have one. The container will allow to
/// define internals such as
/// - the (docker) image to use
/// - the volume paths to be mapped
/// - the port mapping
/// - control the init (root) process that must be always be running in the instance
///
/// And it also allows to control the runtime of the instance itself (start, stop, etc.)
pub mod container;

/// Defines the process(es) that can be running in the instance
pub mod process;

/// Defines the general envelop of what the VM will be running.
///
/// A [`session::Session`] hosts internal information such as the number of CPU, the memory to dedicate to the running instance, the VHD allocation etc.
///
/// How this crate sits on top of the WSL containers SDK.
///
/// ```mermaid
/// flowchart TD
///     app["Your application"]
///
///     subgraph wsl["wsl crate (safe API)"]
///         service["missing_components() / version()"]
///         sb["SessionBuilder"] -- "build()" --> session["Session"]
///         session -- "create_container()" --> cb["ContainerBuilder"]
///         cb -- "build()" --> container["Container"]
///         pb["ProcessBuilder"] -- "with_init_process() / create_process()" --> process["Process"]
///         container --> process
///         process -- "events()" --> events["ProcessEvents"]
///     end
///
///     subgraph sys["wsl-sys crate (unsafe FFI)"]
///         init["init(): registration-free activation"]
///         bindings["bindings::Microsoft::WSL::Containers<br/>(generated from the .rdl metadata)"]
///     end
///
///     subgraph sdk["Microsoft.WSL.Containers SDK"]
///         dll["wslcsdk.dll + SxS manifest<br/>(embedded in the executable)"]
///     end
///
///     subgraph host["Windows host"]
///         subgraph um["User mode"]
///             wslsvc["WSL service<br/>(wslservice.exe)"]
///             hcs["Host Compute Service<br/>(vmcompute.exe)"]
///             vmwp["VM worker process<br/>(vmwp.exe)"]
///         end
///         subgraph km["Kernel mode"]
///             vid["Virtualization infrastructure driver<br/>(vid.sys)"]
///             vmbus["VMBus / Hyper-V sockets"]
///         end
///     end
///
///     hv["Hyper-V hypervisor"]
///
///     subgraph guest["Session VM (Hyper-V guest)"]
///         lk["Linux kernel"]
///         ctr["Linux containers"]
///     end
///
///     app --> service
///     app --> sb
///     service & session & container & process --> bindings
///     sb & service --> init
///     init -- "extracts and activates" --> dll
///     bindings -- "WinRT calls" --> dll
///     dll --> wslsvc
///     wslsvc -- "creates VM" --> hcs
///     hcs --> vmwp
///     vmwp --> vid
///     vid --> hv
///     hv -- "hosts" --> guest
///     wslsvc -. "control channel" .-> vmbus
///     vmbus -.-> lk
///     lk --> ctr
/// ```
#[cfg_attr(doc, doc = include_str!("../../.cargo/mermaid.html"))]
pub mod session;
// endregion

// region: Internal public modules
pub(crate) mod prelude;
pub(crate) mod utils;
// endregion

// region: Re-exported items
pub use crate::error::{Error, Result};
pub use crate::service::Component as ServiceComponent;
pub use crate::service::{missing_components, version};
// endregion

use wsl_sys::bindings::Microsoft::WSL::Containers as raw;
