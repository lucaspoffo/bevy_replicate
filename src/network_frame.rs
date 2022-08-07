use bit_serializer::{BitReader, BitWriter};
use std::io;

use crate::NetworkID;

#[derive(Debug)]
pub enum ComponentChange {
    FullChange,
    Removed,
    NoChange,
    DeltaChange,
}

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

pub trait NetworkedFrame: std::fmt::Debug + Sized + Send + Sync + 'static {
    fn generate_frame(tick: u16, world: &mut bevy::prelude::World) -> Self;
    fn apply_in_world(&self, world: &mut bevy::prelude::World);
    fn write_full_frame(&self, writer: &mut BitWriter) -> Result<(), io::Error>;
    fn read_frame(reader: &mut BitReader, world: &mut bevy::prelude::World) -> Result<Self, io::Error>;
}

pub trait Networked {
    type Component: bevy::prelude::Component + PartialEq + Clone + std::fmt::Debug;

    fn can_delta(_old: &Self::Component, _new: &Self::Component) -> bool {
        false
    }

    fn write_delta(_old: &Self::Component, _new: &Self::Component, _writer: &mut BitWriter) -> Result<(), io::Error> {
        panic!("Delta encoding not implemented for component");
    }

    fn read_delta(_old: &Self::Component, _reader: &mut BitReader) -> Result<Self::Component, io::Error> {
        panic!("Delta encoding not implemented for component");
    }

    fn write_full(component: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error>;
    fn read_full(reader: &mut BitReader) -> Result<Self::Component, io::Error>;
}

#[macro_export]
macro_rules! network_frame {
    ($($type:ty),+) => {
        paste::paste! {
            #[derive(Debug)]
            pub struct NetworkFrame {
                tick: u16,
                entities: Vec<$crate::NetworkID>,
                $(
                    [<$type:snake:lower>]: Vec<Option<<$type as $crate::Networked>::Component>>,
                )*
            }

            impl $crate::NetworkedFrame for NetworkFrame {
                fn generate_frame(tick: u16, world: &mut $crate::bevy::prelude::World) -> Self {
                    let entities = $crate::networked_entities(world);
                    $(
                        let [<$type:snake:lower>] = {
                            let mut query = world.query_filtered::<Option<&<$type as $crate::Networked>::Component>, $crate::bevy::prelude::With<$crate::NetworkID>>();
                            query.iter(world).map(|c| c.cloned()).collect()
                        };
                    )*

                    Self {
                        tick,
                        entities,
                        $([<$type:snake:lower>],)*
                    }
                }


                fn apply_in_world(&self, world: &mut bevy::prelude::World) {
                    world.resource_scope(|world, mut mapping: Mut<$crate::NetworkMapping>| {
                        // Remove entities
                        mapping.0.retain(|network_id, entity| {
                            let removed = !self.entities.contains(network_id);
                            if removed {
                                world.despawn(*entity);
                            }

                            !removed
                        });

                        // Create new networked entities
                        for network_id in self.entities.iter() {
                            if !mapping.0.contains_key(network_id) {
                                let entity_id = world.spawn().insert($crate::NetworkID(network_id.0)).id();
                                mapping.0.insert(*network_id, entity_id);
                            }
                        }

                        // Replicate components
                        $(
                            for (i, network_id) in self.entities.iter().enumerate() {
                                if let Some(component) = &self.[<$type:snake:lower>][i] {
                                    // Should always exist a mapped entity by now
                                    let mapped_entity = mapping.0.get(network_id).unwrap();
                                    let mut entity_mut = world.entity_mut(*mapped_entity);
                                    entity_mut.insert(component.clone());
                                }
                            }
                        )*
                    });
                }

                fn write_full_frame(&self, writer: &mut $crate::bit_serializer::BitWriter) -> Result<(), std::io::Error> {
                    $crate::write_frame_header(writer, self.tick, None, &self.entities)?;

                    $(
                        $crate::write_full_component::<$type>(writer, &self.[<$type:snake:lower>])?;
                    )*

                    Ok(())
                }

                fn read_frame(reader: &mut $crate::bit_serializer::BitReader, world: &mut $crate::bevy::prelude::World) -> Result<Self, std::io::Error> {
                    let header = $crate::read_frame_header(reader)?;
                    if let Some(delta_tick) = header.delta_tick {
                        todo!()
                    } else {
                        $(
                            let [<$type:snake:lower>] = $crate::read_full_component::<$type>(reader, header.entities.len())?;
                        )*

                        Ok(Self {
                            tick: header.tick,
                            entities: header.entities,
                            $([<$type:snake:lower>],)*
                        })
                    }
                }
            }

        }
    }
}

pub fn networked_entities(world: &mut bevy::prelude::World) -> Vec<NetworkID> {
    let mut query = world.query::<&NetworkID>();
    query.iter(world).copied().collect()
}

pub fn write_frame_header(writer: &mut BitWriter, tick: u16, delta_tick: Option<u16>, entities: &[NetworkID]) -> Result<(), io::Error> {
    writer.write_bool(delta_tick.is_some())?;
    if let Some(delta_tick) = delta_tick {
        writer.write_varint_u16(delta_tick)?;
    }
    writer.write_varint_u16(tick)?;
    writer.write_varint_u16(entities.len() as u16)?;
    for network_id in entities.iter() {
        writer.write_bits(network_id.0 as u32, 12)?;
    }

    Ok(())
}

#[derive(Debug)]
pub struct FrameHeader {
    pub tick: u16,
    pub delta_tick: Option<u16>,
    pub entities: Vec<NetworkID>,
}

pub fn read_frame_header(reader: &mut BitReader) -> Result<FrameHeader, io::Error> {
    let is_delta = reader.read_bool()?;
    let delta_tick = if is_delta { Some(reader.read_varint_u16()?) } else { None };
    let tick = reader.read_varint_u16()?;
    let len = reader.read_varint_u16()? as usize;
    if len > 4096 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "network entities length above limit"));
    }
    let mut entities = Vec::with_capacity(len);
    for _ in 0..len {
        let network_id = reader.read_bits(12)? as u16;
        let network_id = NetworkID(network_id);
        entities.push(network_id);
    }

    Ok(FrameHeader {
        tick,
        delta_tick,
        entities,
    })
}

pub fn write_full_component<T: Networked>(writer: &mut BitWriter, components: &[Option<T::Component>]) -> Result<(), io::Error> {
    for component in components.iter() {
        match component {
            Some(_) => writer.write_bits(ComponentChange::FullChange as u32, 2)?,
            None => writer.write_bits(ComponentChange::Removed as u32, 2)?,
        }
    }

    for component in components.iter() {
        if let Some(component) = component {
            T::write_full(component, writer)?;
        }
    }

    Ok(())
}

pub fn read_full_component<T: Networked>(reader: &mut BitReader, entities_len: usize) -> Result<Vec<Option<T::Component>>, io::Error> {
    let mut changes = Vec::with_capacity(entities_len);
    for _ in 0..entities_len {
        let change = reader.read_bits(2)? as u8;
        // Reading 2 bits should always return a valid ComponentChange id
        let change = ComponentChange::try_from(change).unwrap();
        changes.push(change);
    }

    let mut components: Vec<Option<T::Component>> = Vec::with_capacity(entities_len);
    for change in changes.iter() {
        match change {
            ComponentChange::FullChange => {
                let component = T::read_full(reader)?;
                components.push(Some(component));
            }
            ComponentChange::Removed => {
                components.push(None);
            }
            _ => {
                println!("This error: {:?}", change);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid ComponentChange for full snapshot",
                ))
            }
        }
    }

    Ok(components)
}
