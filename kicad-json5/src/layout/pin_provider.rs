//! Pin position providers for both PCB and schematic layout.

use std::collections::HashMap;

use crate::ir::{Footprint, GraphicElement, PinGraphic, Symbol, SymbolInstance};

/// Trait for providing absolute pin positions for a placed component.
pub trait PinPositionProvider {
    fn pin_absolute_pos(&self, pin_id: &str) -> Option<(f64, f64)>;
}

/// PCB footprint pin provider — delegates to `Footprint::pad_absolute_pos`.
pub struct FootprintPinProvider<'a> {
    pub footprint: &'a Footprint,
}

impl<'a> PinPositionProvider for FootprintPinProvider<'a> {
    fn pin_absolute_pos(&self, pad_number: &str) -> Option<(f64, f64)> {
        self.footprint.pad_absolute_pos(pad_number)
    }
}

/// Schematic symbol pin provider — computes from Symbol PinGraphic + SymbolInstance position.
pub struct SymbolPinProvider<'a> {
    pub symbol: &'a Symbol,
    pub instance: &'a SymbolInstance,
    /// Cached pin graphics indexed by pin number and name
    pin_cache: HashMap<String, (f64, f64)>,
}

impl<'a> SymbolPinProvider<'a> {
    pub fn new(symbol: &'a Symbol, instance: &'a SymbolInstance) -> Self {
        let mut pin_cache = HashMap::new();

        let mut collect_from = |graphics: &[GraphicElement]| {
            for ge in graphics {
                if let GraphicElement::Pin(pg) = ge {
                    if let Some(pos) = Self::compute_pin_endpoint(pg, instance.position) {
                        pin_cache.entry(pg.number.clone()).or_insert(pos);
                        pin_cache.entry(pg.name.clone()).or_insert(pos);
                    }
                }
            }
        };

        collect_from(&symbol.graphics);

        // Also check unit graphics (multi-unit symbols)
        for unit in &symbol.units {
            collect_from(&unit.graphics);
        }

        Self {
            symbol,
            instance,
            pin_cache,
        }
    }

    /// Compute the absolute connection endpoint of a pin graphic.
    fn compute_pin_endpoint(pg: &PinGraphic, instance_pos: (f64, f64, f64)) -> Option<(f64, f64)> {
        let (px, py, prot) = pg.position;
        let length = pg.length;
        let pin_rad = prot.to_radians();

        // Pin endpoint in symbol-local coordinates
        let end_x = px + length * pin_rad.cos();
        let end_y = py + length * pin_rad.sin();

        // Transform to absolute coordinates using instance position + rotation
        let (ix, iy, irot) = instance_pos;
        let irad = irot.to_radians();
        let cos = irad.cos();
        let sin = irad.sin();
        let rx = end_x * cos - end_y * sin;
        let ry = end_x * sin + end_y * cos;

        Some((ix + rx, iy + ry))
    }
}

impl<'a> PinPositionProvider for SymbolPinProvider<'a> {
    fn pin_absolute_pos(&self, pin_id: &str) -> Option<(f64, f64)> {
        if let Some(&pos) = self.pin_cache.get(pin_id) {
            return Some(pos);
        }
        // Handle "ic.VIN" style references
        let name = if let Some(dot) = pin_id.find('.') {
            &pin_id[dot + 1..]
        } else {
            pin_id
        };
        self.pin_cache.get(name).copied()
    }
}

/// Build a HashMap of pin name → absolute position from any PinPositionProvider.
pub fn collect_pin_positions(
    provider: &dyn PinPositionProvider,
    pin_names: &[String],
) -> HashMap<String, (f64, f64)> {
    let mut map = HashMap::new();
    for name in pin_names {
        if let Some(pos) = provider.pin_absolute_pos(name) {
            map.insert(name.clone(), pos);
        }
    }
    map
}
