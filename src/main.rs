fn main() {

}

/*
use std::collections::HashMap;
use bevy::prelude::*;
use bevy_replicate::{
    deserialize_full_snap, network_entity::NetworkEntities, networked_transform::TransformNetworked, sequence_buffer::SequenceBuffer,
    serialize_full_snap, LastNetworkTick,
};

#[derive(Debug, Component)]
struct Velocity(Vec3);

fn main() {
    let mut world = World::new();

    init_network_component::<TransformNetworked>(&mut world);

    world.insert_resource(NetworkTick(0));
    world.insert_resource(NetworkFrames(SequenceBuffer::with_capacity(100)));
    world.insert_resource(LastNetworkTick(HashMap::new()));

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
        let buffer = serialize_delta_snap(&mut world, 0).unwrap();
        println!("buffer len {}", buffer.len());
        deserialize_delta_snap(buffer, &mut world).unwrap();

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
*/
