pub mod client;
mod network_entity;
pub mod network_frame;
pub mod networked_transform;
pub mod sequence_buffer;
pub mod server;

#[doc(hidden)]
pub use bevy;
pub use bit_serializer::{BitReader, BitWriter};

pub use network_entity::{NetworkID, NetworkEntities};
pub use network_frame::*;

pub struct NetworkFrameBuffer<T>(pub sequence_buffer::SequenceBuffer<T>);
