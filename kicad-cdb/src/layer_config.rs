//! PCB layer stackup configuration.
//!
//! Defines the copper layer arrangement (2-layer, 4-layer, etc.) and each layer's
//! purpose (signal routing, ground plane, power plane).

use kicad_json5::ir::board::LayerDef;

// ---------------------------------------------------------------------------
// Layer Purpose
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerPurpose {
    Signal,
    GroundPlane,
    PowerPlane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViaType {
    Through,
    Blind,
    Buried,
}

// ---------------------------------------------------------------------------
// Copper Layer Entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CopperLayerEntry {
    pub name: String,
    pub purpose: LayerPurpose,
}

// ---------------------------------------------------------------------------
// Layer Stackup Physical Parameters (for impedance control)
// ---------------------------------------------------------------------------

/// Physical parameters of a PCB layer stackup for impedance-controlled routing.
#[derive(Debug, Clone)]
pub struct StackupParams {
    /// Dielectric constant (εr) of the substrate (FR4 ≈ 4.5).
    pub epsilon_r: f64,
    /// Copper thickness in mm (1oz ≈ 0.035mm).
    pub copper_thickness_mm: f64,
    /// Dielectric height to nearest reference plane in mm, per copper layer index.
    /// Maps layer_index → height to the nearest GND/power plane.
    pub dielectric_height_mm: Vec<(usize, f64)>,
}

impl Default for StackupParams {
    fn default() -> Self {
        StackupParams {
            epsilon_r: 4.5,
            copper_thickness_mm: 0.035,
            dielectric_height_mm: vec![],
        }
    }
}

impl StackupParams {
    /// Standard FR4 4-layer stackup (0.2mm prepreg, 1.0mm core).
    pub fn fr4_four_layer() -> Self {
        StackupParams {
            epsilon_r: 4.5,
            copper_thickness_mm: 0.035,
            dielectric_height_mm: vec![
                (0, 0.20), // F.Cu → In1.Cu (GND plane): 0.2mm prepreg
                (3, 0.20), // B.Cu → In2.Cu (PWR plane): 0.2mm prepreg
            ],
        }
    }

    /// Standard FR4 6-layer stackup.
    pub fn fr4_six_layer() -> Self {
        StackupParams {
            epsilon_r: 4.5,
            copper_thickness_mm: 0.035,
            dielectric_height_mm: vec![
                (0, 0.15), // F.Cu → In1.Cu (GND): 0.15mm prepreg
                (2, 0.40), // In2.Cu → In1.Cu (GND): 0.40mm (symmetric stripline)
                (5, 0.15), // B.Cu → In4.Cu (PWR): 0.15mm prepreg
            ],
        }
    }

    /// Get dielectric height for a given layer index (signal layer).
    /// Returns 0.2mm as fallback if not specified.
    pub fn height_for_layer(&self, layer_idx: usize) -> f64 {
        self.dielectric_height_mm
            .iter()
            .find(|(idx, _)| *idx == layer_idx)
            .map(|(_, h)| *h)
            .unwrap_or(0.2)
    }

    /// Check if a layer is on an external surface (microstrip) vs internal (stripline).
    pub fn is_external_layer(&self, layer_idx: usize, total_layers: usize) -> bool {
        layer_idx == 0 || layer_idx == total_layers - 1
    }
}

// ---------------------------------------------------------------------------
// Board Layer Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BoardLayerConfig {
    /// Ordered copper layers: index 0 = top, last = bottom.
    pub copper_layers: Vec<CopperLayerEntry>,
    /// Target layer for GND zone (zone generation).
    pub ground_zone_layer: Option<String>,
    /// Target layer for PWR zone (zone generation).
    pub power_zone_layer: Option<String>,
    /// Whether blind/buried vias are enabled.
    pub blind_buried_vias: bool,
    /// Physical stackup parameters for impedance-controlled routing.
    pub stackup: StackupParams,
}

impl BoardLayerConfig {
    /// Standard 2-layer board: F.Cu (signal) + B.Cu (signal).
    pub fn two_layer() -> Self {
        BoardLayerConfig {
            copper_layers: vec![
                CopperLayerEntry {
                    name: "F.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "B.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
            ],
            ground_zone_layer: None,
            power_zone_layer: None,
            blind_buried_vias: false,
            stackup: StackupParams::default(),
        }
    }

    /// Standard 4-layer board: F.Cu(sig) + In1.Cu(GND) + In2.Cu(PWR) + B.Cu(sig).
    /// Inner layers are planes — A* only routes on F.Cu and B.Cu.
    pub fn four_layer() -> Self {
        BoardLayerConfig {
            copper_layers: vec![
                CopperLayerEntry {
                    name: "F.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "In1.Cu".into(),
                    purpose: LayerPurpose::GroundPlane,
                },
                CopperLayerEntry {
                    name: "In2.Cu".into(),
                    purpose: LayerPurpose::PowerPlane,
                },
                CopperLayerEntry {
                    name: "B.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
            ],
            ground_zone_layer: Some("In1.Cu".into()),
            power_zone_layer: Some("In2.Cu".into()),
            blind_buried_vias: false,
            stackup: StackupParams::fr4_four_layer(),
        }
    }

    /// 4-layer board with all layers as signal: F.Cu + In1.Cu + In2.Cu + B.Cu.
    /// Maximizes routing channels at the cost of no dedicated ground/power planes.
    pub fn four_signal_layer() -> Self {
        BoardLayerConfig {
            copper_layers: vec![
                CopperLayerEntry {
                    name: "F.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "In1.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "In2.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "B.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
            ],
            ground_zone_layer: None,
            power_zone_layer: None,
            blind_buried_vias: false,
            stackup: StackupParams::fr4_four_layer(),
        }
    }

    /// Standard 6-layer board: F.Cu(sig) + In1.Cu(GND) + In2.Cu(sig) + In3.Cu(PWR) + In4.Cu(PWR) + B.Cu(sig).
    /// Signal routing on F.Cu, In2.Cu, and B.Cu.
    pub fn six_layer() -> Self {
        BoardLayerConfig {
            copper_layers: vec![
                CopperLayerEntry {
                    name: "F.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "In1.Cu".into(),
                    purpose: LayerPurpose::GroundPlane,
                },
                CopperLayerEntry {
                    name: "In2.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
                CopperLayerEntry {
                    name: "In3.Cu".into(),
                    purpose: LayerPurpose::PowerPlane,
                },
                CopperLayerEntry {
                    name: "In4.Cu".into(),
                    purpose: LayerPurpose::PowerPlane,
                },
                CopperLayerEntry {
                    name: "B.Cu".into(),
                    purpose: LayerPurpose::Signal,
                },
            ],
            ground_zone_layer: Some("In1.Cu".into()),
            power_zone_layer: Some("In3.Cu".into()),
            blind_buried_vias: true,
            stackup: StackupParams::fr4_six_layer(),
        }
    }

    /// Create config from a layer count (2, 4, or 6).
    /// 兼容保留：非 2/4/6 静默降级 two_layer（主线 gen_pcb 历史行为）。
    /// 新代码请用 [`from_layer_count_exact`]（非法值显式报错）。
    pub fn from_layer_count(layers: usize) -> Self {
        match layers {
            6 => Self::six_layer(),
            4 => Self::four_layer(),
            _ => Self::two_layer(),
        }
    }

    /// M3：N 层全信号通用构造（F + In×(n-2) + B 全 Signal），支持任意 n≥2——
    /// 波前布线容量随层数线性扩展，不受 2/4/6 预设限制。
    /// Stackup 无对应预设时复用六层参数（厚度仅影响物理仿真，波前不用）。
    pub fn n_signal_layer(n: usize) -> Self {
        assert!(n >= 2, "至少 2 个铜层");
        let mut copper_layers = vec![CopperLayerEntry {
            name: "F.Cu".into(),
            purpose: LayerPurpose::Signal,
        }];
        for i in 1..(n - 1) {
            copper_layers.push(CopperLayerEntry {
                name: format!("In{i}.Cu"),
                purpose: LayerPurpose::Signal,
            });
        }
        copper_layers.push(CopperLayerEntry {
            name: "B.Cu".into(),
            purpose: LayerPurpose::Signal,
        });
        BoardLayerConfig {
            copper_layers,
            ground_zone_layer: None,
            power_zone_layer: None,
            blind_buried_vias: false,
            stackup: if n >= 6 {
                StackupParams::fr4_six_layer()
            } else {
                StackupParams::fr4_four_layer()
            },
        }
    }

    /// 精确版：仅接受已定义的层数（2/4/6），其余报错——替代静默降级。
    pub fn from_layer_count_exact(layers: usize) -> anyhow::Result<Self> {
        match layers {
            2 => Ok(Self::two_layer()),
            4 => Ok(Self::four_layer()),
            6 => Ok(Self::six_layer()),
            other => Err(anyhow::anyhow!(
                "未定义的层数 {other}（支持 2/4/6；任意层数全信号栈用 n_signal_layer）"
            )),
        }
    }

    /// Number of copper layers.
    pub fn layer_count(&self) -> usize {
        self.copper_layers.len()
    }

    /// Indices of signal layers (those that participate in A* routing).
    pub fn signal_layer_indices(&self) -> Vec<usize> {
        self.copper_layers
            .iter()
            .enumerate()
            .filter(|(_, e)| e.purpose == LayerPurpose::Signal)
            .map(|(i, _)| i)
            .collect()
    }

    /// GND zone target layer (In1.Cu for 4-layer, B.Cu fallback).
    pub fn ground_plane_layer(&self) -> Option<&str> {
        self.ground_zone_layer.as_deref().or({
            if self.copper_layers.len() >= 2 {
                Some("B.Cu")
            } else {
                None
            }
        })
    }

    /// PWR zone target layer (In2.Cu for 4-layer, F.Cu fallback).
    pub fn power_plane_layer(&self) -> Option<&str> {
        self.power_zone_layer.as_deref().or({
            if self.copper_layers.len() >= 2 {
                Some("F.Cu")
            } else {
                None
            }
        })
    }

    /// Map KiCad layer name → grid index (0-based).
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        self.copper_layers.iter().position(|e| e.name == name)
    }

    /// Map grid index → KiCad layer name.
    pub fn layer_name(&self, idx: usize) -> &str {
        self.copper_layers
            .get(idx)
            .map(|e| e.name.as_str())
            .unwrap_or("F.Cu")
    }

    /// First and last signal layer names (for thru-hole via layers list).
    pub fn via_layer_names(&self) -> (String, String) {
        let signals = self.signal_layer_indices();
        let first = self.layer_name(*signals.first().unwrap_or(&0)).to_string();
        let last = self.layer_name(*signals.last().unwrap_or(&0)).to_string();
        (first, last)
    }

    /// Generate full `Vec<LayerDef>` for Board IR, including non-copper layers.
    pub fn to_layer_defs(&self) -> Vec<LayerDef> {
        let mut defs = Vec::new();

        for (i, entry) in self.copper_layers.iter().enumerate() {
            let ordinal = match entry.name.as_str() {
                "F.Cu" => 0u32,
                "B.Cu" => 31u32,
                "In1.Cu" => 1u32,
                "In2.Cu" => 2u32,
                "In3.Cu" => 3u32,
                "In4.Cu" => 4u32,
                _ => i as u32,
            };
            defs.push(LayerDef {
                ordinal,
                name: entry.name.clone(),
                layer_type: "signal".into(),
            });
        }

        let non_copper: Vec<(u32, &str, &str)> = vec![
            (32, "B.Adhes", "user"),
            (33, "F.Adhes", "user"),
            (34, "B.Paste", "user"),
            (35, "F.Paste", "user"),
            (36, "B.SilkS", "user"),
            (37, "F.SilkS", "user"),
            (38, "B.Mask", "user"),
            (39, "F.Mask", "user"),
            (40, "Dwgs.User", "user"),
            (41, "Cmts.User", "user"),
            (42, "Eco1.User", "user"),
            (43, "Eco2.User", "user"),
            (44, "Edge.Cuts", "user"),
            (45, "Margin", "user"),
            (46, "B.CrtYd", "user"),
            (47, "F.CrtYd", "user"),
            (48, "B.Fab", "user"),
            (49, "F.Fab", "user"),
        ];

        for (ordinal, name, lt) in non_copper {
            defs.push(LayerDef {
                ordinal,
                name: name.into(),
                layer_type: lt.into(),
            });
        }

        defs
    }

    /// Compute impedance-controlled trace width for a signal on a given layer.
    /// Returns the trace width in mm that achieves `target_z0` ohms.
    pub fn impedance_width(&self, target_z0: f64, layer_idx: usize) -> f64 {
        let h = self.stackup.height_for_layer(layer_idx);
        let is_stripline = !self
            .stackup
            .is_external_layer(layer_idx, self.copper_layers.len());
        crate::layout_directives::LayoutDirectives::impedance_controlled_width(
            target_z0,
            h,
            self.stackup.epsilon_r,
            self.stackup.copper_thickness_mm,
            is_stripline,
        )
    }
}

impl Default for BoardLayerConfig {
    fn default() -> Self {
        Self::two_layer()
    }
}

impl BoardLayerConfig {
    /// Valid via layer pairs for this stackup.
    pub fn valid_via_pairs(&self) -> Vec<(usize, usize)> {
        let sig = self.signal_layer_indices();
        if sig.len() <= 2 || !self.blind_buried_vias {
            if let (Some(&f), Some(&l)) = (sig.first(), sig.last()) {
                return vec![(f, l)];
            }
            return vec![];
        }
        let mut pairs = vec![(*sig.first().unwrap(), *sig.last().unwrap())];
        for &inner in &sig[1..sig.len() - 1] {
            pairs.push((*sig.first().unwrap(), inner));
            pairs.push((*sig.last().unwrap(), inner));
        }
        pairs
    }

    pub fn via_type_for(&self, layer_a: usize, layer_b: usize) -> ViaType {
        let sig = self.signal_layer_indices();
        let first = *sig.first().unwrap();
        let last = *sig.last().unwrap();
        if (layer_a == first && layer_b == last) || (layer_a == last && layer_b == first) {
            ViaType::Through
        } else if layer_a == first || layer_b == first || layer_a == last || layer_b == last {
            ViaType::Blind
        } else {
            ViaType::Buried
        }
    }

    pub fn via_spec(&self, via_type: ViaType) -> (f64, f64) {
        match via_type {
            ViaType::Through => (0.6, 0.3),
            ViaType::Blind => (0.4, 0.2),
            ViaType::Buried => (0.4, 0.2),
        }
    }

    /// Layers traversed by a via connecting layer_a to layer_b (inclusive, signal only).
    pub fn via_traversed_layers(&self, layer_a: usize, layer_b: usize) -> Vec<usize> {
        let sig = self.signal_layer_indices();
        if !self.blind_buried_vias {
            return sig.clone();
        }
        let lo = layer_a.min(layer_b);
        let hi = layer_a.max(layer_b);
        sig.into_iter().filter(|&l| l >= lo && l <= hi).collect()
    }
}
