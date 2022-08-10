use bevy::prelude::*;
use bevy_egui::{EguiContext, EguiPlugin};
use bevy_renet::{
    renet::{ClientAuthentication, DefaultChannel, RenetClient, RenetConnectionConfig},
    run_if_client_connected, RenetClientPlugin,
};
use bevy_replicate::{process_snap, LastReceivedNetworkTick, ReplicateClientPlugin};
use demo::{panic_on_error_system, setup, NetworkFrame, Player, PlayerInput, PROTOCOL_ID};
use renet_visualizer::RenetClientVisualizer;

use std::net::UdpSocket;
use std::time::SystemTime;

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

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.add_plugin(EguiPlugin);

    app.add_plugin(RenetClientPlugin);
    app.insert_resource(new_renet_client());
    app.insert_resource(PlayerInput::default());
    app.add_system(player_input);
    app.add_system(spawn_client_bundle);
    app.add_system(client_send_input.with_run_criteria(run_if_client_connected));
    app.add_system(client_send_last_received_tick.with_run_criteria(run_if_client_connected));

    app.insert_resource(RenetClientVisualizer::<200>::default());
    app.add_system(update_client_visulizer_system);

    app.add_plugin(ReplicateClientPlugin::<NetworkFrame>::default());
    app.add_system_to_stage(CoreStage::PreUpdate, read_network_frame.exclusive_system().at_end());

    app.add_startup_system(setup);
    app.add_system(panic_on_error_system);

    app.run();
}

fn read_network_frame(world: &mut World) {
    world.resource_scope(|world, mut client: Mut<RenetClient>| {
        while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
            process_snap::<NetworkFrame>(message, world).unwrap();
        }
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

    client.send_message(DefaultChannel::Reliable, input_message);
}

fn client_send_last_received_tick(mut client: ResMut<RenetClient>, last_received_tick: Res<LastReceivedNetworkTick>) {
    if let Some(tick) = last_received_tick.0 {
        client.send_message(DefaultChannel::Unreliable, tick.to_le_bytes().to_vec());
    }
}

fn spawn_client_bundle(
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    new_players: Query<Entity, Added<Player>>,
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
