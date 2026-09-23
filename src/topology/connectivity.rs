//! Wire-based connectivity inference
//!
//! This module infers electrical connections by analyzing wire geometry,
//! junctions, and labels in the schematic.

use std::collections::HashMap;

use crate::ir::{Label, Schematic, SymbolInstance};

/// A point in the schematic with some tolerance for floating point comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    /// X coordinate scaled to integer (multiplied by 1000 for 0.001 precision)
    pub x: i64,
    /// Y coordinate scaled to integer
    pub y: i64,
}

impl Point {
    /// Create a new point from floating point coordinates
    pub fn new(x: f64, y: f64) -> Self {
        // Scale to get 0.001 precision
        Self {
            x: (x * 1000.0).round() as i64,
            y: (y * 1000.0).round() as i64,
        }
    }

    /// Check if this point is near another point (within tolerance)
    pub fn is_near(&self, other: &Point, tolerance: f64) -> bool {
        let dx = (self.x - other.x).abs() as f64 / 1000.0;
        let dy = (self.y - other.y).abs() as f64 / 1000.0;
        dx <= tolerance && dy <= tolerance
    }
}

/// Union-Find data structure for grouping connected points
#[derive(Debug, Clone)]
pub struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    pub fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    pub fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    pub fn union(&mut self, x: usize, y: usize) {
        let px = self.find(x);
        let py = self.find(y);
        if px == py {
            return;
        }
        if self.rank[px] < self.rank[py] {
            self.parent[px] = py;
        } else if self.rank[px] > self.rank[py] {
            self.parent[py] = px;
        } else {
            self.parent[py] = px;
            self.rank[px] += 1;
        }
    }
}

/// Connectivity information extracted from the schematic
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Connectivity {
    /// Point to index mapping
    point_indices: HashMap<Point, usize>,
    /// Index to point mapping
    points: Vec<Point>,
    /// Connected net groups (root index -> group of point indices)
    net_groups: HashMap<usize, Vec<usize>>,
    /// Point to net name mapping (inferred from labels)
    point_nets: HashMap<Point, String>,
    /// Net name to connected points mapping
    net_points: HashMap<String, Vec<Point>>,
}

impl Connectivity {
    /// Create a new empty connectivity map
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            point_indices: HashMap::new(),
            points: Vec::new(),
            net_groups: HashMap::new(),
            point_nets: HashMap::new(),
            net_points: HashMap::new(),
        }
    }

    /// Get or create an index for a point
    fn get_or_create_point(&mut self, point: Point) -> usize {
        if let Some(&idx) = self.point_indices.get(&point) {
            return idx;
        }
        let idx = self.points.len();
        self.points.push(point);
        self.point_indices.insert(point, idx);
        idx
    }

    /// Build connectivity from a schematic
    pub fn build(schematic: &Schematic) -> Self {
        let mut connectivity = Self::new();

        // Collect all points from wires
        for wire in &schematic.wires {
            let start = Point::new(wire.start.0, wire.start.1);
            let end = Point::new(wire.end.0, wire.end.1);
            connectivity.get_or_create_point(start);
            connectivity.get_or_create_point(end);
        }

        // Collect all junction points
        for junction in &schematic.junctions {
            let point = Point::new(junction.position.0, junction.position.1);
            connectivity.get_or_create_point(point);
        }

        // Build union-find structure
        let mut uf = UnionFind::new(connectivity.points.len());

        // Union wire endpoints (each wire connects its start and end)
        for wire in &schematic.wires {
            let start = Point::new(wire.start.0, wire.start.1);
            let end = Point::new(wire.end.0, wire.end.1);
            if let (Some(&start_idx), Some(&end_idx)) = (
                connectivity.point_indices.get(&start),
                connectivity.point_indices.get(&end),
            ) {
                uf.union(start_idx, end_idx);
            }
        }

        // Union junctions with nearby wire endpoints
        // Junctions connect all wires that pass through their position
        for junction in &schematic.junctions {
            let junction_point = Point::new(junction.position.0, junction.position.1);

            // Find all wire endpoints near this junction
            let mut nearby_points = Vec::new();
            for wire in &schematic.wires {
                let start = Point::new(wire.start.0, wire.start.1);
                let end = Point::new(wire.end.0, wire.end.1);

                if start.is_near(&junction_point, 0.1) || end.is_near(&junction_point, 0.1) {
                    if let Some(&idx) = connectivity.point_indices.get(&start) {
                        nearby_points.push(idx);
                    }
                    if let Some(&idx) = connectivity.point_indices.get(&end) {
                        nearby_points.push(idx);
                    }
                }
            }

            // Union all nearby points
            for i in 1..nearby_points.len() {
                uf.union(nearby_points[0], nearby_points[i]);
            }
        }

        // Build net groups from union-find
        let mut net_groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..connectivity.points.len() {
            let root = uf.find(i);
            net_groups.entry(root).or_default().push(i);
        }
        connectivity.net_groups = net_groups;

        // Map labels to net groups
        connectivity.map_labels(&schematic.labels);

        connectivity
    }

    /// Map labels to connected net groups
    fn map_labels(&mut self, labels: &[Label]) {
        for label in labels {
            let label_point = Point::new(label.position.0, label.position.1);

            // Find the net group this label belongs to
            for (_root, indices) in &self.net_groups {
                // Check if label is near any point in this group
                let is_connected = indices
                    .iter()
                    .any(|&idx| self.points[idx].is_near(&label_point, 1.0));

                if is_connected {
                    // Assign this net name to all points in the group
                    let net_name = if !label.text.is_empty() {
                        label.text.clone()
                    } else if let Some(ref net_name) = label.net_name {
                        net_name.clone()
                    } else {
                        continue;
                    };

                    // Map all points in this group to the net name
                    for &idx in indices {
                        let point = self.points[idx];
                        self.point_nets.insert(point, net_name.clone());
                    }

                    // Add to net_points
                    let points: Vec<Point> =
                        indices.iter().map(|&idx| self.points[idx]).collect();
                    self.net_points
                        .entry(net_name)
                        .or_default()
                        .extend(points);
                    break;
                }
            }
        }
    }

    /// Get the net name at a specific point
    #[allow(dead_code)]
    pub fn get_net_at(&self, x: f64, y: f64, tolerance: f64) -> Option<String> {
        let query_point = Point::new(x, y);

        // First try exact match
        if let Some(net_name) = self.point_nets.get(&query_point) {
            return Some(net_name.clone());
        }

        // Try nearby points
        for (point, net_name) in &self.point_nets {
            if point.is_near(&query_point, tolerance) {
                return Some(net_name.clone());
            }
        }

        None
    }

    /// Get all points in the same net as a given point
    #[allow(dead_code)]
    pub fn get_connected_points(&self, x: f64, y: f64) -> Vec<Point> {
        let query_point = Point::new(x, y);

        // Find the index of this point
        if let Some(&idx) = self.point_indices.get(&query_point) {
            // Find the root and return all points in that group
            // Note: This requires UnionFind to be stored, but for simplicity
            // we'll use the net_groups which were computed during build
            for indices in self.net_groups.values() {
                if indices.contains(&idx) {
                    return indices.iter().map(|&i| self.points[i]).collect();
                }
            }
        }

        Vec::new()
    }

    /// Get all net names
    #[allow(dead_code)]
    pub fn get_net_names(&self) -> Vec<String> {
        self.net_points.keys().cloned().collect()
    }

    /// Get all points for a net
    #[allow(dead_code)]
    pub fn get_net_points(&self, net_name: &str) -> Option<&Vec<Point>> {
        self.net_points.get(net_name)
    }

    /// Check if a point is on any wire
    #[allow(dead_code)]
    pub fn is_on_wire(&self, x: f64, y: f64, tolerance: f64) -> bool {
        let query_point = Point::new(x, y);

        // Check if point is near any wire endpoint
        for point in &self.points {
            if point.is_near(&query_point, tolerance) {
                return true;
            }
        }

        false
    }
}

/// Pin position resolver for components
#[allow(dead_code)]
pub struct PinResolver<'a> {
    schematic: &'a Schematic,
    /// Component reference -> (pin_number -> pin_position)
    pin_positions: HashMap<String, HashMap<String, (f64, f64)>>,
}

impl<'a> PinResolver<'a> {
    #[allow(dead_code)]
    pub fn new(schematic: &'a Schematic) -> Self {
        Self {
            schematic,
            pin_positions: HashMap::new(),
        }
    }

    /// Calculate pin positions for all components
    /// Note: This requires symbol library data which may not be fully available
    /// For now, we use a simplified approach based on component position
    #[allow(dead_code)]
    pub fn build(&mut self) {
        // This is a placeholder - in a full implementation,
        // we would need to access symbol library data to get pin offsets
        // For now, we rely on the net information stored in PinInstance
    }

    /// Get the position of a component's pin
    /// Returns None if the pin position cannot be determined
    #[allow(dead_code)]
    pub fn get_pin_position(
        &self,
        _component: &SymbolInstance,
        _pin_number: &str,
    ) -> Option<(f64, f64)> {
        // This would require symbol library data
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_creation() {
        let p1 = Point::new(100.0, 200.0);
        let p2 = Point::new(100.0004, 200.0004); // 0.0004 * 1000 = 0.4, rounds to 0
        assert_eq!(p1, p2); // Should be equal due to rounding
    }

    #[test]
    fn test_point_is_near() {
        let p1 = Point::new(100.0, 200.0);
        let p2 = Point::new(100.5, 200.5);
        assert!(p1.is_near(&p2, 1.0));
        assert!(!p1.is_near(&p2, 0.1));
    }

    #[test]
    fn test_union_find() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(1, 2);
        assert_eq!(uf.find(0), uf.find(2));
        assert_ne!(uf.find(0), uf.find(3));
    }
}
