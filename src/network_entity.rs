use bevy::prelude::*;

const ID_BITS: usize = 12;
const MAX_ID: u16 = 1 << ID_BITS - 1;
pub const MAX_LENGTH: usize = 1 << ID_BITS;

#[derive(Debug, Component, Copy, Clone, PartialEq, Eq, Hash)]
pub struct NetworkID(pub(crate) u16);

#[derive(Debug)]
pub struct NetworkEntities {
    used: Box<[bool; MAX_LENGTH]>,
    current_id: usize,
}

impl Default for NetworkEntities {
    fn default() -> Self {
        Self {
            used: Box::new([false; MAX_LENGTH]),
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

    pub fn remove(&mut self, network_id: NetworkID) {
        assert!(network_id.0 <= MAX_ID);
        let index = network_id.0 as usize;
        self.used[index] = false;
    }
}
