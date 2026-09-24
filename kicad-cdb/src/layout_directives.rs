//! Bridge between design rule evaluation results and PCB layout constraints.
//!
//! Design rules (skills.rs) compute electrical parameters (copper area, thermal margins,
//! decoupling distances). This module translates those numerical results into physical
//! layout directives consumed by the layout engine and board generator.

use std::collections::HashMap;

use crate::pipeline::DesignLog;

// ---------------------------------------------------------------------------
// Directive Data Structures
// ---------------------------------------------------------------------------

/// Aggregated layout directives from all rule domains.
#[derive(Debug, Default, Clone)]
pub struct LayoutDirectives {
    pub thermal: Vec<ThermalDirective>,
    pub emc: Vec<EmcDirective>,
    pub signal_integrity: Vec<SignalIntegrityDirective>,
    pub clock: Vec<ClockDirective>,
    pub protection: Vec<ProtectionDirective>,
    pub net_classes: Vec<NetClassDirective>,
    pub sa_weights: SaCostWeights,
    /// P1-8: CLI override — forces this trace width on every signal net,
    /// taking precedence over net classes and SI directives.
    pub trace_width_override: Option<f64>,
    /// P1-8: CLI override — forces this clearance for the whole routing run
    /// (no adaptive reduction in late rip-up rounds).
    pub clearance_override: Option<f64>,
    /// P1-8: per-net exact-name width overrides (keys uppercased; highest
    /// precedence, beats trace_width_override for the matched nets).
    pub net_width_overrides: HashMap<String, f64>,
    /// v35（M2）：连接器锚边覆盖——ref（原样大小写）→ 目标板边。
    /// 命中时 generate_constraints 用覆盖值替代 infer_connector_edge 的
    /// 网络名推断（排线自由度：连接器位置可任选）。
    pub anchor_overrides: HashMap<String, crate::layout_engine::AnchorType>,
}

/// SA floorplanner cost function weights.
#[derive(Debug, Clone)]
pub struct SaCostWeights {
    pub wire_length: f64,
    pub board_area: f64,
    pub overlap: f64,
    pub congestion: f64,
    /// Penalty for component pairs with insufficient routing channel (< 1mm gap).
    pub routing_congestion: f64,
}

impl Default for SaCostWeights {
    fn default() -> Self {
        Self {
            wire_length: 2.0,
            board_area: 0.001,
            overlap: 50.0,
            congestion: 2.0,
            routing_congestion: 0.5,
        }
    }
}

/// Thermal placement directive for a power-dissipating component.
#[derive(Debug, Clone)]
pub struct ThermalDirective {
    pub component_ref: String,
    pub copper_area_mm2: f64,
    pub via_count: usize,
    pub via_grid_spacing_mm: f64,
    pub margin_mm: f64,
}

/// EMC decoupling capacitor placement directive.
#[derive(Debug, Clone)]
pub struct EmcDirective {
    pub component_ref: String,
    pub target_ic_ref: String,
    pub max_distance_mm: f64,
    pub resonance_freq_hz: f64,
    pub cap_rank: usize,
}

/// Signal integrity routing directive for a net.
#[derive(Debug, Clone)]
pub struct SignalIntegrityDirective {
    pub net_name: String,
    pub min_trace_width_mm: f64,
    pub min_spacing_mm: f64,
    pub target_impedance_ohm: f64,
}

/// Clock/crystal placement directive.
#[derive(Debug, Clone)]
pub struct ClockDirective {
    pub crystal_ref: String,
    pub load_cap_refs: Vec<String>,
    pub symmetric_placement: bool,
}

/// Protection component placement directive (TVS/ESD).
#[derive(Debug, Clone)]
pub struct ProtectionDirective {
    pub protection_ref: String,
    pub connector_ref: String,
    pub protected_ic_ref: String,
}

/// Net class routing directive.
#[derive(Debug, Clone)]
pub struct NetClassDirective {
    pub name: String,
    pub net_patterns: Vec<String>,
    pub trace_width_mm: f64,
    pub via_size_mm: f64,
    pub clearance_mm: f64,
}

// ---------------------------------------------------------------------------
// Default Net Classes
// ---------------------------------------------------------------------------

pub fn default_net_classes() -> Vec<NetClassDirective> {
    vec![
        NetClassDirective {
            name: "GND".into(),
            net_patterns: vec!["GND".into()],
            trace_width_mm: 0.8,
            via_size_mm: 0.8,
            clearance_mm: 0.2,
        },
        NetClassDirective {
            name: "Power".into(),
            net_patterns: vec![
                "VIN".into(),
                "5V".into(),
                "3V3".into(),
                "VCC".into(),
                "VDD".into(),
                "12V".into(),
                "VOUT".into(),
            ],
            trace_width_mm: 0.5,
            via_size_mm: 0.6,
            clearance_mm: 0.2,
        },
        NetClassDirective {
            name: "Signal".into(),
            net_patterns: vec![],
            trace_width_mm: 0.25,
            via_size_mm: 0.4,
            clearance_mm: 0.15,
        },
    ]
}

impl NetClassDirective {
    pub fn matches_net(&self, net_name: &str) -> bool {
        let upper = net_name.to_uppercase();
        self.net_patterns
            .iter()
            .any(|p| upper.contains(&p.to_uppercase()))
    }
}

impl LayoutDirectives {
    /// P1-8: effective routing clearance for a net — clearance_override wins,
    /// then the net class, then the 0.2mm router default.
    pub fn effective_clearance(&self, _net_name: &str) -> f64 {
        self.clearance_override.unwrap_or(0.2)
    }

    /// P1-8: apply CLI routing parameters. `width_by_net` entries are "NET=mm".
    pub fn apply_routing_overrides(
        &mut self,
        trace_width: Option<f64>,
        clearance: Option<f64>,
        width_by_net: &[(String, f64)],
    ) {
        self.trace_width_override = trace_width;
        self.clearance_override = clearance;
        for (name, w) in width_by_net {
            self.net_width_overrides.insert(name.to_uppercase(), *w);
        }
    }
}

// ---------------------------------------------------------------------------
// Directive Extraction
// ---------------------------------------------------------------------------

/// Rule name + its computed outputs.
type RuleStep<'a> = (&'a str, &'a HashMap<String, f64>);

/// Extract layout directives from a completed design log.
pub fn extract_directives(log: &DesignLog) -> LayoutDirectives {
    let mut directives = LayoutDirectives::default();

    let mut thermal_outputs: Vec<&HashMap<String, f64>> = Vec::new();
    let mut emc_outputs: Vec<RuleStep> = Vec::new();
    let mut si_outputs: Vec<&HashMap<String, f64>> = Vec::new();
    let mut clock_outputs: Vec<&HashMap<String, f64>> = Vec::new();

    for step in &log.steps {
        let domain = step.rule_name.split('_').next().unwrap_or("");
        match domain {
            "thermal" => thermal_outputs.push(&step.outputs),
            "emc" => emc_outputs.push((&step.rule_name, &step.outputs)),
            "si" => si_outputs.push(&step.outputs),
            "clock" => clock_outputs.push(&step.outputs),
            _ => {}
        }
    }

    extract_thermal(&thermal_outputs, &mut directives);
    extract_emc(&emc_outputs, &mut directives);
    extract_si(&si_outputs, &mut directives);
    extract_clock(&clock_outputs, &mut directives);

    directives
}

fn extract_thermal(outputs: &[&HashMap<String, f64>], directives: &mut LayoutDirectives) {
    let mut p_diss = 0.0_f64;
    let mut copper_area_mm2 = 0.0_f64;
    let mut copper_side_mm = 0.0_f64;
    let mut heatsink_needed = false;

    for out in outputs {
        if let Some(&v) = out.get("p_diss") {
            p_diss = p_diss.max(v);
        }
        if let Some(&v) = out.get("p_dissipated") {
            p_diss = p_diss.max(v);
        }
        if let Some(&v) = out.get("copper_area_mm2") {
            copper_area_mm2 = copper_area_mm2.max(v);
        }
        if let Some(&v) = out.get("copper_side_mm") {
            copper_side_mm = copper_side_mm.max(v);
        }
        if let Some(&v) = out.get("heatsink_needed") {
            heatsink_needed = v > 0.5;
        }
    }

    if p_diss < 0.01 {
        return;
    }

    let via_count = if heatsink_needed {
        ((copper_area_mm2 / 4.0).ceil() as usize).max(1)
    } else if p_diss > 1.0 {
        ((p_diss / 0.5).ceil() as usize).max(1)
    } else {
        0
    };

    let via_grid_spacing = if via_count > 1 && copper_side_mm > 0.0 {
        copper_side_mm / (via_count as f64).sqrt().max(1.0)
    } else {
        1.0
    };

    let margin = 2.5 + p_diss;

    directives.thermal.push(ThermalDirective {
        component_ref: String::new(),
        copper_area_mm2,
        via_count,
        via_grid_spacing_mm: via_grid_spacing,
        margin_mm: margin,
    });

    if p_diss > 0.5 {
        directives.net_classes.push(NetClassDirective {
            name: "PowerThermal".into(),
            net_patterns: vec!["VIN".into(), "VOUT".into(), "5V".into(), "3V3".into()],
            trace_width_mm: (0.5 + p_diss * 0.2).min(2.0),
            via_size_mm: 0.6,
            clearance_mm: 0.25,
        });
    }
}

fn extract_emc(outputs: &[RuleStep], directives: &mut LayoutDirectives) {
    let mut caps: Vec<(f64, f64)> = Vec::new();

    for (rule_name, out) in outputs {
        match *rule_name {
            "emc_decouple_resonance" => {
                let f_res = out.get("f_resonance").copied().unwrap_or(0.0);
                let max_dist = if f_res > 0.0 {
                    3.0 / (1.0 + (f_res / 1e6).log10().max(0.1))
                } else {
                    3.0
                };
                caps.push((f_res, max_dist.clamp(0.5, 5.0)));
            }
            "emc_decouple_esr" => {
                if let Some(&esr_max) = out.get("esr_max") {
                    if esr_max > 0.0 && esr_max < 1.0 {
                        caps.push((1e9, 1.0));
                    }
                }
            }
            _ => {}
        }
    }

    caps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (i, (f_res, max_dist)) in caps.iter().enumerate() {
        directives.emc.push(EmcDirective {
            component_ref: String::new(),
            target_ic_ref: String::new(),
            max_distance_mm: *max_dist,
            resonance_freq_hz: *f_res,
            cap_rank: i + 1,
        });
    }
}

fn extract_si(outputs: &[&HashMap<String, f64>], directives: &mut LayoutDirectives) {
    let mut z0 = 0.0_f64;
    let mut trace_width = 0.0_f64;

    for out in outputs {
        if let Some(&v) = out.get("z0") {
            z0 = v;
        }
        if let Some(&v) = out.get("r_trace") {
            trace_width = trace_width.max(v);
        }
    }

    if z0 > 0.0 {
        directives.signal_integrity.push(SignalIntegrityDirective {
            net_name: String::new(),
            min_trace_width_mm: trace_width.max(0.15),
            min_spacing_mm: 0.2,
            target_impedance_ohm: z0,
        });
    }
}

fn extract_clock(outputs: &[&HashMap<String, f64>], directives: &mut LayoutDirectives) {
    for out in outputs {
        if out.contains_key("c_load_required") {
            directives.clock.push(ClockDirective {
                crystal_ref: String::new(),
                load_cap_refs: vec![],
                symmetric_placement: true,
            });
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Lookup Helpers
// ---------------------------------------------------------------------------

impl LayoutDirectives {
    pub fn thermal_margin_for(&self, component_ref: &str, default: f64) -> f64 {
        self.thermal
            .iter()
            .filter(|d| d.component_ref.is_empty() || d.component_ref == component_ref)
            .map(|d| d.margin_mm)
            .fold(default, f64::max)
    }

    pub fn emc_distance_for_rank(&self, rank: usize) -> Option<f64> {
        self.emc
            .iter()
            .find(|d| d.cap_rank == rank)
            .map(|d| d.max_distance_mm)
    }

    pub fn emc_default_distance(&self) -> f64 {
        self.emc.first().map(|d| d.max_distance_mm).unwrap_or(1.5)
    }

    pub fn net_class_for(&self, net_name: &str) -> Option<&NetClassDirective> {
        self.net_classes.iter().find(|nc| nc.matches_net(net_name))
    }

    pub fn trace_width_for(&self, net_name: &str) -> f64 {
        for si in &self.signal_integrity {
            if si.net_name.is_empty() || si.net_name == net_name {
                return si.min_trace_width_mm;
            }
        }
        if let Some(nc) = self.net_class_for(net_name) {
            return nc.trace_width_mm;
        }
        let upper = net_name.to_uppercase();
        if upper == "GND" {
            0.8
        } else if upper.contains("V") || upper.contains("5V") || upper.contains("3V3") {
            0.5
        } else {
            0.25
        }
    }

    /// Calculate trace width for a target impedance using microstrip/stripline formulas.
    /// Returns width in mm. Uses iterative solve of the IPC-2141 approximation.
    pub fn impedance_controlled_width(
        target_z0: f64,
        dielectric_height_mm: f64,
        epsilon_r: f64,
        copper_thickness_mm: f64,
        is_stripline: bool,
    ) -> f64 {
        // Start with initial guess and iterate
        let mut w = 0.2; // mm, initial guess
        for _ in 0..50 {
            let z0 = if is_stripline {
                stripline_impedance(w, dielectric_height_mm, epsilon_r, copper_thickness_mm)
            } else {
                microstrip_impedance(w, dielectric_height_mm, epsilon_r, copper_thickness_mm)
            };
            let error = z0 - target_z0;
            if error.abs() < 0.5 {
                break;
            } // within 0.5 ohm
              // Increase width → lower impedance
            let step = if error > 0.0 { -0.01 } else { 0.01 };
            w = (w + step).clamp(0.1, 2.0);
        }
        (w * 100.0).round() / 100.0 // round to 0.01mm
    }
}

/// Microstrip impedance (IPC-2141 simplified).
/// w = trace width, h = dielectric height, er = dielectric constant, t = copper thickness (all mm).
fn microstrip_impedance(w: f64, h: f64, er: f64, t: f64) -> f64 {
    let w_eff = w + t * (1.0 + (1.0 / (2.0 * std::f64::consts::PI)) * ((w).ln().max(0.0))).max(0.0);
    let ratio = w_eff / h;
    let (c_eff, z0_air) = if ratio < 1.0 {
        let z0 = 60.0 / (0.5 * ((8.0 * h / w_eff).max(1.0)).ln()).sqrt();
        (
            0.67 * (er + 1.0) + 0.67 * (er - 1.0) / (1.0 + 10.0 / ratio).sqrt(),
            z0,
        )
    } else {
        let z0 = 120.0 * std::f64::consts::PI / (ratio + 1.393 + 0.667 * (ratio + 1.444).ln());
        (
            0.67 * (er + 1.0) + 0.67 * (er - 1.0) / (1.0 + 10.0 / ratio).sqrt(),
            z0,
        )
    };
    z0_air / c_eff.sqrt()
}

/// Symmetric stripline impedance (simplified).
fn stripline_impedance(w: f64, h: f64, er: f64, _t: f64) -> f64 {
    let b = 2.0 * h; // total dielectric thickness
    let ratio = w / b;
    let z0 = if ratio < 0.35 {
        60.0 / er.sqrt() * (b / (8.0 * w.max(0.01))).ln()
    } else {
        94.15 / er.sqrt() / (ratio * 0.08 + 0.75 * b / (w.max(0.01) * std::f64::consts::PI))
    };
    z0.max(10.0) // sanity clamp
}
