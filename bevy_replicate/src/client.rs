use bevy::prelude::*;
use bit_serializer::BitReader;

use std::{collections::HashMap, io, marker::PhantomData, time::Duration};

use crate::{sequence_buffer::SequenceBuffer, NetworkID, NetworkedFrame};

#[doc(hidden)]
pub struct NetworkMapping(pub HashMap<NetworkID, Entity>);

pub struct NetworkInterpolation(pub f32);

pub struct LastReceivedNetworkTick(pub Option<u64>);

pub struct ReplicateClientPlugin<T> {
    tick_rate: f64,
    playout_delay: Duration,
    buffer_size: usize,
    data: PhantomData<T>,
}

impl<T> Default for ReplicateClientPlugin<T> {
    fn default() -> Self {
        Self {
            tick_rate: 20.,
            playout_delay: Duration::from_millis(100),
            buffer_size: 60,
            data: PhantomData,
        }
    }
}

impl<T: NetworkedFrame> Plugin for ReplicateClientPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<T>();
        app.insert_resource(LastReceivedNetworkTick(None));
        app.insert_resource(NetworkMapping(HashMap::new()));
        app.insert_resource(NetworkInterpolation(0.));

        let interpolation_buffer = SnapshotInterpolationBuffer::<T>::new(self.buffer_size, self.playout_delay, self.tick_rate);
        app.insert_resource(interpolation_buffer);
        app.add_system_to_stage(CoreStage::PreUpdate, update_frame::<T>.exclusive_system().at_end());
    }
}

pub fn process_snapshot<T: NetworkedFrame>(buffer: Vec<u8>, world: &mut World) -> Result<(), io::Error> {
    let mut reader = BitReader::new(&buffer)?;
    let snapshot = T::read_frame(&mut reader, world)?;

    let mut last_received_tick = world.resource_mut::<LastReceivedNetworkTick>();
    match last_received_tick.0 {
        Some(tick) => {
            if snapshot.tick() > tick {
                last_received_tick.0 = Some(snapshot.tick())
            }
        }
        None => {
            last_received_tick.0 = Some(snapshot.tick());
        }
    }

    let current_time = world.resource::<Time>().time_since_startup();
    let mut interpolation_buffer = world.resource_mut::<SnapshotInterpolationBuffer<T>>();
    interpolation_buffer.add_snapshot(current_time, snapshot);

    Ok(())
}

#[doc(hidden)]
#[derive(Debug)]
pub struct SnapshotInterpolationBuffer<T> {
    start_time: Duration,
    playout_delay: Duration,
    stopped: bool,
    start_tick: u64,
    interpolating: bool,
    tick_rate: f64,
    interpolation_start_tick: u64,
    interpolation_end_tick: u64,
    interpolation_start_time: Duration,
    interpolation_end_time: Duration,
    tick_duration: Duration,
    pub buffer: SequenceBuffer<T>,
}

fn update_frame<T: NetworkedFrame>(world: &mut World) {
    world.resource_scope(|world, mut interpolation_buffer: Mut<SnapshotInterpolationBuffer<T>>| {
        let current_time = world.resource::<Time>().time_since_startup();
        interpolation_buffer.update(current_time, world);
    })
}

impl<T: NetworkedFrame> SnapshotInterpolationBuffer<T> {
    pub(crate) fn new(buffer_capacity: usize, playout_delay: Duration, send_rate: f64) -> Self {
        Self {
            start_time: Duration::ZERO,
            start_tick: 0,
            playout_delay,
            stopped: true,
            interpolating: false,
            tick_rate: send_rate,
            interpolation_start_tick: 0,
            interpolation_end_tick: 0,
            interpolation_start_time: Duration::ZERO,
            interpolation_end_time: Duration::ZERO,
            tick_duration: Duration::from_secs_f64(1. / send_rate),
            buffer: SequenceBuffer::with_capacity(buffer_capacity),
        }
    }

    pub(crate) fn add_snapshot(&mut self, current_time: Duration, snapshot: T) {
        let tick = snapshot.tick();
        if self.stopped {
            self.start_tick = tick;
            self.start_time = current_time;
            self.stopped = false;
        }
        self.buffer.insert(tick, snapshot);
    }

    fn update(&mut self, current_time: Duration, world: &mut World) {
        // No snapshot received
        if self.stopped {
            return;
        }

        let mut time = match current_time.checked_sub(self.start_time + self.playout_delay) {
            Some(time) => time,
            // Too early to display something
            None => return,
        };

        let frames_since_start = time.mul_f64(self.tick_rate);
        let interpolation_tick = frames_since_start.as_secs_f64().floor() as u64 + self.start_tick;
        if self.interpolating {
            let n: u64 = (self.playout_delay.as_secs_f64() * self.tick_rate).floor() as u64;

            if interpolation_tick.abs_diff(self.interpolation_start_tick) > n {
                self.interpolating = false;
            }
        }

        if !self.interpolating {
            if let Some(_) = self.buffer.get(interpolation_tick) {
                self.interpolation_start_tick = interpolation_tick;
                self.interpolation_end_tick = interpolation_tick;

                self.interpolation_start_time = frames_since_start.div_f64(self.tick_rate);
                self.interpolation_end_time = self.interpolation_start_time;

                self.interpolating = true;
            }
        }

        if !self.interpolating {
            return;
        }

        if time < self.interpolation_start_time {
            time = self.interpolation_start_time;
        }

        // If current time >= end time we need to start a new interpolation
        // from the previous end time to the next sample that exist up to n samples ahead,
        // where n is the # of frames in the playout delay buffer, rounded up.
        if time >= self.interpolation_end_time {
            let n = (self.playout_delay.as_secs_f64() * self.tick_rate).floor() as usize;
            self.interpolation_start_tick = self.interpolation_end_tick;
            self.interpolation_start_time = self.interpolation_end_time;

            for i in 1..=n {
                let end_tick = self.interpolation_start_tick + i as u64;
                if let Some(snapshot) = self.buffer.get(end_tick) {
                    self.interpolation_end_tick = end_tick;
                    self.interpolation_end_time = self.interpolation_start_time + (self.tick_duration * i as u32);
                    snapshot.apply_in_world(world);
                    break;
                }
            }
        }

        // Couldn't start a new interpolation
        if time >= self.interpolation_end_time {
            return;
        }

        // Update interpolation value
        let fract = time.as_secs_f32() - self.interpolation_start_time.as_secs_f32();
        let whole = self.interpolation_end_time.as_secs_f32() - self.interpolation_start_time.as_secs_f32();
        let t = (fract / whole).clamp(0.0, 1.0);
        let mut interpolation = world.resource_mut::<NetworkInterpolation>();
        interpolation.0 = t;
    }
}
