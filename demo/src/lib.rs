use bevy::prelude::*;
use bevy_renet::renet::RenetError;
use bevy_replicate::{network_frame, networked_transform::TransformNetworked, NetworkedComponent};
use bit_serializer::{BitReader, BitWriter};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_ID: u64 = 7;

network_frame!(TransformNetworked, Player);

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Component)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Component, PartialEq, Eq, Clone)]
pub struct Player(pub u64);

impl NetworkedComponent for Player {
    type Component = Self;

    fn write_full(component: &Self::Component, writer: &mut BitWriter) -> Result<(), std::io::Error> {
        writer.write_u64(component.0)
    }

    fn read_full(reader: &mut BitReader) -> Result<Self::Component, std::io::Error> {
        let id = reader.read_u64()?;
        Ok(Self(id))
    }
}

/// set up a simple 3D scene
pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // plane
    commands.spawn_bundle(PbrBundle {
        mesh: meshes.add(Mesh::from(shape::Plane { size: 5.0 })),
        material: materials.add(Color::rgb(0.3, 0.5, 0.3).into()),
        ..Default::default()
    });
    // light
    commands.spawn_bundle(PointLightBundle {
        point_light: PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..Default::default()
        },
        transform: Transform::from_xyz(4.0, 8.0, 4.0),
        ..Default::default()
    });
    // camera
    commands.spawn_bundle(Camera3dBundle {
        transform: Transform::from_xyz(-2.0, 2.5, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..Default::default()
    });
}

// If any error is found we just panic
pub fn panic_on_error_system(mut renet_error: EventReader<RenetError>) {
    for e in renet_error.iter() {
        panic!("{}", e);
    }
}
