use bevy_replicate::{generate_frame, network_frame, process_snap, replicate, LastNetworkTick, NetworkTick};

use bevy::prelude::*;
use bevy_replicate::{network_entity::NetworkEntities, networked_transform::TransformNetworked, ReplicateServerPlugin};

network_frame!(TransformNetworked);

#[derive(Debug, Component)]
struct Velocity(Vec3);

fn main() {
    let mut world = World::new();

    ReplicateServerPlugin::<NetworkFrame>::init_resources(&mut world);
    let mut network_entities = NetworkEntities::default();

    for i in 0..10 {
        world
            .spawn()
            .insert(Transform::default())
            .insert(Velocity(Vec3::new(i as f32, 0.0, i as f32)))
            .insert(network_entities.generate().unwrap());
    }

    let mut update_position = IntoSystem::into_system(update_position_system);
    update_position.initialize(&mut world);
    let mut network_tick = IntoSystem::into_system(network_tick);
    network_tick.initialize(&mut world);

    for _ in 0..10 {
        update_position.run((), &mut world);
        generate_frame::<NetworkFrame>(&mut world);
        let buffer = replicate::<NetworkFrame>(0, &mut world).unwrap();
        println!("buffer len {}", buffer.len());
        let frame = process_snap::<NetworkFrame>(buffer, &mut world).unwrap();
        println!("\n\nProcessed:\n\n {:?}", frame);

        network_tick.run((), &mut world);
    }
}

fn update_position_system(mut query: Query<(&mut Transform, &Velocity)>) {
    for (mut transform, velocity) in query.iter_mut() {
        transform.translation += velocity.0;
    }
}

fn network_tick(mut network_tick: ResMut<NetworkTick>, mut last_received_tick: ResMut<LastNetworkTick>) {
    network_tick.0 += 1;
    if let Some(tick) = last_received_tick.0.get_mut(&0) {
        *tick += 1;
    } else {
        last_received_tick.0.insert(0, 0);
    }
}
