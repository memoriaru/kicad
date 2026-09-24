/// Format a float without unnecessary trailing zeros
pub fn fmt_f(v: f64) -> String {
    if v == v.trunc() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Standard IC body half-width (5.08mm = 200mil)
pub const BODY_HALF_WIDTH: f64 = 5.08;

#[cfg(test)]
pub mod test_helpers {
    use crate::model::{ElectricalType, SymbolPin};

    pub fn make_pin(number: &str, name: &str, etype: ElectricalType) -> SymbolPin {
        SymbolPin {
            number: number.to_string(),
            name: name.to_string(),
            electrical_type: etype,
            pin_group: None,
            alt_functions: None,
        }
    }

    pub fn make_spec(mpn: &str, pins: Vec<SymbolPin>) -> crate::model::SymbolSpec {
        crate::model::SymbolSpec {
            mpn: mpn.to_string(),
            lib_name: "custom".to_string(),
            reference_prefix: Some("U".to_string()),
            description: None,
            datasheet_url: None,
            footprint: None,
            manufacturer: None,
            package: None,
            pins,
        }
    }
}
