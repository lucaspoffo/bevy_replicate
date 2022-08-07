pub mod network_entity;
pub mod networked_transform;
pub mod sequence_buffer;
mod network_frame;
mod network;

use bevy::prelude::*;
use bit_serializer::{BitReader, BitWriter};
use network::{start_network_frame, add_component_network_frame, init_frame_header, write_snap_header, write_full_component};
use network_entity::{NetworkEntities, NetworkID};
use networked_transform::TransformNetworked;
use sequence_buffer::SequenceBuffer;

use std::collections::HashMap;
use std::io;

use crate::network::{read_snap_header, SnapHeader, FrameNetworkID};

pub struct NetworkTick(pub u16);

struct ReplicatePlugin;

impl Plugin for ReplicatePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkEntities::default());
    }
}

pub fn start_network(world: &mut World) {
    start_network_frame(world);
    add_component_network_frame::<Transform>(world);
}

pub fn serialize_full_snap(mut writer: &mut BitWriter, world: &mut World) -> Result<(), io::Error> {
    let tick = {
        let tick = world.get_resource::<NetworkTick>().unwrap();
        tick.0
    };

    let header = init_frame_header(None, world);
    write_snap_header(writer, header)?;

    write_full_component(TransformNetworked, writer, world)?;

    Ok(())
}

pub fn deserialize_full_snap(buffer: Vec<u8>) -> Result<(), io::Error> {
    let mut reader = BitReader::new(&buffer)?;

    let header = read_snap_header(&mut reader)?;
    println!("{:#?}", header);
    {
        let tick = header.tick();
        let entities = header.entities();
        // let mut frame = world.get_resource_mut::<FrameNetworkID>().unwrap();
        // frame.0.insert(tick, entities.clone());
    }
    match header {
        SnapHeader::Full { tick, entities } => {
            
        }
        SnapHeader::Delta { tick, delta_tick, entities } => {
            todo!()
        }
    }

    // let components = read_component_full::<TransformNetworked>(&mut reader, header.entities().len())?;
    // println!("{:#?}", components);

    Ok(())
}



