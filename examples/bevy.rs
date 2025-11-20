// NOTE: This example requires bevy-aabb-instancing, which doesn't yet support Bevy 0.17
// The example has been updated for Bevy 0.17 API changes but won't compile until
// bevy-aabb-instancing is updated. Track progress at:
// https://github.com/ForesightMiningSoftwareCorporation/bevy_aabb_instancing

use bevy::prelude::*;
// use bevy_aabb_instancing::{
//     Cuboid, CuboidMaterial, CuboidMaterialMap, Cuboids, ScalarHueOptions,
//     VertexPullingRenderPlugin, COLOR_MODE_SCALAR_HUE,
// };
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use vdb_rs::VdbReader;

use std::{error::Error, fs::File, io::BufReader};

// Placeholder types until bevy-aabb-instancing is updated
struct Cuboid;
impl Cuboid {
    fn new(_: bevy::math::Vec3, _: bevy::math::Vec3, _: u32) -> Self { Self }
}
struct CuboidMaterial;
#[allow(non_upper_case_globals)]
const COLOR_MODE_SCALAR_HUE: i32 = 0;
struct ScalarHueOptions {
    min_visible: f32,
    max_visible: f32,
    clamp_min: f32,
    clamp_max: f32,
}
impl Default for ScalarHueOptions {
    fn default() -> Self {
        Self {
            min_visible: 0.0,
            max_visible: 1.0,
            clamp_min: 0.0,
            clamp_max: 1.0,
        }
    }
}
type CuboidMaterialMap = Vec<CuboidMaterial>;
struct Cuboids;
impl Cuboids {
    fn new(_: Vec<Cuboid>) -> Self { Self }
    fn aabb(&self) -> bevy::math::bounding::Aabb3d {
        bevy::math::bounding::Aabb3d {
            min: Vec3::ZERO.into(),
            max: Vec3::ONE.into(),
        }
    }
}
struct VertexPullingRenderPlugin {
    outlines: bool,
}
impl bevy::app::Plugin for VertexPullingRenderPlugin {
    fn build(&self, _app: &mut bevy::app::App) {}
}
type CuboidMaterialId = usize;

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
fn setup(mut commands: Commands) {
    let _color_options_id = 0; // Placeholder until bevy-aabb-instancing is updated

    // Note: This setup code is preserved for when bevy-aabb-instancing is updated
    // let color_options_id = color_options_map.push(CuboidMaterial {
    //     color_mode: COLOR_MODE_SCALAR_HUE,
    //     scalar_hue: ScalarHueOptions {
    //         min_visible: -10000.0,
    //         max_visible: 10000.0,
    //         clamp_min: -1.0,
    //         clamp_max: 0.5,
    //         ..Default::default()
    //     },
    //     ..Default::default()
    // });

    // Note: VDB loading code preserved for when bevy-aabb-instancing is updated
    let _filename = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "example.vdb".to_string());

    // Uncomment when bevy-aabb-instancing is updated:
    // let f = File::open(filename).unwrap();
    // let mut vdb_reader = VdbReader::new(BufReader::new(f)).unwrap();
    // let grid_names = vdb_reader.available_grids();
    //
    // let grid_to_load = std::env::args().nth(2).unwrap_or_else(|| {
    //     println!(
    //         "Grid name not specified, defaulting to first available grid.\nAvailable grids: {:?}",
    //         grid_names
    //     );
    //     grid_names.first().cloned().unwrap_or(String::new())
    // });
    //
    // let grid = vdb_reader.read_grid::<half::f16>(&grid_to_load).unwrap();
    // let instances: Vec<Cuboid> = grid
    //     .iter()
    //     .map(|(pos, voxel, level)| {
    //         Cuboid::new(
    //             pos * 0.1,
    //             (pos + level.scale()) * 0.1,
    //             u32::from_le_bytes(f32::to_le_bytes(voxel.to_f32())),
    //         )
    //     })
    //     .collect();
    // let cuboids = Cuboids::new(instances);
    // let aabb = cuboids.aabb();
    // commands
    //     .spawn(SpatialBundle::default())
    //     .insert((cuboids, aabb, color_options_id));

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
