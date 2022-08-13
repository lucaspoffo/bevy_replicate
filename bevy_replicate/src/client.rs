use bevy::{ecs::system::SystemState, prelude::*};
use bit_serializer::BitReader;

use std::{collections::HashMap, io, marker::PhantomData, time::Duration};

use crate::{NetworkID, NetworkedFrame, sequence_buffer::SequenceBuffer, NetworkFrameBuffer};

#[doc(hidden)]
pub struct NetworkMapping(pub HashMap<NetworkID, Entity>);

pub struct TickInterpolation(pub f32);

pub struct LastReceivedNetworkTick(pub Option<u64>);

#[derive(Debug, Default)]
struct ClientInfo {
    tick_duration: Duration,
    desired_delay: Duration,
    last_received_tick: Option<u64>,
    last_applied_tick: Option<u64>,
    current_playback_time: Duration,
    desired_playback_time: Duration,
}

pub struct ReplicateClientPlugin<T> {
    tick_rate: f64,
    data: PhantomData<T>,
}

impl<T> Default for ReplicateClientPlugin<T> {
    fn default() -> Self {
        Self {
            tick_rate: 20.,
            data: PhantomData,
        }
    }
}

impl<T: NetworkedFrame> Plugin for ReplicateClientPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<T>();
        app.insert_resource(LastReceivedNetworkTick(None));
        app.insert_resource(NetworkMapping(HashMap::new()));
        app.insert_resource(TickInterpolation(0.));

        app.insert_resource(ClientInfo {
            tick_duration: Duration::from_secs_f64(1. / self.tick_rate),
            desired_delay: Duration::from_millis(100),
            ..default()
        });

        let buffer: SequenceBuffer<T> = SequenceBuffer::with_capacity(60);
        app.insert_resource(NetworkFrameBuffer(buffer));

        app.add_system_to_stage(CoreStage::PreUpdate, update_info::<T>.exclusive_system().at_end());
    }
}

pub fn process_snapshot<T: NetworkedFrame>(buffer: Vec<u8>, world: &mut World) -> Result<(), io::Error> {
    let mut reader = BitReader::new(&buffer)?;
    let frame = T::read_frame(&mut reader, world)?;

    let mut info = world.resource_mut::<ClientInfo>();
    match info.last_received_tick {
        Some(tick) => {
            if frame.tick() > tick {
                info.last_received_tick = Some(frame.tick())
            }
        }
        None => {
            info.last_received_tick = Some(frame.tick());
            info.current_playback_time = (info.tick_duration * frame.tick() as u32).saturating_sub(info.desired_delay);
        }
    }

    let frame_buffer = &mut world.resource_mut::<NetworkFrameBuffer<T>>().0;
    frame_buffer.insert(frame.tick(), frame);

    Ok(())
}

fn update_info<T: NetworkedFrame>(world: &mut World) {
    let tick = {
        let mut system_state: SystemState<(ResMut<ClientInfo>, Res<Time>, Res<NetworkFrameBuffer<T>>, ResMut<TickInterpolation>)> =
            SystemState::new(world);
        let (mut info, time, buffer, mut tick_interpolation) = system_state.get_mut(world);
        if let Some(last_received_tick) = info.last_received_tick {
            // TODO: need a better way to determine the desired playback time
            info.desired_playback_time = (last_received_tick as u32 * info.tick_duration).saturating_sub(info.desired_delay);
        }

        if let (Some(last_applied), Some(last_received)) = (info.last_applied_tick, info.last_received_tick) {
            if last_received - last_applied > 10 {
                println!("received: {}, applied: {}", last_received, last_applied);
            }
        }

        let scale: f64 = if info.desired_playback_time > info.current_playback_time {
            1.02
        } else if info
            .desired_playback_time
            .as_millis()
            .abs_diff(info.current_playback_time.as_millis())
            < 8
        {
            1.0
        } else {
            0.98
        };

        info.current_playback_time += time.delta().mul_f64(scale);

        let mut snapshot_times: Vec<(u64, Duration)> = buffer
            .0
            .entries()
            .iter()
            .filter_map(|snap| if let Some(snap) = snap { Some((snap.tick(), snap.tick() as u32 * info.tick_duration)) } else { None })
            .collect();
        snapshot_times.sort_by(|(tick_a, _), (tick_b, _)| tick_a.cmp(tick_b));

        if snapshot_times.len() == 0 {
            return;
        }

        let i = snapshot_times.partition_point(|(_, time)| *time < info.current_playback_time);

        let (tick, interpolation) = if i == 0 {
            // current playback time is behind oldest snapshot
            (snapshot_times[i].0, 0.0)
        } else if i == snapshot_times.len() {
            // current playback time is ahead of newest snapshot
            (snapshot_times[i - 1].0, 0.0)
        } else {
            // current playback time is between two snapshots
            let fract = info.current_playback_time.as_secs_f64() - snapshot_times[i - 1].1.as_secs_f64();
            let whole = snapshot_times[i].1.as_secs_f64() - snapshot_times[i - 1].1.as_secs_f64();
            (snapshot_times[i].0, fract / whole)
        };

        tick_interpolation.0 = interpolation as f32;

        tick
    };

    let last_applied_tick = { world.resource::<ClientInfo>().last_applied_tick.clone() };
    let apply_tick = match last_applied_tick {
        None => true,
        Some(last_applied_tick) => last_applied_tick != tick,
    };
    if apply_tick {
        world.resource_scope(|world, buffer: Mut<NetworkFrameBuffer<T>>| {
            world.resource_mut::<ClientInfo>().last_applied_tick = Some(tick);
            let frame = buffer.0.get(tick).unwrap();
            frame.apply_in_world(world);
        });
    }
}
