use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::composition::{Composition, ModuleInstance, NetDef};
use crate::power_tree::{PowerTreeRequest, RailSpec};

// ── Data Structures ──────────────────────────────────────────────

/// Structured design requirement
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DesignSpec {
    PowerBoard(PowerBoardSpec),
    IcBoard(IcBoardSpec),
    MixedBoard(MixedBoardSpec),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerBoardSpec {
    pub vin: f64,
    pub outputs: Vec<RailSpec>,
    #[serde(default)]
    pub isolated: bool,
    #[serde(default)]
    pub efficiency_min: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcBoardSpec {
    pub name: String,
    pub core_ics: Vec<IcRequest>,
    #[serde(default)]
    pub interfaces: Vec<InterfaceReq>,
    pub power_input_voltage: f64,
    #[serde(default)]
    pub constraints: Vec<DesignConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcRequest {
    pub ic_type: IcType,
    #[serde(default)]
    pub mpn: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IcType {
    Mcu,
    OpAmp,
    Adc,
    Dac,
    Sensor,
    Driver,
    UsbUart,
    Can,
    Power,
    Clock,
    Logic,
    Custom(String),
}

impl std::fmt::Display for IcType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IcType::Mcu => write!(f, "MCU"),
            IcType::OpAmp => write!(f, "OpAmp"),
            IcType::Adc => write!(f, "ADC"),
            IcType::Dac => write!(f, "DAC"),
            IcType::Sensor => write!(f, "Sensor"),
            IcType::Driver => write!(f, "Driver"),
            IcType::UsbUart => write!(f, "USB-UART"),
            IcType::Can => write!(f, "CAN"),
            IcType::Power => write!(f, "Power"),
            IcType::Clock => write!(f, "Clock"),
            IcType::Logic => write!(f, "Logic"),
            IcType::Custom(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceReq {
    pub interface_type: InterfaceType,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub parameters: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceType {
    Usb,
    Uart,
    Spi,
    I2c,
    Swd,
    Jtag,
    Can,
    Gpio,
    PowerIn,
    PowerOut,
    Analog,
    Custom(String),
}

impl std::fmt::Display for InterfaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterfaceType::Usb => write!(f, "USB"),
            InterfaceType::Uart => write!(f, "UART"),
            InterfaceType::Spi => write!(f, "SPI"),
            InterfaceType::I2c => write!(f, "I2C"),
            InterfaceType::Swd => write!(f, "SWD"),
            InterfaceType::Jtag => write!(f, "JTAG"),
            InterfaceType::Can => write!(f, "CAN"),
            InterfaceType::Gpio => write!(f, "GPIO"),
            InterfaceType::PowerIn => write!(f, "PowerIn"),
            InterfaceType::PowerOut => write!(f, "PowerOut"),
            InterfaceType::Analog => write!(f, "Analog"),
            InterfaceType::Custom(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignConstraint {
    pub name: String,
    pub value: f64,
    #[serde(default)]
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedBoardSpec {
    pub ic_board: IcBoardSpec,
    pub power: PowerBoardSpec,
}

// ── Validation ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ValidationMessage {
    pub level: ValidationLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationLevel {
    Error,
    Warning,
    Info,
}

pub fn validate_spec(spec: &DesignSpec) -> Vec<ValidationMessage> {
    let mut msgs = Vec::new();
    match spec {
        DesignSpec::PowerBoard(p) => validate_power_board(p, &mut msgs),
        DesignSpec::IcBoard(ic) => validate_ic_board(ic, &mut msgs),
        DesignSpec::MixedBoard(m) => {
            validate_ic_board(&m.ic_board, &mut msgs);
            validate_power_board(&m.power, &mut msgs);
        }
    }
    msgs
}

fn validate_power_board(spec: &PowerBoardSpec, msgs: &mut Vec<ValidationMessage>) {
    if spec.vin <= 0.0 {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Error,
            message: format!("输入电压必须为正数，当前 {}", spec.vin),
        });
    }
    if spec.outputs.is_empty() {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Error,
            message: "至少需要一个输出".into(),
        });
    }
    for out in &spec.outputs {
        if out.vout <= 0.0 {
            msgs.push(ValidationMessage {
                level: ValidationLevel::Error,
                message: format!("输出电压必须为正数: {}V", out.vout),
            });
        }
        if out.vout >= spec.vin && !spec.isolated {
            msgs.push(ValidationMessage {
                level: ValidationLevel::Warning,
                message: format!(
                    "输出 {}V >= 输入 {}V，非隔离拓扑可能无法实现",
                    out.vout, spec.vin
                ),
            });
        }
        if out.iout <= 0.0 {
            msgs.push(ValidationMessage {
                level: ValidationLevel::Warning,
                message: format!("输出电流应 > 0: {}A", out.iout),
            });
        }
    }
}

fn validate_ic_board(spec: &IcBoardSpec, msgs: &mut Vec<ValidationMessage>) {
    if spec.core_ics.is_empty() {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Error,
            message: "至少需要一个核心 IC".into(),
        });
    }
    if spec.power_input_voltage <= 0.0 {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Error,
            message: format!("供电电压必须为正数，当前 {}", spec.power_input_voltage),
        });
    }
    for ic in &spec.core_ics {
        match ic.ic_type {
            IcType::Mcu => validate_mcu_params(ic, msgs),
            IcType::OpAmp => validate_opamp_params(ic, msgs),
            IcType::Adc | IcType::Dac => validate_converter_params(ic, msgs),
            _ => {}
        }
    }
}

fn validate_mcu_params(ic: &IcRequest, msgs: &mut Vec<ValidationMessage>) {
    if !ic.parameters.contains_key("frequency") {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Warning,
            message: "MCU 未指定频率，将使用默认 8MHz".into(),
        });
    }
    if !ic.parameters.contains_key("vdd") {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Info,
            message: "MCU 未指定 VDD，默认 3.3V".into(),
        });
    }
}

fn validate_opamp_params(ic: &IcRequest, msgs: &mut Vec<ValidationMessage>) {
    if !ic.parameters.contains_key("gain") {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Warning,
            message: "运放未指定增益".into(),
        });
    }
}

fn validate_converter_params(ic: &IcRequest, msgs: &mut Vec<ValidationMessage>) {
    if !ic.parameters.contains_key("resolution") {
        msgs.push(ValidationMessage {
            level: ValidationLevel::Info,
            message: format!("{} 未指定分辨率，默认 12-bit", ic.ic_type),
        });
    }
}

// ── IC Knowledge Base ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PeripheralTemplate {
    pub role: String,
    pub lib: String,
    pub count: usize,
    pub value: String,
    pub params: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct InterfaceTemplate {
    pub interface_type: InterfaceType,
    pub required_peripherals: Vec<PeripheralTemplate>,
    pub connector: String,
}

#[derive(Debug, Clone)]
pub struct PowerRailTemplate {
    pub voltage: Option<f64>,
    pub current: Option<f64>,
    pub decoupling_count: usize,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct IcPeripheralRule {
    pub ic_type: IcType,
    pub peripherals: Vec<PeripheralTemplate>,
    pub interfaces: Vec<InterfaceTemplate>,
    pub power_rails: Vec<PowerRailTemplate>,
}

pub struct IcKnowledge {
    rules: HashMap<String, IcPeripheralRule>,
}

impl IcKnowledge {
    pub fn new() -> Self {
        let mut rules = HashMap::new();
        rules.insert("mcu".into(), crate::ic_knowledge::mcu_rule());
        rules.insert("op_amp".into(), crate::ic_knowledge::opamp_rule());
        rules.insert("adc".into(), crate::ic_knowledge::adc_rule());
        rules.insert("dac".into(), crate::ic_knowledge::dac_rule());
        rules.insert("usb_uart".into(), crate::ic_knowledge::usb_uart_rule());
        rules.insert("can".into(), crate::ic_knowledge::can_rule());
        rules.insert("sensor".into(), crate::ic_knowledge::sensor_rule());
        rules.insert("driver".into(), crate::ic_knowledge::driver_rule());
        rules.insert("power".into(), crate::ic_knowledge::power_ic_rule());
        rules.insert("logic".into(), crate::ic_knowledge::logic_rule());
        rules.insert("clock".into(), crate::ic_knowledge::clock_rule());
        Self { rules }
    }

    pub fn get_rule(&self, ic_type: &IcType) -> Option<&IcPeripheralRule> {
        let key = match ic_type {
            IcType::Mcu => "mcu",
            IcType::OpAmp => "op_amp",
            IcType::Adc => "adc",
            IcType::Dac => "dac",
            IcType::UsbUart => "usb_uart",
            IcType::Can => "can",
            IcType::Sensor => "sensor",
            IcType::Driver => "driver",
            IcType::Power => "power",
            IcType::Logic => "logic",
            IcType::Clock => "clock",
            _ => return None,
        };
        self.rules.get(key)
    }

    pub fn list_supported_types(&self) -> Vec<IcType> {
        vec![
            IcType::Mcu,
            IcType::OpAmp,
            IcType::Adc,
            IcType::Dac,
            IcType::UsbUart,
            IcType::Can,
            IcType::Sensor,
            IcType::Driver,
            IcType::Power,
            IcType::Logic,
            IcType::Clock,
        ]
    }
}

impl Default for IcKnowledge {
    fn default() -> Self {
        Self::new()
    }
}

// ── Spec → Composition Assembly ──────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AssemblyResult {
    pub composition: Composition,
    pub power_request: Option<PowerTreeRequest>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub peripheral_summary: Vec<PeripheralSummary>,
    #[serde(default)]
    pub total_component_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeripheralSummary {
    pub ic_type: String,
    pub role: String,
    pub count: usize,
    pub value: String,
}

pub fn spec_to_composition(spec: &DesignSpec, knowledge: &IcKnowledge) -> Result<AssemblyResult> {
    match spec {
        DesignSpec::PowerBoard(p) => assemble_power_board(p),
        DesignSpec::IcBoard(ic) => assemble_ic_board(ic, knowledge),
        DesignSpec::MixedBoard(m) => assemble_mixed_board(m, knowledge),
    }
}

fn assemble_power_board(spec: &PowerBoardSpec) -> Result<AssemblyResult> {
    let power_req = PowerTreeRequest {
        vin: spec.vin,
        outputs: spec.outputs.clone(),
        isolated: spec.isolated,
    };
    let comp = Composition {
        name: "power_board".into(),
        description: format!(
            "Power board: {}V input, {} outputs",
            spec.vin,
            spec.outputs.len()
        ),
        modules: vec![],
        global_nets: vec![
            NetDef {
                name: "VIN".into(),
                net_type: Some("power".into()),
            },
            NetDef {
                name: "GND".into(),
                net_type: Some("power".into()),
            },
        ],
    };
    Ok(AssemblyResult {
        composition: comp,
        power_request: Some(power_req),
        warnings: vec!["电源板模块将由 power_tree 自动填充".into()],
        peripheral_summary: vec![],
        total_component_count: 0,
    })
}

fn assemble_ic_board(spec: &IcBoardSpec, knowledge: &IcKnowledge) -> Result<AssemblyResult> {
    let mut modules = Vec::new();
    let mut global_nets = vec![
        NetDef {
            name: "VIN".into(),
            net_type: Some("power".into()),
        },
        NetDef {
            name: "GND".into(),
            net_type: Some("power".into()),
        },
    ];
    let mut warnings = Vec::new();
    let mut all_rails: Vec<RailSpec> = Vec::new();
    let mut peripheral_summary: Vec<PeripheralSummary> = Vec::new();
    let mut total_component_count: usize = 0;
    let mut idx = 0u32;

    for ic_req in &spec.core_ics {
        let rule = match knowledge.get_rule(&ic_req.ic_type) {
            Some(r) => r,
            None => {
                warnings.push(format!(
                    "IC 类型 {} 暂无知识规则，将仅放置核心 IC",
                    ic_req.ic_type
                ));
                let id = format!("ic_{}", idx);
                modules.push(ModuleInstance {
                    id,
                    template: ic_req.mpn.clone().unwrap_or_else(|| "generic_ic".into()),
                    template_type: "ic-core".into(),
                    params: ic_req
                        .parameters
                        .iter()
                        .map(|(k, &v)| (k.clone(), v))
                        .collect(),
                    nets: HashMap::new(),
                    y_offset: None,
                    topology_inputs: None,
                    computed_values: HashMap::new(),
                });
                idx += 1;
                continue;
            }
        };

        // Core IC module
        let ic_id = format!("ic_{}", idx);
        total_component_count += 1;
        let mut ic_params = ic_req
            .parameters
            .iter()
            .map(|(k, &v)| (k.clone(), v))
            .collect::<HashMap<_, _>>();
        if let Some(ref _mpn) = ic_req.mpn {
            ic_params.insert("_mpn".into(), 0.0); // mpn stored in template name
        }
        modules.push(ModuleInstance {
            id: ic_id.clone(),
            template: ic_req
                .mpn
                .clone()
                .unwrap_or_else(|| format!("generic_{}", ic_req.ic_type).to_lowercase()),
            template_type: "ic-core".into(),
            params: ic_params,
            nets: HashMap::new(),
            y_offset: None,
            topology_inputs: None,
            computed_values: HashMap::new(),
        });

        // Peripheral modules
        for peri in &rule.peripherals {
            let peri_id = format!("{}_{}", peri.role, idx);
            let mut peri_nets = HashMap::new();
            peri_nets.insert("parent_ic".into(), ic_id.clone());
            peri_nets.insert(
                "vcc".into(),
                format!(
                    "VCC_{}",
                    rule.power_rails
                        .first()
                        .map(|r| r.name.clone())
                        .unwrap_or_default()
                ),
            );
            peri_nets.insert("gnd".into(), "GND".into());

            peripheral_summary.push(PeripheralSummary {
                ic_type: format!("{}", ic_req.ic_type),
                role: peri.role.clone(),
                count: peri.count,
                value: peri.value.clone(),
            });
            total_component_count += peri.count;

            modules.push(ModuleInstance {
                id: peri_id,
                template: format!("{}_{}", ic_req.ic_type, peri.role),
                template_type: "ic-core".into(),
                params: peri.params.keys().map(|k| (k.clone(), 0.0)).collect(),
                nets: peri_nets,
                y_offset: None,
                topology_inputs: None,
                computed_values: HashMap::new(),
            });
        }

        // Collect power rails
        for rail in &rule.power_rails {
            let voltage = rail.voltage.unwrap_or(spec.power_input_voltage);
            let current = rail.current.unwrap_or(0.05);
            let net_name = format!("{}{}", rail.name, idx);
            global_nets.push(NetDef {
                name: net_name.clone(),
                net_type: Some("power".into()),
            });
            all_rails.push(RailSpec {
                vout: voltage,
                iout: current,
                name: Some(net_name),
            });
        }

        idx += 1;
    }

    // Interface modules
    for (i, iface) in spec.interfaces.iter().enumerate() {
        let iface_id = format!("iface_{}", i);
        modules.push(ModuleInstance {
            id: iface_id,
            template: format!("connector_{}", iface.interface_type).to_lowercase(),
            template_type: "ic-core".into(),
            params: iface
                .parameters
                .iter()
                .map(|(k, &v)| (k.clone(), v))
                .collect(),
            nets: HashMap::new(),
            y_offset: None,
            topology_inputs: None,
            computed_values: HashMap::new(),
        });
    }

    // Build power request if rails were collected
    let power_request = if all_rails.is_empty() {
        // No rails derived from IC knowledge — provide a default
        warnings.push("未推导出供电需求，使用默认 3.3V".into());
        Some(PowerTreeRequest {
            vin: spec.power_input_voltage,
            outputs: vec![RailSpec {
                vout: 3.3,
                iout: 0.5,
                name: Some("VDD".into()),
            }],
            isolated: false,
        })
    } else {
        // Deduplicate rails by voltage (merge currents)
        let mut merged: Vec<RailSpec> = Vec::new();
        for rail in all_rails {
            let v_rounded = (rail.vout * 10.0).round() / 10.0;
            if let Some(existing) = merged
                .iter_mut()
                .find(|r| (r.vout * 10.0).round() / 10.0 == v_rounded && r.name == rail.name)
            {
                existing.iout += rail.iout;
            } else {
                merged.push(rail);
            }
        }
        let outputs = merged;
        Some(PowerTreeRequest {
            vin: spec.power_input_voltage,
            outputs,
            isolated: false,
        })
    };

    let comp = Composition {
        name: spec.name.clone(),
        description: format!(
            "IC board: {} core ICs, {} interfaces",
            spec.core_ics.len(),
            spec.interfaces.len()
        ),
        modules,
        global_nets,
    };

    Ok(AssemblyResult {
        composition: comp,
        power_request,
        warnings,
        peripheral_summary,
        total_component_count,
    })
}

fn assemble_mixed_board(spec: &MixedBoardSpec, knowledge: &IcKnowledge) -> Result<AssemblyResult> {
    let ic_result = assemble_ic_board(&spec.ic_board, knowledge)?;
    let mut comp = ic_result.composition;
    let mut warnings = ic_result.warnings;

    // Merge power board outputs into the IC board's power request
    let mut power_req = ic_result.power_request.unwrap_or(PowerTreeRequest {
        vin: spec.power.vin,
        outputs: vec![],
        isolated: false,
    });
    power_req.outputs.extend(spec.power.outputs.clone());
    if power_req.vin != spec.power.vin && power_req.vin == 0.0 {
        power_req.vin = spec.power.vin;
    }

    comp.description = format!(
        "Mixed board: {} ICs + {} power outputs",
        spec.ic_board.core_ics.len(),
        spec.power.outputs.len()
    );

    warnings.push("混合板：电源树需求已合并".into());

    Ok(AssemblyResult {
        composition: comp,
        power_request: Some(power_req),
        warnings,
        peripheral_summary: ic_result.peripheral_summary,
        total_component_count: ic_result.total_component_count,
    })
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_power_board_ok() {
        let spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 12.0,
            outputs: vec![
                RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: Some("5V".into()),
                },
                RailSpec {
                    vout: 3.3,
                    iout: 1.0,
                    name: Some("3V3".into()),
                },
            ],
            isolated: false,
            efficiency_min: None,
        });
        let msgs = validate_spec(&spec);
        let errors: Vec<_> = msgs
            .iter()
            .filter(|m| m.level == ValidationLevel::Error)
            .collect();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_validate_power_board_vout_gt_vin() {
        let spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 5.0,
            outputs: vec![RailSpec {
                vout: 12.0,
                iout: 1.0,
                name: None,
            }],
            isolated: false,
            efficiency_min: None,
        });
        let msgs = validate_spec(&spec);
        assert!(msgs
            .iter()
            .any(|m| m.level == ValidationLevel::Warning && m.message.contains("非隔离")));
    }

    #[test]
    fn test_validate_ic_board_no_ics() {
        let spec = DesignSpec::IcBoard(IcBoardSpec {
            name: "test".into(),
            core_ics: vec![],
            interfaces: vec![],
            power_input_voltage: 5.0,
            constraints: vec![],
        });
        let msgs = validate_spec(&spec);
        assert!(msgs
            .iter()
            .any(|m| m.level == ValidationLevel::Error && m.message.contains("核心 IC")));
    }

    #[test]
    fn test_validate_mcu_no_frequency() {
        let spec = DesignSpec::IcBoard(IcBoardSpec {
            name: "test".into(),
            core_ics: vec![IcRequest {
                ic_type: IcType::Mcu,
                mpn: Some("STM32F103".into()),
                parameters: HashMap::new(),
            }],
            interfaces: vec![],
            power_input_voltage: 5.0,
            constraints: vec![],
        });
        let msgs = validate_spec(&spec);
        assert!(msgs.iter().any(|m| m.message.contains("频率")));
    }

    #[test]
    fn test_ic_knowledge_mcu_peripherals() {
        let kb = IcKnowledge::new();
        let rule = kb.get_rule(&IcType::Mcu).unwrap();
        assert!(
            rule.peripherals.len() >= 3,
            "MCU should have >= 3 peripheral types"
        );
        assert!(
            !rule.power_rails.is_empty(),
            "MCU should have >= 1 power rail"
        );
        assert_eq!(rule.power_rails[0].voltage, Some(3.3));
    }

    #[test]
    fn test_ic_knowledge_all_types() {
        let kb = IcKnowledge::new();
        let types = kb.list_supported_types();
        assert!(types.len() >= 11);
        for t in &types {
            assert!(kb.get_rule(t).is_some(), "Missing rule for {:?}", t);
        }
    }

    #[test]
    fn test_power_ic_knowledge() {
        let kb = IcKnowledge::new();
        let rule = kb.get_rule(&IcType::Power).unwrap();
        assert!(rule.peripherals.len() >= 2);
        assert!(rule
            .interfaces
            .iter()
            .any(|i| matches!(i.interface_type, InterfaceType::PowerIn)));
        assert!(rule
            .interfaces
            .iter()
            .any(|i| matches!(i.interface_type, InterfaceType::PowerOut)));
    }

    #[test]
    fn test_logic_ic_knowledge() {
        let kb = IcKnowledge::new();
        let rule = kb.get_rule(&IcType::Logic).unwrap();
        assert!(rule.peripherals.len() >= 2);
        assert_eq!(rule.power_rails[0].voltage, Some(3.3));
    }

    #[test]
    fn test_clock_ic_knowledge() {
        let kb = IcKnowledge::new();
        let rule = kb.get_rule(&IcType::Clock).unwrap();
        assert!(rule.peripherals.iter().any(|p| p.role == "load_cap"));
        assert!(rule.peripherals.iter().any(|p| p.role == "decoupling_bank"));
    }

    #[test]
    fn test_spec_to_composition_mcu() {
        let kb = IcKnowledge::new();
        let spec = DesignSpec::IcBoard(IcBoardSpec {
            name: "stm32_min".into(),
            core_ics: vec![IcRequest {
                ic_type: IcType::Mcu,
                mpn: Some("STM32F103C8T6".into()),
                parameters: HashMap::from([("frequency".into(), 72.0), ("vdd".into(), 3.3)]),
            }],
            interfaces: vec![InterfaceReq {
                interface_type: InterfaceType::Swd,
                role: "debug".into(),
                parameters: HashMap::new(),
            }],
            power_input_voltage: 5.0,
            constraints: vec![],
        });
        let result = spec_to_composition(&spec, &kb).unwrap();
        assert!(
            result.composition.modules.len() >= 2,
            "Should have IC + peripherals"
        );
        assert!(result.power_request.is_some());
        let pr = result.power_request.unwrap();
        assert_eq!(pr.vin, 5.0);
        assert!(!pr.outputs.is_empty());
    }

    #[test]
    fn test_spec_to_composition_power_board() {
        let kb = IcKnowledge::new();
        let spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: 12.0,
            outputs: vec![RailSpec {
                vout: 5.0,
                iout: 2.0,
                name: Some("5V".into()),
            }],
            isolated: false,
            efficiency_min: None,
        });
        let result = spec_to_composition(&spec, &kb).unwrap();
        assert!(result.power_request.is_some());
        assert_eq!(result.power_request.unwrap().outputs.len(), 1);
    }

    #[test]
    fn test_spec_to_composition_unsupported_ic() {
        let kb = IcKnowledge::new();
        let spec = DesignSpec::IcBoard(IcBoardSpec {
            name: "custom".into(),
            core_ics: vec![IcRequest {
                ic_type: IcType::Custom("FPGA".into()),
                mpn: None,
                parameters: HashMap::new(),
            }],
            interfaces: vec![],
            power_input_voltage: 5.0,
            constraints: vec![],
        });
        let result = spec_to_composition(&spec, &kb).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("暂无知识规则")));
    }

    #[test]
    fn test_mixed_board_merges_power() {
        let kb = IcKnowledge::new();
        let spec = DesignSpec::MixedBoard(MixedBoardSpec {
            ic_board: IcBoardSpec {
                name: "mcu_with_power".into(),
                core_ics: vec![IcRequest {
                    ic_type: IcType::Mcu,
                    mpn: Some("STM32F103".into()),
                    parameters: HashMap::from([("frequency".into(), 72.0)]),
                }],
                interfaces: vec![],
                power_input_voltage: 12.0,
                constraints: vec![],
            },
            power: PowerBoardSpec {
                vin: 12.0,
                outputs: vec![RailSpec {
                    vout: 5.0,
                    iout: 2.0,
                    name: Some("5V_EXT".into()),
                }],
                isolated: false,
                efficiency_min: None,
            },
        });
        let result = spec_to_composition(&spec, &kb).unwrap();
        let pr = result.power_request.unwrap();
        assert!(
            pr.outputs.len() >= 2,
            "Mixed board should merge IC + power outputs"
        );
    }

    #[test]
    fn test_validation_negative_voltage() {
        let spec = DesignSpec::PowerBoard(PowerBoardSpec {
            vin: -5.0,
            outputs: vec![],
            isolated: false,
            efficiency_min: None,
        });
        let msgs = validate_spec(&spec);
        assert!(msgs.iter().any(|m| m.level == ValidationLevel::Error));
    }
}
