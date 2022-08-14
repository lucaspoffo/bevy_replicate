use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use bevy_renet::{
    renet::{DefaultChannel, RenetConnectionConfig, RenetServer, ServerAuthentication, ServerConfig, ServerEvent},
    RenetServerPlugin,
};
use bevy_replicate::{
    server::{replicate, LastNetworkTick, NetworkFrameBuffer, NetworkTick, ReplicateServerPlugin},
    NetworkEntities,
};

use demo::{panic_on_error_system, setup, NetworkFrame, Player, PlayerInput, PROTOCOL_ID};

use std::net::UdpSocket;
use std::time::SystemTime;

const PLAYER_MOVE_SPEED: f32 = 1.0;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugin(EguiPlugin);

    app.add_plugin(RenetServerPlugin);
    app.add_plugin(ReplicateServerPlugin::<NetworkFrame>::default());
    app.insert_resource(new_renet_server());
    app.add_system(server_update_system);
    app.add_system(move_players_system);
    app.add_system_to_stage(CoreStage::PostUpdate, server_sync_players.exclusive_system().at_start());

    app.add_startup_system(setup);
    app.add_system(panic_on_error_system);

    app.run();
}

fn new_renet_server() -> RenetServer {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(server_addr).unwrap();
    let connection_config = RenetConnectionConfig::default();
    let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Unsecure);
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    RenetServer::new(current_time, server_config, connection_config, socket).unwrap()
}

fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut server: ResMut<RenetServer>,
    mut network_entities: ResMut<NetworkEntities>,
    mut last_received_tick: ResMut<LastNetworkTick>,
    mut player_query: Query<(Entity, &Player, &mut PlayerInput)>,
) {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected(id, _) => {
                println!("Player {} connected.", id);
                // Spawn player cube
                commands
                    .spawn_bundle(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform: Transform::from_xyz(0.0, 0.5, 0.0),
                        ..Default::default()
                    })
                    .insert(PlayerInput::default())
                    .insert(Player(*id))
                    .insert(network_entities.generate().unwrap());
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Player {} disconnected.", id);
                last_received_tick.0.remove(id);
                for (entity, player, _) in player_query.iter() {
                    if player.0 == *id {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }
    }

    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::Reliable) {
            let player_input: PlayerInput = bincode::deserialize(&message).unwrap();
            for (_, player, mut input) in player_query.iter_mut() {
                if player.0 == client_id {
                    *input = player_input;
                }
            }
        }
    }
}

fn server_sync_players(
    mut server: ResMut<RenetServer>,
    network_tick: Res<NetworkTick>,
    network_buffer: Res<NetworkFrameBuffer<NetworkFrame>>,
    mut last_received_tick: ResMut<LastNetworkTick>,
) {
    // Update last received tick
    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::Unreliable) {
            let tick = u64::from_le_bytes(message.try_into().unwrap());
            match last_received_tick.0.get_mut(&client_id) {
                None => {
                    last_received_tick.0.insert(client_id, tick);
                }
                Some(last_tick) => {
                    if *last_tick < tick {
                        *last_tick = tick;
                    }
                }
            }
        }
    }

    for client_id in server.clients_id().into_iter() {
        let message = replicate::<NetworkFrame>(client_id, &network_tick, &last_received_tick, &network_buffer).unwrap();
        server.send_message(client_id, DefaultChannel::Unreliable, message);
    }
}

fn move_players_system(mut query: Query<(&mut Transform, &PlayerInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        transform.translation.x += x * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();
        transform.translation.z += y * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();

        transform.rotate_x(std::f32::consts::PI / 4. * time.delta_seconds());
    }
}
