use crate::{
    network_entity::{cleanup_network_entity_system, track_network_entity_system, NetworkEntities},
    sequence_buffer::SequenceBuffer,
    NetworkedFrame,
};
use bevy::{prelude::*, time::FixedTimestep};
use bit_serializer::BitWriter;
use iyes_loopless::prelude::*;
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
    // struct DeltaCache(HashMap<delta_tick, Bytes>), return Bytes instead of Vec<u8>
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

pub struct ReplicateServerStatePlugin<T, S> {
    config: ReplicateServerConfig,
    data: PhantomData<T>,
    state: PhantomData<S>,
}

pub struct ReplicateServerConfig {
    pub tick_rate: f64,
    pub buffer_size: usize,
}

impl<T, S> Default for ReplicateServerStatePlugin<T, S> {
    fn default() -> Self {
        Self {
            config: Default::default(),
            data: PhantomData,
            state: PhantomData,
        }
    }
}


impl Default for ReplicateServerConfig {
    fn default() -> Self {
        Self { tick_rate: 20., buffer_size: 60 }
    }
}

impl<T: NetworkedFrame, S: bevy::ecs::schedule::StateData> ReplicateServerStatePlugin<T, S> {
    pub fn new(config: ReplicateServerConfig) -> Self {
        Self {
            config,
            data: PhantomData,
            state: PhantomData,
        }
    }

    pub fn build(self, app: &mut App, state: S) {
        app.add_enter_system(state.clone(), resources_setup::<T>);
        app.add_exit_system(state.clone(), resources_cleanup::<T>);

        app.add_system_to_stage(
            CoreStage::PreUpdate,
            iyes_loopless::condition::IntoConditionalExclusiveSystem::run_in_state(tick_network, state.clone())
                .at_end()
                .with_run_criteria(FixedTimestep::steps_per_second(self.config.tick_rate)),
        );
        app.add_system_to_stage(
            CoreStage::Update,
            iyes_loopless::condition::IntoConditionalExclusiveSystem::run_in_state(generate_network_frame::<T>, state.clone())
                .at_end()
                .with_run_criteria(FixedTimestep::steps_per_second(self.config.tick_rate)),
        );

        app.add_system(track_network_entity_system.run_in_state(state.clone()));
        app.add_system(cleanup_network_entity_system.run_in_state(state));

        app.insert_resource(self.config);
    }
}

fn resources_setup<T: NetworkedFrame>(mut commands: Commands, config: Res<ReplicateServerConfig>) {
    commands.insert_resource(NetworkEntities::default());
    commands.insert_resource(NetworkTick(0));
    commands.insert_resource(LastNetworkTick(HashMap::new()));

    let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(config.buffer_size);
    commands.insert_resource(NetworkFrameBuffer(buffer));
}

fn resources_cleanup<T: NetworkedFrame>(mut commands: Commands) {
    commands.remove_resource::<NetworkEntities>();
    commands.remove_resource::<NetworkTick>();
    commands.remove_resource::<LastNetworkTick>();
    commands.remove_resource::<NetworkFrameBuffer<T>>();
}
