// NOTE: This example requires bevy-aabb-instancing, which doesn't yet support Bevy 0.17
// The example has been updated for Bevy 0.17 API changes but won't compile until
// bevy-aabb-instancing is updated. Track progress at:
// https://github.com/ForesightMiningSoftwareCorporation/bevy_aabb_instancing

use bevy::prelude::*;
// use bevy_aabb_instancing::{
//     Cuboid, CuboidMaterial, CuboidMaterialId, CuboidMaterialMap, Cuboids, ScalarHueOptions,
//     VertexPullingRenderPlugin, COLOR_MODE_SCALAR_HUE,
// };
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use bevy_panorbit_camera::{PanOrbitCamera, PanOrbitCameraPlugin};
use half::f16;
use vdb_rs::{Grid, Map, VdbLevel, VdbReader};

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

#[derive(Debug, PartialEq, Copy, Clone)]
enum SliceAxis {
    X = 0,
    Y,
    Z,
}

impl From<SliceAxis> for Vec3 {
    fn from(value: SliceAxis) -> Self {
        match value {
            SliceAxis::X => Vec3::X,
            SliceAxis::Y => Vec3::Y,
            SliceAxis::Z => Vec3::Z,
        }
    }
}

#[derive(Debug, PartialEq)]
enum RenderMode {
    FirstDensity,
    Tiles,
    Slice(SliceAxis),
}

#[derive(Resource)]
struct RenderSettings {
    render_mode: RenderMode,
    render_slice_index: i32,
    min_slice_indices: IVec3,
    max_slice_indices: IVec3,
    dirty: bool,
    visible_voxels: u64,
}

#[derive(Resource)]
struct ModelData {
    color_options_id: CuboidMaterialId,
    grid: Grid<f16>,
}

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
        .add_plugins(EguiPlugin::default())
        .add_systems(Update, settings_ui)
        .add_systems(Update, rebuild_model)
        .run();

    Ok(())
}

fn settings_ui(mut contexts: EguiContexts, mut settings: ResMut<RenderSettings>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Window::new("Settings").show(ctx, |ui| {
        egui::ComboBox::from_label("Render")
            .selected_text(format!("{:?}", settings.render_mode))
            .show_ui(ui, |ui| {
                settings.dirty |= ui
                    .selectable_value(
                        &mut settings.render_mode,
                        RenderMode::FirstDensity,
                        "Density",
                    )
                    .changed();
                settings.dirty |= ui
                    .selectable_value(&mut settings.render_mode, RenderMode::Tiles, "Tiles")
                    .changed();
                settings.dirty |= ui
                    .selectable_value(
                        &mut settings.render_mode,
                        RenderMode::Slice(SliceAxis::X),
                        "X slice",
                    )
                    .changed();
                settings.dirty |= ui
                    .selectable_value(
                        &mut settings.render_mode,
                        RenderMode::Slice(SliceAxis::Y),
                        "Y slice",
                    )
                    .changed();
                settings.dirty |= ui
                    .selectable_value(
                        &mut settings.render_mode,
                        RenderMode::Slice(SliceAxis::Z),
                        "Z slice",
                    )
                    .changed();
            });
        if let RenderMode::Slice(i) = settings.render_mode {
            let range =
                settings.min_slice_indices[i as usize]..=settings.max_slice_indices[i as usize];
            settings.dirty |= ui
                .add(egui::Slider::new(&mut settings.render_slice_index, range))
                .changed()
        }
        ui.label(format!("visible voxels: {}", settings.visible_voxels));
    });
}

// Marker component for voxel entities
#[derive(Component)]
struct VoxelMarker;

fn rebuild_model(
    mut commands: Commands,
    mut settings: ResMut<RenderSettings>,
    model_data: Res<ModelData>,
    existing_voxels: Query<Entity, With<VoxelMarker>>,
) {
    if settings.dirty {
        for entity in existing_voxels.iter() {
            commands.entity(entity).despawn();
        }

        // Convert from glam 0.24 (used by vdb_rs) to glam 0.30 (used by Bevy 0.17)
        let translation = match &model_data.grid.transform {
            Map::ScaleTranslateMap { translation, .. } => {
                let v = translation.as_vec3();
                Vec3::new(v.x, v.y, v.z)
            },
            _ => Vec3::ZERO,
        };

        let slice_index = settings.render_slice_index;

        let reject_fn: Box<dyn Fn(Vec3, VdbLevel) -> bool> = match settings.render_mode {
            RenderMode::FirstDensity => Box::new(|_, _| false),
            RenderMode::Slice(i) => Box::new(move |pos, _| pos[i as usize] as i32 != slice_index),
            RenderMode::Tiles => Box::new(|_, level| level == VdbLevel::Voxel),
        };

        let instances: Vec<Cuboid> = model_data
            .grid
            .iter()
            .filter_map(|(pos, voxel, level)| {
                // Convert glam 0.24 Vec3 to glam 0.30 Vec3
                let pos_bevy = Vec3::new(pos.x, pos.y, pos.z);

                // If our voxel intersects the slice index, we have to move it there to properly evaluate the reject_fn
                let brick_starting_pos = ((pos_bevy / level.scale()).floor()
                    + if slice_index.is_negative() { 1.0 } else { 0.0 })
                    * level.scale();
                let slice_local_offset = (slice_index % level.scale() as i32) as f32;
                let level_invariate_position = brick_starting_pos + slice_local_offset;
                if reject_fn(level_invariate_position, level) {
                    None
                } else {
                    let mut dimension_mult = Vec3::ONE;
                    let mut final_pos = pos_bevy;
                    if let RenderMode::Slice(i) = settings.render_mode {
                        dimension_mult -= Vec3::from(i);
                        final_pos[i as usize] = slice_index as f32;
                    }
                    dimension_mult = (dimension_mult * level.scale()).max(Vec3::ONE);

                    let final_pos = final_pos + translation;
                    Some(Cuboid::new(
                        final_pos * 0.1,
                        (final_pos + dimension_mult) * 0.1,
                        u32::from_le_bytes(f32::to_le_bytes(voxel.to_f32())),
                    ))
                }
            })
            .collect();

        settings.visible_voxels = instances.len() as u64;
        let cuboids = Cuboids::new(instances);

        let _aabb = cuboids.aabb();
        commands.spawn((
            Transform::default(),
            Visibility::default(),
            VoxelMarker,
            // cuboids, aabb, and material will be added when bevy-aabb-instancing is updated
        ));
        settings.dirty = false;
    }
}

/// set up a simple 3D scene
fn setup(mut commands: Commands) {
    // Note: Example disabled until bevy-aabb-instancing supports Bevy 0.17
    // Uncomment when dependency is updated:

    // let grid = load_grid();
    //
    // commands.insert_resource(RenderSettings {
    //     render_mode: RenderMode::FirstDensity,
    //     render_slice_index: 0,
    //     min_slice_indices: grid.descriptor.aabb_min().unwrap(),
    //     max_slice_indices: grid.descriptor.aabb_max().unwrap(),
    //     dirty: true,
    //     visible_voxels: 0,
    // });
    //
    // commands.insert_resource(ModelData {
    //     color_options_id: 0,
    //     grid,
    // });

    // Note: This example requires a VDB file to be provided as an argument
    // and bevy-aabb-instancing to be updated for Bevy 0.17
    let grid = if std::env::args().nth(1).is_some() {
        load_grid()
    } else {
        eprintln!("Warning: No VDB file provided. This example requires a .vdb file as the first argument.");
        eprintln!("Usage: cargo run --example slicer -- path/to/file.vdb [grid_name]");
        eprintln!("Note: This example also requires bevy-aabb-instancing to support Bevy 0.17");
        // Create an empty grid to allow compilation
        Grid {
            descriptor: vdb_rs::GridDescriptor {
                name: String::from("empty"),
                file_version: 0,
                instance_parent: String::new(),
                grid_type: String::from("unknown"),
                grid_pos: 0,
                block_pos: 0,
                end_pos: 0,
                compression: vdb_rs::Compression::NONE,
                meta_data: vdb_rs::Metadata::default(),
            },
            transform: Map::UniformScaleMap {
                scale_values: glam::DVec3::ONE,
                voxel_size: glam::DVec3::ONE,
                scale_values_inverse: glam::DVec3::ONE,
                inv_scale_sqr: glam::DVec3::ONE,
                inv_twice_scale: glam::DVec3::ONE,
            },
            tree: vdb_rs::Tree { root_nodes: Vec::new() },
        }
    };

    // Convert glam 0.24 IVec3 to glam 0.30 IVec3 (for Bevy 0.17)
    let min_indices_old = grid.descriptor.aabb_min().unwrap_or(glam::IVec3::ZERO);
    let max_indices_old = grid.descriptor.aabb_max().unwrap_or(glam::IVec3::ONE);
    let min_indices = IVec3::new(min_indices_old.x, min_indices_old.y, min_indices_old.z);
    let max_indices = IVec3::new(max_indices_old.x, max_indices_old.y, max_indices_old.z);

    commands.insert_resource(RenderSettings {
        render_mode: RenderMode::FirstDensity,
        render_slice_index: 0,
        min_slice_indices: min_indices,
        max_slice_indices: max_indices,
        dirty: false,
        visible_voxels: 0,
    });

    commands.insert_resource(ModelData {
        color_options_id: 0,
        grid,
    });
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

fn load_grid() -> Grid<f16> {
    let filename = std::env::args()
        .nth(1)
        .expect("Missing VDB filename as first argument");

    let f = File::open(filename.clone()).unwrap();
    let mut vdb_reader = VdbReader::new(BufReader::new(f)).unwrap();
    let grid_names = vdb_reader.available_grids();

    let grid_to_load = std::env::args().nth(2).unwrap_or_else(|| {
        println!(
            "Grid name not specified, defaulting to first available grid.\nAvailable grids: {:?}",
            grid_names
        );
        grid_names.first().cloned().unwrap_or(String::new())
    });

    vdb_reader.read_grid::<half::f16>(&grid_to_load).unwrap()
}
