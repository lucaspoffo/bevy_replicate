use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPlugin};
use bevy_renet::{
    renet::{
        ClientAuthentication, RenetClient, RenetConnectionConfig, RenetError, RenetServer, ServerAuthentication, ServerConfig, ServerEvent,
    },
    run_if_client_connected, RenetClientPlugin, RenetServerPlugin,
};
use bevy_replicate::{
    network_entity::NetworkEntities, network_frame, networked_transform::TransformNetworked, process_snap, NetworkFrameBuffer, NetworkTick,
    Networked, NetworkedFrame, ReplicateClientPlugin, ReplicateServerPlugin,
};
use bit_serializer::BitWriter;
use renet_visualizer::RenetClientVisualizer;

use std::time::SystemTime;
use std::{collections::HashMap, net::UdpSocket};

use serde::{Deserialize, Serialize};

const PROTOCOL_ID: u64 = 7;

const PLAYER_MOVE_SPEED: f32 = 1.0;

network_frame!(TransformNetworked, PlayerMarker);

#[derive(Debug, Default, Serialize, Deserialize, Component)]
struct PlayerInput {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

#[derive(Debug, Component)]
struct Player {
    id: u64,
}

#[derive(Debug, Component, PartialEq, Eq, Clone)]
struct PlayerMarker;

impl Networked for PlayerMarker {
    type Component = Self;

    fn write_full(_component: &Self::Component, _writer: &mut bit_serializer::BitWriter) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn read_full(_reader: &mut bit_serializer::BitReader) -> Result<Self::Component, std::io::Error> {
        Ok(Self)
    }
}

#[derive(Debug, Default)]
struct Lobby {
    players: HashMap<u64, Entity>,
}

#[derive(Debug, Serialize, Deserialize, Component)]
enum ServerMessages {
    PlayerConnected { id: u64 },
    PlayerDisconnected { id: u64 },
}

fn new_renet_client() -> RenetClient {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let connection_config = RenetConnectionConfig::default();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Unsecure {
        client_id,
        protocol_id: PROTOCOL_ID,
        server_addr,
        user_data: None,
    };
    RenetClient::new(current_time, socket, connection_config, authentication).unwrap()
}

fn new_renet_server() -> RenetServer {
    let server_addr = "127.0.0.1:5000".parse().unwrap();
    let socket = UdpSocket::bind(server_addr).unwrap();
    let connection_config = RenetConnectionConfig::default();
    let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Unsecure);
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    RenetServer::new(current_time, server_config, connection_config, socket).unwrap()
}

fn main() {
    println!("Usage: run with \"server\" or \"client\" argument");
    let args: Vec<String> = std::env::args().collect();

    let exec_type = &args[1];
    let is_host = match exec_type.as_str() {
        "client" => false,
        "server" => true,
        _ => panic!("Invalid argument, must be \"client\" or \"server\"."),
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugin(EguiPlugin);

    app.insert_resource(Lobby::default());

    if is_host {
        app.add_plugin(RenetServerPlugin);
        app.add_plugin(ReplicateServerPlugin::<NetworkFrame>::default());
        app.insert_resource(new_renet_server());
        app.add_system(server_update_system);
        app.add_system(move_players_system);
        app.add_system_to_stage(CoreStage::PostUpdate, server_sync_players.exclusive_system().at_start());
    } else {
        app.add_plugin(RenetClientPlugin);
        app.insert_resource(new_renet_client());
        app.insert_resource(PlayerInput::default());
        app.add_system(player_input);
        app.add_system(spawn_client_bundle);
        app.add_system(client_send_input.with_run_criteria(run_if_client_connected));
        app.add_system(client_sync_players.with_run_criteria(run_if_client_connected));

        app.insert_resource(RenetClientVisualizer::<200>::default());
        app.add_system(update_client_visulizer_system);

        app.add_plugin(ReplicateClientPlugin::<NetworkFrame>::default());
        app.add_system_to_stage(CoreStage::PreUpdate, read_network_frame.exclusive_system().at_end());
    }

    app.add_startup_system(setup);
    app.add_system(panic_on_error_system);

    app.run();
}

fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lobby: ResMut<Lobby>,
    mut server: ResMut<RenetServer>,
    mut network_entities: ResMut<NetworkEntities>,
) {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected(id, _) => {
                println!("Player {} connected.", id);
                // Spawn player cube
                let player_entity = commands
                    .spawn_bundle(PbrBundle {
                        mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
                        material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
                        transform: Transform::from_xyz(0.0, 0.5, 0.0),
                        ..Default::default()
                    })
                    .insert(PlayerInput::default())
                    .insert(Player { id: *id })
                    .insert(PlayerMarker)
                    .insert(network_entities.generate().unwrap())
                    .id();

                lobby.players.insert(*id, player_entity);
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Player {} disconnected.", id);
                if let Some(player_entity) = lobby.players.remove(id) {
                    commands.entity(player_entity).despawn();
                }
            }
        }
    }

    for client_id in server.clients_id().into_iter() {
        while let Some(message) = server.receive_message(client_id, 0) {
            let player_input: PlayerInput = bincode::deserialize(&message).unwrap();
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(player_input);
            }
        }
    }
}

fn server_sync_players(
    mut server: ResMut<RenetServer>,
    network_tick: Res<NetworkTick>,
    network_buffer: Res<NetworkFrameBuffer<NetworkFrame>>,
) {
    let frame = network_buffer.0.get(network_tick.0).unwrap();
    let mut writer = BitWriter::with_capacity(1000);
    frame.write_full_frame(&mut writer).unwrap();

    server.broadcast_message(1, writer.consume().unwrap());
}

fn read_network_frame(world: &mut World) {
    world.resource_scope(|world, mut client: Mut<RenetClient>| {
        while let Some(message) = client.receive_message(1) {
            process_snap::<NetworkFrame>(message, world).unwrap();
        }
    });
}

fn client_sync_players(mut client: ResMut<RenetClient>) {
    while let Some(message) = client.receive_message(0) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerConnected { id } => {
                println!("Player {} connected.", id);
            }
            ServerMessages::PlayerDisconnected { id } => {
                println!("Player {} disconnected.", id);
            }
        }
    }
}

/// set up a simple 3D scene
fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
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

fn player_input(keyboard_input: Res<Input<KeyCode>>, mut player_input: ResMut<PlayerInput>) {
    player_input.left = keyboard_input.pressed(KeyCode::A) || keyboard_input.pressed(KeyCode::Left);
    player_input.right = keyboard_input.pressed(KeyCode::D) || keyboard_input.pressed(KeyCode::Right);
    player_input.up = keyboard_input.pressed(KeyCode::W) || keyboard_input.pressed(KeyCode::Up);
    player_input.down = keyboard_input.pressed(KeyCode::S) || keyboard_input.pressed(KeyCode::Down);
}

fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>) {
    let input_message = bincode::serialize(&*player_input).unwrap();

    client.send_message(0, input_message);
}

fn move_players_system(mut query: Query<(&mut Transform, &PlayerInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let x = (input.right as i8 - input.left as i8) as f32;
        let y = (input.down as i8 - input.up as i8) as f32;
        transform.translation.x += x * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();
        transform.translation.z += y * PLAYER_MOVE_SPEED * time.delta().as_secs_f32();
    }
}

// If any error is found we just panic
fn panic_on_error_system(mut renet_error: EventReader<RenetError>) {
    for e in renet_error.iter() {
        panic!("{}", e);
    }
}

fn spawn_client_bundle(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    new_players: Query<Entity, Added<PlayerMarker>>,
) {
    for entity in new_players.iter() {
        commands.entity(entity).insert_bundle(PbrBundle {
            mesh: meshes.add(Mesh::from(shape::Cube { size: 1.0 })),
            material: materials.add(Color::rgb(0.8, 0.7, 0.6).into()),
            transform: Transform::from_xyz(0.0, 0.5, 0.0),
            ..Default::default()
        });
    }
}

fn update_client_visulizer_system(
    mut egui_context: ResMut<EguiContext>,
    mut visualizer: ResMut<RenetClientVisualizer<200>>,
    client: Res<RenetClient>,
    mut show_visualizer: Local<bool>,
    keyboard_input: Res<Input<KeyCode>>,
) {
    visualizer.add_network_info(client.network_info());
    if keyboard_input.just_pressed(KeyCode::F1) {
        *show_visualizer = !*show_visualizer;
    }
    if *show_visualizer {
        visualizer.show_window(egui_context.ctx_mut());
    }
}
