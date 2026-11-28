//! `castable-core`: the pure protocol core of the universal cast receiver.
//!
//! No sockets, no threads, no clocks: every function takes values and returns
//! values, so the same code runs on iOS, Android, desktop and WebAssembly.
//! `castable-net` (later) does the I/O around it.

mod descriptor;
pub mod device;
mod didl;
mod error;
mod errors;
pub mod keys;
mod media_info;
mod renderer;
pub mod service;
mod session;
pub mod soap;
mod time;
mod tracks;
mod xml;

pub use descriptor::{Delivery, MediaDescriptor, MediaKind};
pub use device::DeviceInfo;
pub use error::UpnpError;
pub use errors::{CastError, ErrorCategory, ErrorCode};
pub use keys::PlayerKey;
pub use media_info::{AudioInfo, MediaInfo, VideoInfo};
pub use renderer::{Action, ReceiverView, Renderer, Reply};
pub use service::Service;
pub use session::{
    AfterLoad, Cast, Gen, Input, Outcome, Phase, PhaseView, Playback, PlayerCommand, PlayerEvent, ScreenAction,
    ScreenCommand, SenderCommand, Session, TransportState, Unloaded, View,
};
pub use tracks::{MediaTrack, TrackKind};
