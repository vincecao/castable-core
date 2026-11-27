//! `castable-core`: the pure protocol core of the universal cast receiver.
//!
//! No sockets, no threads, no clocks: every function takes values and returns
//! values, so the same code runs on iOS, Android, desktop and WebAssembly.
//! `castable-net` (later) does the I/O around it.

pub mod descriptor;
pub mod device;
pub mod didl;
pub mod error;
pub mod renderer;
pub mod service;
pub mod session;
pub mod soap;
pub mod time;
pub mod xml;

pub use descriptor::{Delivery, MediaDescriptor, MediaKind};
pub use device::DeviceInfo;
pub use error::UpnpError;
pub use renderer::{Action, Command, Renderer, Reply};
pub use service::Service;
pub use session::{
    AfterLoad, Cast, Gen, Input, Outcome, Phase, Playback, PlayerCommand, PlayerEvent, SenderCommand, Session,
    TransportState, Unloaded,
};
