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
