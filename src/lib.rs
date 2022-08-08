pub mod network_entity;
pub mod network_frame;
pub mod networked_transform;
pub mod sequence_buffer;

pub use bevy;
pub use bit_serializer;

use bevy::prelude::*;
use bit_serializer::{BitReader, BitWriter};
use network_entity::NetworkEntities;

pub use network_entity::NetworkID;
pub use network_frame::*;
use sequence_buffer::SequenceBuffer;

use std::{collections::HashMap, io, marker::PhantomData};

pub struct NetworkTick(pub u16);

pub struct NetworkFrameBuffer<T>(pub SequenceBuffer<T>);

pub struct LastNetworkTick(pub HashMap<u64, u16>);

pub struct LastReceivedNetworkTick(pub Option<u16>);

pub struct ReplicateServerPlugin<T> {
    data: PhantomData<T>,
}

impl<T> Default for ReplicateServerPlugin<T> {
    fn default() -> Self {
        Self { data: PhantomData }
    }
}

impl<T: NetworkedFrame> Plugin for ReplicateServerPlugin<T> {
    // TODO: cleanup NetworkEntity when entity is despawned
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkEntities::default());
        app.insert_resource(NetworkTick(0));
        app.insert_resource(LastNetworkTick(HashMap::new()));

        let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(60);
        app.insert_resource(NetworkFrameBuffer(buffer));

        app.add_system_to_stage(CoreStage::PreUpdate, tick_network);
        app.add_system_to_stage(CoreStage::Update, generate_frame::<T>.exclusive_system().at_end());
    }
}

impl<T: NetworkedFrame> ReplicateServerPlugin<T> {
    pub fn init_resources(world: &mut World) {
        world.insert_resource(NetworkEntities::default());
        world.insert_resource(NetworkTick(0));
        world.insert_resource(LastNetworkTick(HashMap::new()));

        let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(60);
        world.insert_resource(NetworkFrameBuffer(buffer));
    }
}

pub fn generate_frame<T: NetworkedFrame>(world: &mut World) {
    let tick = world.get_resource::<NetworkTick>().unwrap().0;
    let frame = T::generate_frame(tick, world);
    let buffer = &mut world.get_resource_mut::<NetworkFrameBuffer<T>>().unwrap().0;
    buffer.insert(tick, frame);
}

fn tick_network(mut network_tick: ResMut<NetworkTick>) {
    network_tick.0 += 1;
}

pub fn replicate<T: NetworkedFrame>(
    client: u64,
    tick: &NetworkTick,
    buffer: &NetworkFrameBuffer<T>,
    last_ticks: &LastNetworkTick,
) -> Result<Vec<u8>, io::Error> {
    let mut writer = BitWriter::with_capacity(1000);
    let frame = buffer.0.get(tick.0).unwrap();
    if let Some(last_received_tick) = last_ticks.0.get(&client) {
        match buffer.0.get(*last_received_tick) {
            Some(last_received_frame) => {
                frame.write_delta_frame(&mut writer, last_received_frame)?;
            }
            None => {
                frame.write_full_frame(&mut writer)?;
            }
        }
    } else {
        frame.write_full_frame(&mut writer)?;
    }

    writer.consume()
}

// TODO: maybe add an event with the buffer, add then renet can just emit the buffer there,
// and we can add this as a system in the client plugin
pub fn process_snap<T: NetworkedFrame>(buffer: Vec<u8>, world: &mut World) -> Result<(), io::Error> {
    let mut reader = BitReader::new(&buffer)?;
    let frame = T::read_frame(&mut reader, world)?;

    let last_received_tick = &mut world.get_resource_mut::<LastReceivedNetworkTick>().unwrap();
    match last_received_tick.0 {
        Some(tick) => {
            if frame.tick() > tick {
                last_received_tick.0 = Some(frame.tick())
            }
        }
        None => last_received_tick.0 = Some(frame.tick()),
    }

    let frame_buffer = &mut world.get_resource_mut::<NetworkFrameBuffer<T>>().unwrap().0;
    frame_buffer.insert(frame.tick(), frame.clone());

    let mut frames = world.get_resource_mut::<Events<T>>().unwrap();
    frames.send(frame);

    Ok(())
}

pub struct ReplicateClientPlugin<T> {
    data: PhantomData<T>,
}

impl<T> Default for ReplicateClientPlugin<T> {
    fn default() -> Self {
        Self { data: PhantomData }
    }
}

impl<T: NetworkedFrame> Plugin for ReplicateClientPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<T>();
        app.insert_resource(LastReceivedNetworkTick(None));
        app.insert_resource(NetworkTick(0));
        app.insert_resource(NetworkMapping(HashMap::new()));

        let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(60);
        app.insert_resource(NetworkFrameBuffer(buffer));

        app.add_system_to_stage(CoreStage::PreUpdate, apply_network_frame::<T>.exclusive_system().at_end());
    }
}

// TODO: add frame to buffer before applying it to the world
// Also, check order
fn apply_network_frame<T: NetworkedFrame>(world: &mut World) {
    world.resource_scope(|world, network_frames: Mut<Events<T>>| {
        for frame in network_frames.get_reader().iter(&network_frames) {
            frame.apply_in_world(world);
        }
    });
}

pub struct NetworkMapping(pub HashMap<NetworkID, Entity>);
