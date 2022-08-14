use crate::{
    network_entity::{cleanup_network_entity_system, track_network_entity_system, NetworkEntities},
    sequence_buffer::SequenceBuffer,
    NetworkedFrame,
};
use bevy::{prelude::*, time::FixedTimestep};
use bit_serializer::BitWriter;
use std::{collections::HashMap, io, marker::PhantomData};

pub struct NetworkTick(pub u64);

pub struct NetworkFrameBuffer<T>(pub SequenceBuffer<T>);

pub struct LastNetworkTick(pub HashMap<u64, u64>);

pub struct ReplicateServerPlugin<T> {
    tick_rate: f64,
    data: PhantomData<T>,
}

impl<T> Default for ReplicateServerPlugin<T> {
    fn default() -> Self {
        Self {
            tick_rate: 20.,
            data: PhantomData,
        }
    }
}

impl<T: NetworkedFrame> Plugin for ReplicateServerPlugin<T> {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkEntities::default());
        app.insert_resource(NetworkTick(0));
        app.insert_resource(LastNetworkTick(HashMap::new()));

        let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(60);
        app.insert_resource(NetworkFrameBuffer(buffer));

        app.add_system_to_stage(
            CoreStage::PreUpdate,
            tick_network.with_run_criteria(FixedTimestep::steps_per_second(self.tick_rate)),
        );
        app.add_system_to_stage(
            CoreStage::Update,
            generate_network_frame::<T>
                .exclusive_system()
                .at_end()
                .with_run_criteria(FixedTimestep::steps_per_second(self.tick_rate)),
        );

        app.add_system(track_network_entity_system);
        app.add_system(cleanup_network_entity_system);
    }
}

fn generate_network_frame<T: NetworkedFrame>(world: &mut World) {
    let tick = world.resource::<NetworkTick>().0;
    let frame = T::generate_frame(tick, world);
    let buffer = &mut world.resource_mut::<NetworkFrameBuffer<T>>().0;
    buffer.insert(tick, frame);
}

fn tick_network(mut network_tick: ResMut<NetworkTick>) {
    network_tick.0 += 1;
}

pub fn replicate<T: NetworkedFrame>(
    client: u64,
    tick: &NetworkTick,
    last_ticks: &LastNetworkTick,
    buffer: &NetworkFrameBuffer<T>,
) -> Result<Vec<u8>, io::Error> {
    // TODO: add cache for full frame or generating a frame with the same delta_tick
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
