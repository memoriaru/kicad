//! Constraint-based placement solver for IC peripheral circuits.

use std::collections::HashMap;

use super::types::{BodyDims, PlacementEntry, PlacementResult};

const MIL_TO_MM: f64 = 0.0254;

/// Constraint-based placement solver for component auto-layout.
///
/// Resolves positions from constraint entries (NearPin, BetweenPins, OffsetFrom, Fixed)
/// and performs collision detection/resolution.
pub struct PlacementSolver {
    resolved: HashMap<String, PlacementResult>,
    ic_pins: HashMap<String, (f64, f64)>,
    body_dims: HashMap<String, BodyDims>,
}

impl PlacementSolver {
    /// Create a new solver anchored at `ic_pos` with the given IC pin positions.
    pub fn new(ic_pos: (f64, f64), ic_pin_positions: &HashMap<String, (f64, f64)>) -> Self {
        let mut resolved = HashMap::new();
        resolved.insert(
            "ic".into(),
            PlacementResult {
                x: ic_pos.0,
                y: ic_pos.1,
                rotation: 0.0,
            },
        );
        Self {
            resolved,
            ic_pins: ic_pin_positions.clone(),
            body_dims: HashMap::new(),
        }
    }

    /// Register body dimensions for a component role (for collision detection)
    pub fn register_body(&mut self, role: &str, width: f64, height: f64) {
        self.body_dims
            .insert(role.to_string(), BodyDims { width, height });
    }

    /// Resolve all peripheral placements from layout constraints.
    pub fn resolve_all(&mut self, layout: &HashMap<String, PlacementEntry>) {
        // Resolve in dependency order: Fixed first, then NearPin/BetweenPins, then OffsetFrom
        // Pass 1: Fixed
        for (role, entry) in layout {
            if role == "ic" {
                continue;
            }
            if let PlacementEntry::Fixed { x, y } = entry {
                self.resolved.insert(
                    role.clone(),
                    PlacementResult {
                        x: *x,
                        y: *y,
                        rotation: 0.0,
                    },
                );
            }
        }

        // Pass 2: NearPin + BetweenPins
        // Track per-pin placement count to stagger overlapping NearPin components vertically.
        let mut pin_count: HashMap<String, usize> = HashMap::new();
        let stagger_grid = 7.62; // mm — 3 schematic grid units

        for (role, entry) in layout {
            if role == "ic" {
                continue;
            }
            match entry {
                PlacementEntry::NearPin {
                    near_pin,
                    max_distance_mil,
                    side,
                    clearance_mil,
                } => {
                    if let Some(mut result) = self.resolve_near_pin(
                        near_pin,
                        *max_distance_mil,
                        side.as_deref(),
                        clearance_mil.unwrap_or(20.0),
                    ) {
                        let count = pin_count.entry(near_pin.clone()).or_insert(0);
                        if *count > 0 {
                            result.y += *count as f64 * stagger_grid;
                        }
                        *count += 1;
                        self.resolved.insert(role.clone(), result);
                    }
                }
                PlacementEntry::BetweenPins {
                    pin_a,
                    pin_b,
                    offset_mil,
                    side,
                } => {
                    if let Some(result) = self.resolve_between_pins(
                        pin_a,
                        pin_b,
                        offset_mil.unwrap_or(0.0),
                        side.as_deref(),
                    ) {
                        self.resolved.insert(role.clone(), result);
                    }
                }
                _ => {}
            }
        }

        // Pass 3: OffsetFrom (depends on other components being placed)
        for (role, entry) in layout {
            if role == "ic" {
                continue;
            }
            if let PlacementEntry::OffsetFrom {
                offset_from,
                dx_mil,
                dy_mil,
            } = entry
            {
                if let Some(result) = self.resolve_offset_from(offset_from, *dx_mil, *dy_mil) {
                    self.resolved.insert(role.clone(), result);
                }
            }
        }

        // Pass 4: Collision resolution
        self.resolve_collisions();
    }

    /// Get resolved position for a role
    pub fn get(&self, role: &str) -> Option<PlacementResult> {
        self.resolved.get(role).copied()
    }

    /// Get resolved position with fallback
    pub fn get_or(&self, role: &str, fallback: (f64, f64, f64)) -> PlacementResult {
        self.resolved.get(role).copied().unwrap_or(PlacementResult {
            x: fallback.0,
            y: fallback.1,
            rotation: fallback.2,
        })
    }

    // ── Constraint resolvers ──────────────────────────────────────────

    fn resolve_near_pin(
        &self,
        near_pin: &str,
        max_distance_mil: f64,
        side: Option<&str>,
        clearance_mil: f64,
    ) -> Option<PlacementResult> {
        let (px, py) = self.resolve_pin_target(near_pin)?;
        let max_dist = max_distance_mil * MIL_TO_MM;
        let clearance = clearance_mil * MIL_TO_MM;

        let (dir_x, dir_y, rotation) = match side {
            Some("left") => (-1.0, 0.0, 0.0),
            Some("right") => (1.0, 0.0, 0.0),
            Some("above") => (0.0, -1.0, 90.0),
            Some("below") => (0.0, 1.0, 90.0),
            _ => {
                let ic = self.resolved.get("ic")?;
                let dx = px - ic.x;
                let dy = py - ic.y;
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                let rot = if dx.abs() > dy.abs() { 0.0 } else { 90.0 };
                (dx / len, dy / len, rot)
            }
        };

        let offset = clearance;
        let dist = offset.min(max_dist);

        let x = px + dir_x * dist;
        let y = py + dir_y * dist;

        Some(PlacementResult { x, y, rotation })
    }

    fn resolve_between_pins(
        &self,
        pin_a: &str,
        pin_b: &str,
        offset_mil: f64,
        side: Option<&str>,
    ) -> Option<PlacementResult> {
        let (ax, ay) = self.resolve_pin_target(pin_a)?;
        let (bx, by) = self.resolve_pin_target(pin_b)?;

        let mid_x = (ax + bx) / 2.0;
        let mid_y = (ay + by) / 2.0;

        let dx = bx - ax;
        let dy = by - ay;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);

        // Normal direction (perpendicular to pin-pin line)
        let nx = -dy / len;
        let ny = dx / len;

        let offset = offset_mil * MIL_TO_MM;

        let (sign, rotation) = match side {
            Some("right") | Some("below") => (-1.0, 0.0),
            Some("left") | Some("above") => (1.0, 0.0),
            _ => {
                // Default: place on the side away from IC center
                let ic = self.resolved.get("ic")?;
                let to_ic_x = ic.x - mid_x;
                let to_ic_y = ic.y - mid_y;
                let dot = to_ic_x * nx + to_ic_y * ny;
                if dot > 0.0 {
                    (-1.0, 0.0)
                } else {
                    (1.0, 0.0)
                }
            }
        };

        let x = mid_x + sign * nx * offset;
        let y = mid_y + sign * ny * offset;

        // Rotation: align component along the pin-pin direction
        let angle = dy.atan2(dx).to_degrees();

        Some(PlacementResult {
            x,
            y,
            rotation: angle + rotation,
        })
    }

    fn resolve_offset_from(
        &self,
        target: &str,
        dx_mil: f64,
        dy_mil: f64,
    ) -> Option<PlacementResult> {
        let base = self.resolved.get(target)?;
        Some(PlacementResult {
            x: base.x + dx_mil * MIL_TO_MM,
            y: base.y + dy_mil * MIL_TO_MM,
            rotation: base.rotation,
        })
    }

    /// Parse "ic.VIN" or just "VIN" into an absolute pin position
    fn resolve_pin_target(&self, target: &str) -> Option<(f64, f64)> {
        let pin_name = if let Some(dot_pos) = target.find('.') {
            &target[dot_pos + 1..]
        } else {
            target
        };
        self.ic_pins.get(pin_name).copied()
    }

    // ── Collision detection & resolution ──────────────────────────────

    fn resolve_collisions(&mut self) {
        let roles: Vec<String> = self.resolved.keys().cloned().collect();
        let max_iterations = 20;

        for _ in 0..max_iterations {
            let mut any_collision = false;

            for i in 0..roles.len() {
                for j in (i + 1)..roles.len() {
                    let ri = &roles[i];
                    let rj = &roles[j];

                    let pos_i = self.resolved.get(ri).copied();
                    let pos_j = self.resolved.get(rj).copied();
                    let (Some(pi), Some(pj)) = (pos_i, pos_j) else {
                        continue;
                    };

                    let dim_i = self.body_dims.get(ri).copied().unwrap_or(BodyDims {
                        width: 1.0,
                        height: 1.0,
                    });
                    let dim_j = self.body_dims.get(rj).copied().unwrap_or(BodyDims {
                        width: 1.0,
                        height: 1.0,
                    });

                    if rects_overlap(
                        (pi.x, pi.y, dim_i.width, dim_i.height),
                        (pj.x, pj.y, dim_j.width, dim_j.height),
                    ) {
                        any_collision = true;
                        let dx = pj.x - pi.x;
                        let dy = pj.y - pi.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        let push = 0.5; // mm step

                        let (dir_x, dir_y) = if len < 0.01 {
                            (1.0, 0.0)
                        } else {
                            (dx / len, dy / len)
                        };

                        if rj != "ic" {
                            if let Some(pos) = self.resolved.get_mut(rj) {
                                pos.x += dir_x * push;
                                pos.y += dir_y * push;
                            }
                        } else if ri != "ic" {
                            if let Some(pos) = self.resolved.get_mut(ri) {
                                pos.x -= dir_x * push;
                                pos.y -= dir_y * push;
                            }
                        }
                    }
                }
            }

            if !any_collision {
                break;
            }
        }
    }
}

fn rects_overlap(
    (x1, y1, w1, h1): (f64, f64, f64, f64),
    (x2, y2, w2, h2): (f64, f64, f64, f64),
) -> bool {
    let gap = 0.2; // minimum clearance mm
    !(x1 + w1 / 2.0 + gap < x2 - w2 / 2.0
        || x1 - w1 / 2.0 - gap > x2 + w2 / 2.0
        || y1 + h1 / 2.0 + gap < y2 - h2 / 2.0
        || y1 - h1 / 2.0 - gap > y2 + h2 / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_solver_at(pos: (f64, f64)) -> PlacementSolver {
        PlacementSolver::new(pos, &HashMap::new())
    }

    #[test]
    fn test_near_pin_left() {
        let mut pins = HashMap::new();
        pins.insert("VIN".into(), (38.0, 0.0));

        let mut layout = HashMap::new();
        layout.insert(
            "c_in".into(),
            PlacementEntry::NearPin {
                near_pin: "ic.VIN".into(),
                max_distance_mil: 200.0,
                side: Some("left".into()),
                clearance_mil: Some(20.0),
            },
        );

        let mut solver = PlacementSolver::new((40.0, 0.0), &pins);
        solver.resolve_all(&layout);

        let pos = solver.get("c_in").unwrap();
        assert!(
            pos.x < 38.0,
            "c_in should be left of VIN pin, got x={}",
            pos.x
        );
        assert!(
            (pos.y - 0.0).abs() < 1.0,
            "c_in should be at similar y as VIN pin"
        );
    }

    #[test]
    fn test_near_pin_right() {
        let mut pins = HashMap::new();
        pins.insert("VOUT".into(), (42.0, 0.0));

        let mut layout = HashMap::new();
        layout.insert(
            "c_out".into(),
            PlacementEntry::NearPin {
                near_pin: "ic.VOUT".into(),
                max_distance_mil: 200.0,
                side: Some("right".into()),
                clearance_mil: Some(20.0),
            },
        );

        let mut solver = PlacementSolver::new((40.0, 0.0), &pins);
        solver.resolve_all(&layout);

        let pos = solver.get("c_out").unwrap();
        assert!(
            pos.x > 42.0,
            "c_out should be right of VOUT pin, got x={}",
            pos.x
        );
    }

    #[test]
    fn test_between_pins() {
        let mut pins = HashMap::new();
        pins.insert("VOUT".into(), (42.0, 0.0));
        pins.insert("FB".into(), (38.0, 2.0));

        let mut layout = HashMap::new();
        layout.insert(
            "r_fb".into(),
            PlacementEntry::BetweenPins {
                pin_a: "ic.VOUT".into(),
                pin_b: "ic.FB".into(),
                offset_mil: Some(40.0),
                side: Some("right".into()),
            },
        );

        let mut solver = PlacementSolver::new((40.0, 0.0), &pins);
        solver.resolve_all(&layout);

        let pos = solver.get("r_fb").unwrap();
        let mid_x = (42.0 + 38.0) / 2.0;
        assert!(
            (pos.x - mid_x).abs() < 5.0,
            "r_fb should be near midpoint x, got {}",
            pos.x
        );
    }

    #[test]
    fn test_offset_from() {
        let mut layout = HashMap::new();
        layout.insert("r1".into(), PlacementEntry::Fixed { x: 60.0, y: 10.0 });
        layout.insert(
            "r2".into(),
            PlacementEntry::OffsetFrom {
                offset_from: "r1".into(),
                dx_mil: 0.0,
                dy_mil: 100.0, // 100 mil ≈ 2.54mm
            },
        );

        let mut solver = make_solver_at((40.0, 0.0));
        solver.resolve_all(&layout);

        let r2 = solver.get("r2").unwrap();
        let expected_y = 10.0 + 100.0 * MIL_TO_MM;
        assert!(
            (r2.y - expected_y).abs() < 0.01,
            "r2 y offset should be ~{}, got {}",
            expected_y,
            r2.y
        );
        assert!((r2.x - 60.0).abs() < 0.01, "r2 x should stay at 60.0");
    }

    #[test]
    fn test_collision_avoidance() {
        let mut layout = HashMap::new();
        layout.insert("c1".into(), PlacementEntry::Fixed { x: 50.0, y: 0.0 });
        layout.insert("c2".into(), PlacementEntry::Fixed { x: 50.0, y: 0.0 });

        let mut solver = make_solver_at((40.0, 0.0));
        solver.register_body("c1", 2.0, 1.25);
        solver.register_body("c2", 2.0, 1.25);
        solver.resolve_all(&layout);

        let c1 = solver.get("c1").unwrap();
        let c2 = solver.get("c2").unwrap();

        let dist = ((c2.x - c1.x).powi(2) + (c2.y - c1.y).powi(2)).sqrt();
        assert!(
            dist > 2.0,
            "Colliding components should be separated, dist={}",
            dist
        );
    }
}
