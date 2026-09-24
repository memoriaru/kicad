//! Footprint name sanity check against official KiCad library patterns (P0-1b).
//!
//! The toolchain synthesizes footprints and assigns `lib:name` ids. Historically
//! it fabricated names that don't exist in the official kicad-footprints repo
//! (`L_4020_1005Metric`, `SOT-323` without the `_SC-70` suffix), producing
//! ERC `footprint_link_issues` and broken BOM/fab association. This checker
//! flags names outside the known-good patterns.

use kicad_json5::ir::board::Board;

/// (library prefix, allowed footprint name patterns as `starts_with` lists).
/// A footprint under a known library whose name matches none of its patterns
/// is flagged. Libraries not in the table are flagged as unknown too.
const KNOWN: &[(&str, &[&str])] = &[
    (
        "Resistor_SMD:",
        &["R_0402_", "R_0603_", "R_0805_", "R_1206_", "R_2512_"],
    ),
    (
        "Capacitor_SMD:",
        &[
            "C_0402_", "C_0603_", "C_0805_", "C_1206_", "C_1203_", "CP_EIA-", "CP_Elec_",
        ],
    ),
    (
        "Inductor_SMD:",
        &[
            "L_0402_",
            "L_0603_",
            "L_0805_",
            "L_1210_",
            "L_1206_",
            "L_1812_",
            "L_Bourns-",
            "L_Wuerth-",
        ],
    ),
    (
        "Package_TO_SOT_SMD:",
        &[
            "SOT-23",
            "SOT-223-",
            "SOT-89",
            "SOT-323_SC-70",
            "SOT-353_SC-70",
            "SOT-363_SC-88",
            "SOT-23-5_L",
            "SOT-23-6_L",
            "TSOT-23-",
            "TO-252-",
            "TO-263-",
        ],
    ),
    (
        "Package_SO:",
        &["SOIC-8_", "SOIC-16_", "MSOP-", "TSSOP-", "VSSOP-"],
    ),
    ("Package_QFP:", &["TQFP-", "LQFP-"]),
    ("Package_DFN_QFN:", &["QFN-", "DFN-"]),
    ("Connector_PinHeader_2.54mm:", &["PinHeader_"]),
    ("Connector_PinSocket_2.54mm:", &["PinSocket_"]),
    ("Connector_USB:", &["USB_"]),
    ("Connector_JST:", &["JST_"]),
    (
        "Fuse:",
        &["Fuse_1206_", "Fuse_1812_", "Fuse_2920_", "Fuseholder"],
    ),
    ("Diode_SMD:", &["SOD-", "SMA_", "SMB_", "SMC_"]),
    ("LED_SMD:", &["LED_0603_", "LED_0805_", "LED_1206_"]),
    ("Button_Switch_SMD:", &["SW_"]),
    ("Crystal:", &["Crystal_SMD"]),
    ("Battery:", &["BatteryHolder"]),
    ("TestPoint:", &["TestPoint"]),
    ("MountingHole:", &["MountingHole"]),
];

pub fn check_footprint_names(board: &Board) -> Vec<String> {
    let mut warnings = Vec::new();
    for fp in &board.footprints {
        let lib_id = &fp.lib_id;
        let known = KNOWN.iter().find(|(lib, _)| lib_id.starts_with(lib));
        match known {
            None => {
                warnings.push(format!(
                    "{}: footprint '{}' 库不在已知官方库清单（确认名称或扩充清单）",
                    fp.reference, lib_id
                ));
            }
            Some((_, patterns)) => {
                let name = &lib_id[lib_id.find(':').map(|i| i + 1).unwrap_or(0)..];
                if !patterns.iter().any(|p| name.starts_with(p)) {
                    warnings.push(format!(
                        "{}: footprint '{}' 不匹配该库的官方命名（官方不存在此名字，BOM/贴片会解析失败）",
                        fp.reference, lib_id
                    ));
                }
            }
        }
    }
    warnings
}
