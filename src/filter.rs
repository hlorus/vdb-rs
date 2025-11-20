// Filter operations for VDB grids
// Ported from OpenVDB tools/Filter.h
//
//! This module provides filtering operations for VDB grids, including:
//! - Mean (box) filter
//! - Gaussian filter (approximation via iterated mean filtering)
//! - Median filter
//! - Offset operation
//!
//! # Example
//! ```no_run
//! use vdb_rs::{Filter, FilterConfig, VdbReader};
//! use std::{fs::File, io::BufReader};
//!
//! let f = File::open("example.vdb").unwrap();
//! let mut reader = VdbReader::new(BufReader::new(f)).unwrap();
//! let grid = reader.read_grid::<f32>("density").unwrap();
//!
//! // Apply a mean filter
//! let filter = Filter::new(&grid);
//! let smoothed = filter.mean(FilterConfig {
//!     width: 2,
//!     iterations: 1,
//! });
//!
//! // Apply a Gaussian filter
//! let gaussian = filter.gaussian(FilterConfig {
//!     width: 1,
//!     iterations: 1,
//! });
//! ```

use crate::{Grid, Node, Node3};
use glam::IVec3;
use std::ops::Add;

/// Trait for types that can be filtered
pub trait Filterable: Copy + Add<Output = Self> + Default {
    fn to_f32(&self) -> f32;
    fn from_f32(v: f32) -> Self;
}

impl Filterable for f32 {
    fn to_f32(&self) -> f32 {
        *self
    }
    fn from_f32(v: f32) -> Self {
        v
    }
}

impl Filterable for half::f16 {
    fn to_f32(&self) -> f32 {
        half::f16::to_f32(*self)
    }
    fn from_f32(v: f32) -> Self {
        half::f16::from_f32(v)
    }
}

/// Filter configuration
pub struct FilterConfig {
    /// Width of the filter (filter size is 2*width+1)
    pub width: i32,
    /// Number of iterations to apply
    pub iterations: usize,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            width: 1,
            iterations: 1,
        }
    }
}

/// Filter operations on VDB grids
pub struct Filter<'a, ValueTy> {
    grid: &'a Grid<ValueTy>,
}

impl<'a, ValueTy: Filterable> Filter<'a, ValueTy> {
    /// Create a new filter for the given grid
    pub fn new(grid: &'a Grid<ValueTy>) -> Self {
        Self { grid }
    }

    /// Apply a mean (box) filter to the grid
    ///
    /// The mean filter performs separable filtering along each axis.
    /// Filter width is 2*width+1 voxels.
    pub fn mean(&self, config: FilterConfig) -> Grid<ValueTy> {
        let mut result = self.grid.clone();

        for _ in 0..config.iterations {
            // Apply separable mean filter: X, then Z, then Y (OpenVDB order)
            result = self.mean_pass_x(&result, config.width);
            result = self.mean_pass_z(&result, config.width);
            result = self.mean_pass_y(&result, config.width);
        }

        result
    }

    /// Apply Gaussian filter approximation
    ///
    /// Approximates a Gaussian by running 4 iterations of mean filtering,
    /// achieving better than 95% accuracy.
    pub fn gaussian(&self, config: FilterConfig) -> Grid<ValueTy> {
        let gaussian_config = FilterConfig {
            width: config.width,
            iterations: config.iterations * 4, // 4 mean iterations per Gaussian iteration
        };
        self.mean(gaussian_config)
    }

    /// Apply median filter
    ///
    /// Non-separable filter that replaces each voxel with the median
    /// of its neighborhood.
    pub fn median(&self, config: FilterConfig) -> Grid<ValueTy> {
        let mut result = self.grid.clone();

        for _ in 0..config.iterations {
            result = self.median_pass(&result, config.width);
        }

        result
    }

    /// Add a constant offset to all active voxels
    pub fn offset(&self, offset: ValueTy) -> Grid<ValueTy> {
        let mut result = self.grid.clone();

        // Apply offset to all active voxels in leaf nodes
        for root_node in &mut result.tree.root_nodes {
            for node4 in root_node.nodes.values_mut() {
                for node3 in node4.nodes.values_mut() {
                    for (idx, active) in node3.value_mask.iter().enumerate() {
                        if *active && idx < node3.buffer.len() {
                            node3.buffer[idx] = node3.buffer[idx] + offset;
                        }
                    }
                }
            }
        }

        result
    }

    // Private helper methods

    fn mean_pass_x(&self, grid: &Grid<ValueTy>, width: i32) -> Grid<ValueTy> {
        self.apply_separable_filter(grid, width, 0)
    }

    fn mean_pass_y(&self, grid: &Grid<ValueTy>, width: i32) -> Grid<ValueTy> {
        self.apply_separable_filter(grid, width, 1)
    }

    fn mean_pass_z(&self, grid: &Grid<ValueTy>, width: i32) -> Grid<ValueTy> {
        self.apply_separable_filter(grid, width, 2)
    }

    fn apply_separable_filter(&self, grid: &Grid<ValueTy>, width: i32, axis: usize) -> Grid<ValueTy> {
        let mut result = grid.clone();
        let kernel_size = 2 * width + 1;
        let weight = 1.0 / kernel_size as f32;

        // Process each leaf node
        for root_node in &mut result.tree.root_nodes {
            for node4 in root_node.nodes.values_mut() {
                for (_node_idx, node3) in node4.nodes.iter_mut() {
                    let filtered_buffer = self.filter_leaf_node_axis(
                        &grid,
                        node3,
                        width,
                        axis,
                        weight,
                    );
                    node3.buffer = filtered_buffer;
                }
            }
        }

        result
    }

    fn filter_leaf_node_axis(
        &self,
        grid: &Grid<ValueTy>,
        node: &Node3<ValueTy>,
        width: i32,
        axis: usize,
        weight: f32,
    ) -> Vec<ValueTy> {
        let dim = 1 << Node3::<ValueTy>::LOG_2_DIM; // 2^3 = 8
        let mut filtered = node.buffer.clone();

        // Iterate through all voxels in the leaf node
        for z in 0..dim {
            for y in 0..dim {
                for x in 0..dim {
                    let idx = (z * dim * dim + y * dim + x) as usize;

                    if !node.value_mask[idx] {
                        continue; // Skip inactive voxels
                    }

                    let local_coord = IVec3::new(x, y, z);
                    let world_coord = node.origin + local_coord;

                    // Accumulate values along the axis
                    let mut sum = 0.0f32;
                    let mut count = 0;

                    for offset in -width..=width {
                        let mut sample_coord = world_coord;
                        sample_coord[axis] += offset;

                        if let Some(value) = self.sample_voxel(grid, sample_coord) {
                            sum += value.to_f32();
                            count += 1;
                        }
                    }

                    if count > 0 {
                        filtered[idx] = ValueTy::from_f32(sum * weight);
                    }
                }
            }
        }

        filtered
    }

    fn median_pass(&self, grid: &Grid<ValueTy>, width: i32) -> Grid<ValueTy> {
        let mut result = grid.clone();

        // Process each leaf node
        for root_node in &mut result.tree.root_nodes {
            for node4 in root_node.nodes.values_mut() {
                for node3 in node4.nodes.values_mut() {
                    let filtered_buffer = self.filter_leaf_node_median(
                        grid,
                        node3,
                        width,
                    );
                    node3.buffer = filtered_buffer;
                }
            }
        }

        result
    }

    fn filter_leaf_node_median(
        &self,
        grid: &Grid<ValueTy>,
        node: &Node3<ValueTy>,
        width: i32,
    ) -> Vec<ValueTy> {
        let dim = 1 << Node3::<ValueTy>::LOG_2_DIM;
        let mut filtered = node.buffer.clone();

        for z in 0..dim {
            for y in 0..dim {
                for x in 0..dim {
                    let idx = (z * dim * dim + y * dim + x) as usize;

                    if !node.value_mask[idx] {
                        continue;
                    }

                    let local_coord = IVec3::new(x, y, z);
                    let world_coord = node.origin + local_coord;

                    // Collect neighborhood values
                    let mut values = Vec::new();

                    for dz in -width..=width {
                        for dy in -width..=width {
                            for dx in -width..=width {
                                let sample_coord = world_coord + IVec3::new(dx, dy, dz);
                                if let Some(value) = self.sample_voxel(grid, sample_coord) {
                                    values.push(value.to_f32());
                                }
                            }
                        }
                    }

                    if !values.is_empty() {
                        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let median = values[values.len() / 2];
                        filtered[idx] = ValueTy::from_f32(median);
                    }
                }
            }
        }

        filtered
    }

    fn sample_voxel(&self, grid: &Grid<ValueTy>, coord: IVec3) -> Option<ValueTy> {
        // Find the leaf node containing this coordinate
        for root_node in &grid.tree.root_nodes {
            for node4 in root_node.nodes.values() {
                for node3 in node4.nodes.values() {
                    let dim = 1 << Node3::<ValueTy>::LOG_2_DIM;
                    let local = coord - node3.origin;

                    if local.x >= 0 && local.x < dim
                        && local.y >= 0 && local.y < dim
                        && local.z >= 0 && local.z < dim
                    {
                        let idx = (local.z * dim * dim + local.y * dim + local.x) as usize;
                        if idx < node3.buffer.len() && node3.value_mask[idx] {
                            return Some(node3.buffer[idx]);
                        }
                    }
                }
            }
        }
        None
    }
}

impl<ValueTy: Clone> Clone for Grid<ValueTy> {
    fn clone(&self) -> Self {
        Self {
            tree: self.tree.clone(),
            transform: self.transform.clone(),
            descriptor: self.descriptor.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Compression, GridDescriptor, Map, Metadata, Node4, Node5, Tree};
    use bitvec::prelude::*;
    use std::collections::HashMap;

    /// Create a simple test grid with known values
    fn create_test_grid() -> Grid<f32> {
        // Create a simple 8x8x8 grid with values
        let mut buffer = vec![0.0f32; 512]; // 8^3
        let mut value_mask = bitvec![u64, Lsb0; 0; 512];

        // Fill with a pattern: values increase with distance from origin
        for z in 0..8 {
            for y in 0..8 {
                for x in 0..8 {
                    let idx = (z * 64 + y * 8 + x) as usize;
                    let distance = ((x * x + y * y + z * z) as f32).sqrt();
                    buffer[idx] = distance;
                    value_mask.set(idx, true);
                }
            }
        }

        let node3 = Node3 {
            buffer,
            value_mask,
            origin: IVec3::ZERO,
        };

        let mut node4_map = HashMap::new();
        node4_map.insert(0, node3);

        let node4 = Node4 {
            child_mask: bitvec![u64, Lsb0; 1; 4096],
            value_mask: bitvec![u64, Lsb0; 0; 4096],
            nodes: node4_map,
            data: vec![],
            origin: IVec3::ZERO,
        };

        let mut node5_map = HashMap::new();
        node5_map.insert(0, node4);

        let node5 = Node5 {
            child_mask: bitvec![u64, Lsb0; 1; 32768],
            value_mask: bitvec![u64, Lsb0; 0; 32768],
            nodes: node5_map,
            data: vec![],
            origin: IVec3::ZERO,
        };

        Grid {
            tree: Tree {
                root_nodes: vec![node5],
            },
            transform: Map::UniformScaleMap {
                scale_values: glam::DVec3::ONE,
                voxel_size: glam::DVec3::ONE,
                scale_values_inverse: glam::DVec3::ONE,
                inv_scale_sqr: glam::DVec3::ONE,
                inv_twice_scale: glam::DVec3::splat(0.5),
            },
            descriptor: GridDescriptor {
                name: "test".to_string(),
                file_version: 0,
                instance_parent: String::new(),
                grid_type: "float".to_string(),
                grid_pos: 0,
                block_pos: 0,
                end_pos: 0,
                compression: Compression::NONE,
                meta_data: Metadata::default(),
            },
        }
    }

    #[test]
    fn test_offset() {
        let grid = create_test_grid();
        let filter = Filter::new(&grid);
        let offset_value = 2.5f32;

        // Apply offset
        let result = filter.offset(offset_value);

        // Verify all values are offset correctly
        let original_node = &grid.tree.root_nodes[0].nodes[&0].nodes[&0];
        let result_node = &result.tree.root_nodes[0].nodes[&0].nodes[&0];

        for (idx, active) in original_node.value_mask.iter().enumerate() {
            if *active && idx < original_node.buffer.len() {
                let original_val = original_node.buffer[idx];
                let result_val = result_node.buffer[idx];
                let expected = original_val + offset_value;
                assert!(
                    (result_val - expected).abs() < 0.0001,
                    "Offset failed at index {}: expected {}, got {}",
                    idx,
                    expected,
                    result_val
                );
            }
        }
    }

    #[test]
    fn test_mean_filter_center_voxel() {
        let grid = create_test_grid();
        let filter = Filter::new(&grid);

        // Apply mean filter with width 1
        let config = FilterConfig {
            width: 1,
            iterations: 1,
        };
        let result = filter.mean(config);

        // Check the center voxel (4,4,4) - it should be averaged with its 26 neighbors
        // We can't easily compute the exact expected value without sampling,
        // but we can verify it changed and is within reasonable bounds
        let original_node = &grid.tree.root_nodes[0].nodes[&0].nodes[&0];
        let result_node = &result.tree.root_nodes[0].nodes[&0].nodes[&0];

        let center_idx = (4 * 64 + 4 * 8 + 4) as usize;
        let original_val = original_node.buffer[center_idx];
        let result_val = result_node.buffer[center_idx];

        // The filtered value should be different but in a reasonable range
        assert_ne!(original_val, result_val, "Mean filter should change values");
        assert!(
            result_val > 0.0 && result_val < 15.0,
            "Filtered value out of expected range: {}",
            result_val
        );
    }

    #[test]
    fn test_gaussian_approximation() {
        let grid = create_test_grid();
        let filter = Filter::new(&grid);

        let config = FilterConfig {
            width: 1,
            iterations: 1,
        };

        // Gaussian should apply mean 4 times
        let gaussian_result = filter.gaussian(config);

        // Apply mean 4 times manually
        let mean_config = FilterConfig {
            width: 1,
            iterations: 4,
        };
        let mean_result = filter.mean(mean_config);

        // Results should be identical
        let gaussian_node = &gaussian_result.tree.root_nodes[0].nodes[&0].nodes[&0];
        let mean_node = &mean_result.tree.root_nodes[0].nodes[&0].nodes[&0];

        for idx in 0..gaussian_node.buffer.len() {
            let diff = (gaussian_node.buffer[idx] - mean_node.buffer[idx]).abs();
            assert!(
                diff < 0.0001,
                "Gaussian should equal 4x mean iterations at index {}: diff = {}",
                idx,
                diff
            );
        }
    }

    #[test]
    fn test_median_filter() {
        let grid = create_test_grid();
        let filter = Filter::new(&grid);

        let config = FilterConfig {
            width: 1,
            iterations: 1,
        };
        let result = filter.median(config);

        // Verify the result is different from original
        let original_node = &grid.tree.root_nodes[0].nodes[&0].nodes[&0];
        let result_node = &result.tree.root_nodes[0].nodes[&0].nodes[&0];

        let mut changed_count = 0;
        for idx in 0..original_node.buffer.len() {
            if (original_node.buffer[idx] - result_node.buffer[idx]).abs() > 0.0001 {
                changed_count += 1;
            }
        }

        // At least some values should change
        assert!(
            changed_count > 0,
            "Median filter should change at least some values"
        );
    }

    #[test]
    fn test_multiple_iterations() {
        let grid = create_test_grid();
        let filter = Filter::new(&grid);

        // Single iteration
        let config1 = FilterConfig {
            width: 1,
            iterations: 1,
        };
        let result1 = filter.mean(config1);

        // Two iterations
        let config2 = FilterConfig {
            width: 1,
            iterations: 2,
        };
        let result2 = filter.mean(config2);

        // Results should be different
        let node1 = &result1.tree.root_nodes[0].nodes[&0].nodes[&0];
        let node2 = &result2.tree.root_nodes[0].nodes[&0].nodes[&0];

        let center_idx = (4 * 64 + 4 * 8 + 4) as usize;
        assert_ne!(
            node1.buffer[center_idx], node2.buffer[center_idx],
            "Multiple iterations should produce different results"
        );
    }

    #[test]
    fn test_filterable_f32() {
        let val = 3.5f32;
        assert_eq!(val.to_f32(), 3.5);
        assert_eq!(f32::from_f32(3.5), 3.5);
    }

    #[test]
    fn test_filterable_f16() {
        let val = half::f16::from_f32(3.5);
        assert!((val.to_f32() - 3.5).abs() < 0.001);
        let result = half::f16::from_f32(3.5);
        assert!((result.to_f32() - 3.5).abs() < 0.001);
    }
}
