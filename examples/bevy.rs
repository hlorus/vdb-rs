// NOTE: Using bevy-aabb-instancing from upstream PR for Bevy 0.17 support

use bevy::prelude::*;
use bevy_aabb_instancing::{
    Cuboid, CuboidMaterial, CuboidMaterialMap, Cuboids, ScalarHueOptions,
    VertexPullingRenderPlugin, COLOR_MODE_SCALAR_HUE,
};
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use vdb_rs::VdbReader;

use std::{error::Error, fs::File, io::BufReader};

fn main() -> Result<(), Box<dyn Error>> {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VDB Viewer".into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins(VertexPullingRenderPlugin { outlines: true })
        .add_plugins(PanOrbitCameraPlugin)
        .add_systems(Startup, setup)
        .run();

    Ok(())
}

/// set up a simple 3D scene
fn setup(mut commands: Commands, mut color_options_map: ResMut<CuboidMaterialMap>) {
    let color_options_id = color_options_map.push(CuboidMaterial {
        color_mode: COLOR_MODE_SCALAR_HUE,
        scalar_hue: ScalarHueOptions {
            min_visible: -10000.0,
            max_visible: 10000.0,
            clamp_min: -1.0,
            clamp_max: 0.5,
            ..Default::default()
        },
        ..Default::default()
    });

    let filename = std::env::args()
        .nth(1)
        .expect("Missing VDB filename as first argument");

    let f = File::open(filename).unwrap();
    let mut vdb_reader = VdbReader::new(BufReader::new(f)).unwrap();
    let grid_names = vdb_reader.available_grids();

    let grid_to_load = std::env::args().nth(2).unwrap_or_else(|| {
        println!(
            "Grid name not specified, defaulting to first available grid.\nAvailable grids: {:?}",
            grid_names
        );
        grid_names.first().cloned().unwrap_or(String::new())
    });

    let grid = vdb_reader.read_grid::<half::f16>(&grid_to_load).unwrap();
    let instances: Vec<Cuboid> = grid
        .iter()
        .map(|(pos, voxel, level)| {
            // Convert from glam 0.24 (vdb_rs) to glam 0.30 (Bevy 0.17)
            let pos_bevy = Vec3::new(pos.x, pos.y, pos.z);
            Cuboid::new(
                pos_bevy * 0.1,
                (pos_bevy + level.scale()) * 0.1,
                u32::from_le_bytes(f32::to_le_bytes(voxel.to_f32())),
            )
        })
        .collect();
    let cuboids = Cuboids::new(instances);
    let aabb = cuboids.aabb();
    commands
        .spawn((Transform::default(), Visibility::default()))
        .insert((cuboids, aabb, color_options_id));

    commands.spawn((
        PointLight {
            intensity: 1500.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        PanOrbitCamera::default(),
    ));
}
