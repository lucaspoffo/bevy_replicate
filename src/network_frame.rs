use bevy::prelude::*;
use paste::paste;
use crate::networked_transform::TransformNetworked;

pub trait NetworkedFrame {
    fn generate_frame(world: &World) -> Self;
}


macro_rules! network_frame {
    ($($type:ty),+) => {
        paste! {
            
            pub struct NetworkFrame {
                $(
                    $type:snake:lower : $type::<$crate::networked::Networked>::Component,
                )*
            }

        }
    }
}

#[test]
fn test() {
    network_frame!(TransformNetworked);
}
