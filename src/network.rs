use std::collections::HashMap;
use std::io;

use bevy::prelude::*;
use bit_serializer::{BitReader, BitWriter};

use crate::network_entity::{self, NetworkID};
use crate::sequence_buffer::SequenceBuffer;

pub struct NetworkTick(pub u16);
pub struct LastNetworkTick(pub HashMap<u64, u16>);

#[derive(Debug, Clone)]
pub enum SnapHeader {
    Full {
        tick: u16,
        entities: Vec<NetworkID>,
    },
    Delta {
        tick: u16,
        delta_tick: u16,
        entities: Vec<NetworkID>,
    },
}

pub trait Networked {
    type Component: Component + PartialEq + Clone + std::fmt::Debug;

    fn can_delta(&self, old: &Self::Component, new: &Self::Component) -> bool;
    fn write_delta(&self, old: &Self::Component, new: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error>;
    fn read_delta(&self, old: &Self::Component, reader: &mut BitReader) -> Result<Self::Component, io::Error>;
    fn write_full(&self, component: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error>;
    fn read_full(&self, reader: &mut BitReader) -> Result<Self::Component, io::Error>;

    fn apply_component(world: &mut World, entity: Entity, component: Self::Component) {
        world.entity_mut(entity).insert(component);
    }
}

#[derive(Debug)]
pub enum ComponentChange {
    NoChange,
    DeltaChange,
    FullChange,
    Removed,
}

pub struct FrameNetworkID(SequenceBuffer<Vec<NetworkID>>);

pub struct ComponentMapping<T> {
    list: Vec<Option<T>>,
    map: HashMap<NetworkID, T>,
}

pub struct FrameComponent<T>(SequenceBuffer<ComponentMapping<T>>);

impl TryFrom<u8> for ComponentChange {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use ComponentChange::*;

        match value {
            0 => Ok(FullChange),
            1 => Ok(Removed),
            2 => Ok(NoChange),
            3 => Ok(DeltaChange),
            _ => Err("Invalid ComponentChange id"),
        }
    }
}

impl SnapHeader {
    pub fn tick(&self) -> u16 {
        match self {
            SnapHeader::Full { tick, .. } => *tick,
            SnapHeader::Delta { tick, .. } => *tick,
        }
    }

    pub fn entities(&self) -> &[NetworkID] {
        match self {
            SnapHeader::Full { entities, .. } => entities,
            SnapHeader::Delta { entities, .. } => entities,
        }
    }
}

pub fn start_network_frame(world: &mut World) {
    let networked_entities = get_networked_entities(world);
    let tick = world.get_resource::<NetworkTick>().unwrap().0;

    let mut frame = world.get_resource_mut::<FrameNetworkID>().unwrap();
    frame.0.insert(tick, networked_entities);
}

pub fn add_component_network_frame<T: Component + Clone>(world: &mut World) {
    let tick = world.get_resource::<NetworkTick>().unwrap().0;
    let networked_entities = {
        let frame = world.get_resource::<FrameNetworkID>().unwrap();
        frame.0.get(tick).unwrap().clone()
    };

    let mut query = world.query_filtered::<Option<&T>, With<NetworkID>>();
    let list: Vec<Option<T>> = query.iter(world).map(|c| c.cloned()).collect();
    let mut map: HashMap<NetworkID, T> = HashMap::new();

    for (index, component) in list.iter().enumerate() {
        if let Some(component) = component {
            map.insert(networked_entities[index], component.clone());
        }
    }

    let mut frame = world.get_resource_mut::<FrameComponent<T>>().unwrap();
    let component_mapping = ComponentMapping { list, map };
    frame.0.insert(tick, component_mapping);
}

fn get_networked_entities(world: &mut World) -> Vec<NetworkID> {
    let mut query = world.query::<&NetworkID>();
    query.iter(world).copied().collect()
}

pub fn init_frame_header(delta_tick: Option<u16>, world: &mut World) -> SnapHeader {
    let tick = world.get_resource::<NetworkTick>().unwrap().0;
    let entities = {
        let frame = world.get_resource::<FrameNetworkID>().unwrap();
        frame.0.get(tick).unwrap().clone()
    };

    if let Some(delta_tick) = delta_tick {
        SnapHeader::Delta {
            tick,
            entities,
            delta_tick,
        }
    } else {
        SnapHeader::Full { tick, entities }
    }
}

pub fn write_snap_header(writer: &mut BitWriter, header: SnapHeader) -> Result<(), io::Error> {
    match header {
        SnapHeader::Full { tick, entities } => {
            writer.write_bool(true)?;
            writer.write_varint_u16(tick)?;
            writer.write_varint_u16(entities.len() as u16)?;
            for network_id in entities.iter() {
                writer.write_bits(network_id.0 as u32, 12)?;
            }
        }
        SnapHeader::Delta {
            tick,
            delta_tick,
            entities,
        } => {
            writer.write_bool(false)?;
            writer.write_varint_u16(tick)?;
            writer.write_varint_u16(delta_tick)?;
            writer.write_varint_u16(entities.len() as u16)?;
            for network_id in entities.iter() {
                writer.write_bits(network_id.0 as u32, 12)?;
            }
        }
    }

    Ok(())
}

pub fn read_snap_header(reader: &mut BitReader) -> Result<SnapHeader, io::Error> {
    let is_full = reader.read_bool()?;
    if is_full {
        let tick = reader.read_varint_u16()?;
        let len = reader.read_varint_u16()? as usize;
        if len > network_entity::MAX_LENGTH {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "network entities length above limit"));
        }
        let mut entities = Vec::with_capacity(len);
        for _ in 0..len {
            let network_id = reader.read_bits(12)? as u16;
            let network_id = NetworkID(network_id);
            entities.push(network_id);
        }

        Ok(SnapHeader::Full { tick, entities })
    } else {
        let tick = reader.read_varint_u16()?;
        let delta_tick = reader.read_varint_u16()?;
        let len = reader.read_varint_u16()? as usize;
        if len > network_entity::MAX_LENGTH {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "network entities length above limit"));
        }
        let mut entities = Vec::with_capacity(len);
        for _ in 0..len {
            let network_id = reader.read_bits(12)? as u16;
            let network_id = NetworkID(network_id);
            entities.push(network_id);
        }

        Ok(SnapHeader::Delta {
            tick,
            delta_tick,
            entities,
        })
    }
}

pub fn write_full_component<T: Networked>(networked: T, writer: &mut BitWriter, world: &mut World) -> Result<(), io::Error> {
    let tick = world.get_resource::<NetworkTick>().unwrap().0;
    let components = {
        let frame = world.get_resource::<FrameComponent<T::Component>>().unwrap();
        frame.0.get(tick).unwrap()
    };
    for component in components.list.iter() {
        match component {
            Some(_) => writer.write_bits(ComponentChange::FullChange as u32, 2)?,
            None => writer.write_bits(ComponentChange::Removed as u32, 2)?,
        }
    }

    for component in components.list.iter() {
        if let Some(component) = component {
            networked.write_full(component, writer)?;
        }
    }

    Ok(())
}

pub fn read_full_component<T: Networked>(
    networked: T,
    entities: &[NetworkID],
    reader: &mut BitReader,
    world: &mut World,
) -> Result<Vec<Option<T::Component>>, io::Error> {
    let mut changes = Vec::with_capacity(entities.len());
    for _ in 0..entities.len() {
        let change = reader.read_bits(2)? as u8;
        // Reading 2 bits should always return a valid ComponentChange id
        let change = ComponentChange::try_from(change).unwrap();
        changes.push(change);
    }

    let mut components: Vec<Option<T::Component>> = Vec::with_capacity(entities.len());
    for (i, change) in changes.iter().enumerate() {
        match change {
            ComponentChange::FullChange => {
                let component = networked.read_full(reader)?;
                components.push(Some(component));
            }
            ComponentChange::Removed => {
                components.push(None);
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid ComponentChange for full snapshot",
                ))
            }
        }
    }

    Ok(components)
}

fn save_component_frame<T: Component + Clone>(tick: u16, world: &mut World, entities: &[NetworkID], list: Vec<Option<T>>) {
    let mut frame = world.get_resource_mut::<FrameComponent<T>>().unwrap();

    let mut map: HashMap<NetworkID, T> = HashMap::new();
    for (i, component) in list.iter().enumerate() {
        if let Some(component) = component {
            map.insert(entities[i], component.clone());
        }
    }

    let component_mapping = ComponentMapping { list, map };
    frame.0.insert(tick, component_mapping);
}

/*
fn write_component_delta<T: Networked>(writer: &mut BitWriter, world: &mut World, delta_tick: u16) -> Result<(), io::Error> {
    let last_received_componets = world
        .get_resource::<NetworkComponentFrameBuffer<T::Component>>()
        .unwrap()
        .0
        .get(delta_tick)
        .unwrap()
        .clone();

    let mut query = world.query::<(Option<&T::Component>, &NetworkID)>();
    for (component, network_id) in query.iter(world) {
        match component {
            Some(component) => match last_received_componets.get(network_id) {
                Some(last_component) => {
                    if last_component == component {
                        writer.write_bits(ComponentChange::NoChange.as_u8() as u32, 2)?;
                    } else if T::can_delta(last_component, component) {
                        writer.write_bits(ComponentChange::DeltaChange.as_u8() as u32, 2)?;
                    } else {
                        writer.write_bits(ComponentChange::FullChange.as_u8() as u32, 2)?;
                    }
                }
                None => {
                    writer.write_bits(ComponentChange::FullChange.as_u8() as u32, 2)?;
                }
            },
            None => writer.write_bits(ComponentChange::Removed.as_u8() as u32, 2)?,
        }
    }

    for (component, network_id) in query.iter(world) {
        if let Some(component) = component {
            match last_received_componets.get(network_id) {
                Some(last_component) => {
                    if last_component == component {
                        continue;
                    }

                    if T::can_delta(last_component, component) {
                        T::write_delta(last_component, component, writer)?;
                    } else {
                        T::write_full(component, writer)?;
                    }
                }
                None => {
                    T::write_full(component, writer)?;
                }
            }
        }
    }

    Ok(())
}

fn read_component_delta<T: Networked>(
    reader: &mut BitReader,
    world: &World,
    delta_tick: u16,
    entities: &[NetworkID],
) -> Result<Vec<Option<T::Component>>, io::Error> {
    let mut changes = Vec::with_capacity(entities.len());
    for _ in 0..entities.len() {
        let change = reader.read_bits(2)? as u8;
        let change = ComponentChange::from_u8(change).unwrap();
        changes.push(change);
    }

    let component_sequence_buffer = world.get_resource::<NetworkComponentFrameBuffer<T::Component>>().unwrap();
    let delta_frame = component_sequence_buffer.0.get(delta_tick).unwrap();

    let mut components = Vec::with_capacity(entities.len());
    for (i, change) in changes.iter().enumerate() {
        match change {
            ComponentChange::FullChange => {
                let component = T::read_full(reader)?;
                components.push(Some(component));
            }
            ComponentChange::Removed => {
                components.push(None);
            }
            ComponentChange::DeltaChange => {
                let old_value = delta_frame.get(&entities[i]).unwrap();
                let new_value = T::read_delta(old_value, reader)?;
                components.push(Some(new_value));
            }
            ComponentChange::NoChange => {
                let old_value = delta_frame.get(&entities[i]).unwrap().clone();
                components.push(Some(old_value));
            }
        }
    }

    Ok(components)
}

pub fn serialize_delta_snap(world: &mut World, client: u64) -> Result<Vec<u8>, io::Error> {
    let mut writer = BitWriter::default();
    let tick: u16 = {
        let tick = world.get_resource::<NetworkTick>().unwrap();
        tick.0
    };

    let last_received_tick: Option<u16> = {
        let last_network_tick = world.get_resource::<LastNetworkTick>().unwrap();
        last_network_tick.0.get(&client).cloned()
    };

    let has_frame = match last_received_tick {
        None => false,
        Some(last_received_tick) => {
            let network_frames = world.get_resource::<NetworkFrames>().unwrap();
            network_frames.0.get(last_received_tick).is_some()
        }
    };
    let entities = get_networked_entities(world);

    match (last_received_tick, has_frame) {
        (Some(last_received_tick), true) => {
            println!("Should serialize_delta after");
            /*
            let created_entities = {
                let network_frames = world.get_resource::<NetworkFrames>().unwrap();
                let frame = network_frames.0.get(last_received_tick).unwrap();
                let mut created_entities: Vec<bool> = Vec::with_capacity(entities.len());
                for delta_entity in frame.entities().iter() {
                    let is_new = !entities.contains(delta_entity);
                    created_entities.push(is_new);
                }
                created_entities
            };*/
            let header = SnapHeader::Delta {
                tick,
                delta_tick: last_received_tick,
                entities,
            };

            write_snap_header(&mut writer, &header)?;

            // let mut network_frames = world.get_resource_mut::<NetworkFrames>().unwrap();
            // network_frames.0.insert(tick, header);

            write_component_delta::<TransformNetworked>(&mut writer, world, tick)?;
        }
        _ => {
            println!("Should serialize_full first");
            let header = SnapHeader::Full { tick, entities };
            write_snap_header(&mut writer, &header)?;

            // Serialize header
            write_component_full::<TransformNetworked>(&mut writer, world, tick)?;
        }
    }

    writer.consume()
}

pub fn deserialize_delta_snap(buffer: Vec<u8>, world: &mut World) -> Result<(), io::Error> {
    let mut reader = BitReader::new(&buffer)?;
    let header = read_snap_header(&mut reader)?;
    println!("{:#?}", header);
    match header {
        SnapHeader::Full { tick, entities } => {
            let components = read_component_full::<TransformNetworked>(&mut reader, entities.len())?;
            add_network_frame(world, &components, &entities, tick);

            println!("{:#?}", components);
        }
        SnapHeader::Delta {
            tick,
            delta_tick,
            entities,
            ..
        } => {
            let components = read_component_delta::<TransformNetworked>(&mut reader, world, delta_tick, &entities)?;
            add_network_frame(world, &components, &entities, tick);

            println!("{:#?}", components);
        }
    }

    Ok(())
}

*/
