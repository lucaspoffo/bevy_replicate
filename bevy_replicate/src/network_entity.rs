use std::collections::HashMap;

use bevy::prelude::*;

const ID_BITS: usize = 12;
const MAX_ID: u16 = (1 << ID_BITS) - 1;
pub(crate) const MAX_LENGTH: usize = 1 << ID_BITS;

#[derive(Debug, Component, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NetworkID(pub u16);

#[derive(Debug)]
pub struct NetworkEntities {
    used: Box<[bool; MAX_LENGTH]>,
    entity_map: HashMap<Entity, NetworkID>,
    current_id: usize,
}

impl Default for NetworkEntities {
    fn default() -> Self {
        Self {
            used: Box::new([false; MAX_LENGTH]),
            entity_map: HashMap::new(),
            current_id: 0,
        }
    }
}

impl NetworkEntities {
    pub fn generate(&mut self) -> Option<NetworkID> {
        let mut count = 0;
        loop {
            if !self.used[self.current_id] {
                let network_id = NetworkID(self.current_id as u16);
                self.used[self.current_id] = true;
                self.current_id += 1;
                return Some(network_id);
            }

            if self.current_id as u16 > MAX_ID {
                self.current_id = 0;
            }

            count += 1;
            if count >= MAX_LENGTH {
                return None;
            }
        }
    }

    pub fn remove(&mut self, entity: Entity) {
        if let Some(network_id) = self.entity_map.remove(&entity) {
            let index = network_id.0 as usize;
            self.used[index] = false;
        }
    }
}

pub fn track_network_entity_system(mut network_entities: ResMut<NetworkEntities>, query: Query<(Entity, &NetworkID), Added<NetworkID>>) {
    for (entity, network_id) in query.iter() {
        network_entities.entity_map.insert(entity, *network_id);
    }
}

pub fn cleanup_network_entity_system(mut network_entities: ResMut<NetworkEntities>, removals: RemovedComponents<NetworkID>) {
    for entity in removals.iter() {
        network_entities.remove(entity);
    }
}
