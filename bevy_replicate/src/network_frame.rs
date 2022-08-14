use bevy::ecs::world::EntityMut;
use bevy::prelude::*;
use bit_serializer::{BitReader, BitWriter};
use std::{collections::HashMap, io};

use crate::{network_entity, NetworkID};

#[derive(Debug, Clone, Copy)]
pub enum ComponentChange {
    FullChange,
    NoComponent,
    NoChange,
    DeltaChange,
}

impl TryFrom<u8> for ComponentChange {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use ComponentChange::*;

        match value {
            0 => Ok(FullChange),
            1 => Ok(NoComponent),
            2 => Ok(NoChange),
            3 => Ok(DeltaChange),
            _ => Err("Invalid ComponentChange id"),
        }
    }
}

pub trait NetworkedFrame: std::fmt::Debug + Clone + Sized + Send + Sync + 'static {
    fn tick(&self) -> u64;
    fn generate_frame(tick: u64, world: &mut bevy::prelude::World) -> Self;
    fn apply_in_world(&self, world: &mut bevy::prelude::World);
    fn write_full_frame(&self, writer: &mut BitWriter) -> Result<(), io::Error>;
    fn write_delta_frame(&self, writer: &mut BitWriter, delta_frame: &Self) -> Result<(), io::Error>;
    fn read_frame(reader: &mut BitReader, world: &mut bevy::prelude::World) -> Result<Self, io::Error>;
}

pub trait NetworkedComponent {
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

    fn apply(mut entity_mut: EntityMut<'_>, component: &Self::Component) {
        entity_mut.insert(component.clone());
    }
}

/// Generate a NetworkFrame that contains all desired networked components
/// Usage: network_frame!(ComponentA, ComponentB);
/// All components need to implement the NetworkedComponent trait.
// This traits generate an struct like:
// struct NetworkFrame {
//    tick: u64,
//    entities: Vec<NetworkID>,
//    component_a: Vec<Option<ComponentA>>
//    component_b: Vec<Option<ComponentA>>
// }
//
// Instead of Vec<(NetworkID, Option<ComponentA>, Option<ComponentB>)> we store them in separeted vecs,
// Easier to get and apply with ecs.
#[macro_export]
macro_rules! network_frame {
    ($($type:ty),+) => {
        paste::paste! {
            #[derive(Debug, PartialEq, Clone)]
            pub struct NetworkFrame {
                tick: u64,
                entities: Vec<$crate::NetworkID>,
                $(
                    [<$type:snake:lower>]: Vec<Option<<$type as $crate::NetworkedComponent>::Component>>,
                )*
            }

            impl $crate::NetworkedFrame for NetworkFrame {
                fn tick(&self) -> u64 {
                    self.tick
                }

                fn generate_frame(tick: u64, world: &mut $crate::bevy::prelude::World) -> Self {
                    let entities = $crate::networked_entities(world);
                    $(
                        let [<$type:snake:lower>] = $crate::networked_components::<$type>(world);
                    )*

                    Self {
                        tick,
                        entities,
                        $([<$type:snake:lower>],)*
                    }
                }


                fn apply_in_world(&self, world: &mut $crate::bevy::prelude::World) {
                    world.resource_scope(|world, mut mapping: $crate::bevy::prelude::Mut<$crate::client::NetworkMapping>| {
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
                                    let entity_mut = world.entity_mut(*mapped_entity);
                                    <$type as $crate::NetworkedComponent>::apply(entity_mut, component);
                                }
                            }
                        )*
                    });
                }

                fn write_full_frame(&self, writer: &mut $crate::BitWriter) -> Result<(), std::io::Error> {
                    $crate::write_frame_header(writer, self.tick, None, &self.entities)?;

                    $(
                        $crate::write_full_component::<$type>(writer, &self.[<$type:snake:lower>])?;
                    )*

                    Ok(())
                }

                fn write_delta_frame(&self, writer: &mut $crate::BitWriter, delta_frame: &Self) -> Result<(), std::io::Error> {
                    $crate::write_frame_header(writer, self.tick, Some(delta_frame.tick), &self.entities)?;
                    let delta_mapping = $crate::generate_delta_mapping(&delta_frame.entities, &self.entities);

                    $(
                        $crate::write_delta_component::<$type>(
                            writer,
                            &self.entities,
                            &self.[<$type:snake:lower>],
                            &delta_frame.[<$type:snake:lower>],
                            &delta_mapping
                        )?;
                    )*

                    Ok(())
                }

                fn read_frame(reader: &mut $crate::BitReader, world: &mut $crate::bevy::prelude::World) -> Result<Self, std::io::Error> {
                    let header = $crate::read_frame_header(reader)?;
                    if let Some(delta_tick) = header.delta_tick {
                        let frame_buffer = world.resource::<$crate::client::SnapshotInterpolationBuffer<Self>>();
                        if let Some(delta_frame) = frame_buffer.buffer.get(delta_tick) {
                            let delta_mapping = $crate::generate_delta_mapping(&delta_frame.entities, &header.entities);
                            $(
                                let [<$type:snake:lower>] = $crate::read_delta_component::<$type>(
                                    reader,
                                    &header.entities,
                                    &delta_frame.[<$type:snake:lower>],
                                    &delta_mapping
                                )?;
                            )*

                            Ok(Self {
                                tick: header.tick,
                                entities: header.entities,
                                $([<$type:snake:lower>],)*
                            })
                        } else {
                            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Delta frame not available"));
                        }
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

pub fn networked_entities(world: &mut World) -> Vec<NetworkID> {
    let mut query = world.query::<&NetworkID>();
    query.iter(world).copied().collect()
}

pub fn networked_components<T: NetworkedComponent>(world: &mut World) -> Vec<Option<T::Component>> {
    let mut query = world.query_filtered::<Option<&T::Component>, With<NetworkID>>();
    query.iter(world).map(|c| c.cloned()).collect()
}

pub fn write_frame_header(writer: &mut BitWriter, tick: u64, delta_tick: Option<u64>, entities: &[NetworkID]) -> Result<(), io::Error> {
    writer.write_bool(delta_tick.is_some())?;
    if let Some(delta_tick) = delta_tick {
        writer.write_varint_u64(delta_tick)?;
    }
    writer.write_varint_u64(tick)?;
    writer.write_varint_u16(entities.len() as u16)?;
    for network_id in entities.iter() {
        writer.write_bits(network_id.0 as u32, 12)?;
    }

    Ok(())
}

#[derive(Debug)]
pub struct FrameHeader {
    pub tick: u64,
    pub delta_tick: Option<u64>,
    pub entities: Vec<NetworkID>,
}

pub fn read_frame_header(reader: &mut BitReader) -> Result<FrameHeader, io::Error> {
    let is_delta = reader.read_bool()?;

    let delta_tick = if is_delta {
        let delta_tick = reader.read_varint_u64()?;
        Some(delta_tick)
    } else {
        None
    };

    let tick = reader.read_varint_u64()?;
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

    Ok(FrameHeader {
        tick,
        delta_tick,
        entities,
    })
}

// When serializing a Vec<Option<Component>> without delta, we use 1 bit for each component to
// check if there is Some(component) and do a full write.
pub fn write_full_component<T: NetworkedComponent>(writer: &mut BitWriter, components: &[Option<T::Component>]) -> Result<(), io::Error> {
    for component in components.iter() {
        writer.write_bool(component.is_some())?;
    }

    for component in components.iter().flatten() {
        T::write_full(component, writer)?;
    }

    Ok(())
}

pub fn read_full_component<T: NetworkedComponent>(
    reader: &mut BitReader,
    entities_len: usize,
) -> Result<Vec<Option<T::Component>>, io::Error> {
    let mut has_components = Vec::with_capacity(entities_len);
    for _ in 0..entities_len {
        let has_component = reader.read_bool()?;
        has_components.push(has_component);
    }

    let mut components: Vec<Option<T::Component>> = Vec::with_capacity(entities_len);
    for &has_component in has_components.iter() {
        if has_component {
            let component = T::read_full(reader)?;
            components.push(Some(component));
        } else {
            components.push(None);
        }
    }

    Ok(components)
}

pub fn generate_delta_mapping(previous_entities: &[NetworkID], current_entities: &[NetworkID]) -> HashMap<NetworkID, usize> {
    let mut map: HashMap<NetworkID, usize> = HashMap::new();
    for new in current_entities.iter() {
        if let Some(position) = previous_entities.iter().position(|old| old == new) {
            map.insert(*new, position);
        }
    }
    map
}

// When serializing a Vec<Option<Component>> with delta, we use 2 bits to see what happened since
// the delta frame:
//
//   FullChange  -> We can't delta with the old component or there is none to compare, full write
//                  the component
//   NoComponent -> No component in this frame, write nothing
//   NoChange    -> The component is the same, write nothing
//   DeltaChange -> The component has change and we can delta encode with the old one, delta write
//
pub fn write_delta_component<T: NetworkedComponent>(
    writer: &mut BitWriter,
    entities: &[NetworkID],
    current_components: &[Option<T::Component>],
    previous_components: &[Option<T::Component>],
    delta_mapping: &HashMap<NetworkID, usize>,
) -> Result<(), io::Error> {
    let mut changes: Vec<ComponentChange> = Vec::with_capacity(current_components.len());
    let mut write_change = |change: ComponentChange| -> Result<(), io::Error> {
        changes.push(change);
        writer.write_bits(change as u32, 2)
    };
    for (i, current) in current_components.iter().enumerate() {
        let previous = delta_mapping
            .get(&entities[i])
            .and_then(|index| previous_components[*index].as_ref());
        match (previous, current) {
            (_, None) => write_change(ComponentChange::NoComponent)?,
            (None, Some(_)) => write_change(ComponentChange::FullChange)?,
            (Some(previous), Some(current)) if previous == current => write_change(ComponentChange::NoChange)?,
            (Some(previous), Some(current)) if T::can_delta(previous, current) => write_change(ComponentChange::DeltaChange)?,
            (Some(_), Some(_)) => write_change(ComponentChange::FullChange)?,
        }
    }

    for (i, change) in changes.iter().enumerate() {
        match change {
            ComponentChange::NoComponent | ComponentChange::NoChange => {}
            ComponentChange::FullChange => {
                let component = current_components[i].as_ref().unwrap();
                T::write_full(component, writer)?;
            }
            ComponentChange::DeltaChange => {
                let current = current_components[i].as_ref().unwrap();
                let previous = delta_mapping
                    .get(&entities[i])
                    .and_then(|index| previous_components[*index].as_ref())
                    .unwrap();

                T::write_delta(previous, current, writer)?;
            }
        }
    }

    Ok(())
}

pub fn read_delta_component<T: NetworkedComponent>(
    reader: &mut BitReader,
    entities: &[NetworkID],
    previous_components: &[Option<T::Component>],
    delta_mapping: &HashMap<NetworkID, usize>,
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
                let component = T::read_full(reader)?;
                components.push(Some(component));
            }
            ComponentChange::NoComponent => {
                components.push(None);
            }
            ComponentChange::NoChange => match delta_mapping.get(&entities[i]) {
                None => return Err(io::Error::new(io::ErrorKind::InvalidData, "Component not found in delta mapping")),
                Some(index) => {
                    let component = previous_components[*index].clone();
                    components.push(component);
                }
            },
            ComponentChange::DeltaChange => match delta_mapping.get(&entities[i]) {
                None => return Err(io::Error::new(io::ErrorKind::InvalidData, "Component not found in delta mapping")),
                Some(index) => {
                    let previous_component = match &previous_components[*index] {
                        Some(component) => component,
                        None => return Err(io::Error::new(io::ErrorKind::InvalidData, "Component for delta encoding not found")),
                    };
                    let component = T::read_delta(previous_component, reader)?;
                    components.push(Some(component));
                }
            },
        }
    }

    Ok(components)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::Component;

    use crate::client::SnapshotInterpolationBuffer;

    use super::*;

    #[derive(Debug, Component, PartialEq, Eq, Clone)]
    struct Simple(u32);

    impl NetworkedComponent for Simple {
        type Component = Self;

        fn can_delta(old: &Self::Component, new: &Self::Component) -> bool {
            new.0.abs_diff(old.0) < 32
        }

        fn write_full(component: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error> {
            writer.write_u32(component.0)
        }

        fn read_full(reader: &mut BitReader) -> Result<Self::Component, io::Error> {
            let value = reader.read_u32()?;

            Ok(Self(value))
        }

        fn write_delta(old: &Self::Component, new: &Self::Component, writer: &mut BitWriter) -> Result<(), io::Error> {
            let diff = new.0.abs_diff(old.0);
            writer.write_bool(new.0 > old.0)?;
            writer.write_bits(diff, 5)
        }

        fn read_delta(old: &Self::Component, reader: &mut BitReader) -> Result<Self::Component, io::Error> {
            let sign = reader.read_bool()?;
            let diff = reader.read_bits(5)?;
            let value = if sign { old.0 + diff } else { old.0 - diff };

            Ok(Self(value))
        }
    }

    network_frame!(Simple);

    #[test]
    fn test_full() {
        let frame = NetworkFrame {
            tick: 0,                                    // 8 bits + 1 bit for delta frame bool + 8 bits for len = 17 bits
            entities: vec![NetworkID(0), NetworkID(1)], // 2 * 12 = 24 bits
            // Changes: 2 * 1 = 2
            simple: vec![Some(Simple(10)), None], // 1 full = 32 bits
        };
        // 17 + 24 + 2 + 32 = 75 bits written

        let mut writer = BitWriter::with_capacity(100);
        frame.write_full_frame(&mut writer).unwrap();
        assert_eq!(writer.bits_written(), 75);

        let buffer = writer.consume().unwrap();
        let mut reader = BitReader::new(&buffer).unwrap();

        let mut world = bevy::prelude::World::new();
        let read_frame = NetworkFrame::read_frame(&mut reader, &mut world).unwrap();

        assert_eq!(frame, read_frame);
    }

    #[test]
    fn test_delta() {
        // 0 -> delta
        // 1 -> full change
        // 2 -> Removed entity
        // 3 -> From None -> Some(..)
        // 4 -> From Some -> None
        // 10 -> Created entity with Some(..)
        // 11 -> Created entity with None
        let first_frame = NetworkFrame {
            tick: 0,
            entities: vec![NetworkID(0), NetworkID(1), NetworkID(2), NetworkID(3), NetworkID(4)],
            simple: vec![Some(Simple(10)), Some(Simple(0)), Some(Simple(0)), None, Some(Simple(4))],
        };

        let second_frame = NetworkFrame {
            tick: 0, // 8 bits + 1 bit for delta frame bool + 8 bits for delta tick + 8 bits for len = 25 bits
            entities: vec![NetworkID(0), NetworkID(1), NetworkID(3), NetworkID(4), NetworkID(10), NetworkID(11)], // 12 * 6 = 72 bits
            simple: vec![
                // Changes 2 * 6 = 12 bits
                // Already had entity
                Some(Simple(16)),
                Some(Simple(100)),
                Some(Simple(3)),
                None, // 1 delta + 3 full = 6 bits + 3 * 32 bits = 102 bits
                // New entities
                Some(Simple(50)),
                None,
            ],
        };
        // 25 + 72 + 12 + 102 = 211 bits written

        let mut world = bevy::prelude::World::new();
        let mut buffer = SnapshotInterpolationBuffer::new(5, Duration::ZERO, 60.);
        buffer.add_snapshot(Duration::ZERO, first_frame.clone());
        world.insert_resource(buffer);

        let mut writer = BitWriter::with_capacity(100);
        second_frame.write_delta_frame(&mut writer, &first_frame).unwrap();

        assert_eq!(writer.bits_written(), 211);

        let buffer = writer.consume().unwrap();
        let mut reader = BitReader::new(&buffer).unwrap();

        let read_frame = NetworkFrame::read_frame(&mut reader, &mut world).unwrap();

        assert_eq!(second_frame, read_frame);
    }
}
