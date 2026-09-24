use std::collections::HashMap;

use crate::requirement::{
    IcPeripheralRule, IcType, InterfaceTemplate, InterfaceType, PeripheralTemplate,
    PowerRailTemplate,
};

pub fn mcu_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Mcu,
        peripherals: vec![
            PeripheralTemplate {
                role: "crystal".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "computed".into(),
                params: HashMap::from([
                    ("freq_mhz".into(), "8".into()),
                    ("load_cap_pf".into(), "20".into()),
                ]),
            },
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 4,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "decoupling_bulk".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "10uF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "reset".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "100nF".into(),
                params: HashMap::from([("function".into(), "reset_cap".into())]),
            },
            PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 1,
                value: "10k".into(),
                params: HashMap::from([("function".into(), "reset_pullup".into())]),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Swd,
            required_peripherals: vec![PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 1,
                value: "10k".into(),
                params: HashMap::new(),
            }],
            connector: "Pin_Header_Straight_1x04".into(),
        }],
        power_rails: vec![
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.1),
                decoupling_count: 4,
                name: "VDD".into(),
            },
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.02),
                decoupling_count: 2,
                name: "VDDA".into(),
            },
        ],
    }
}

pub fn opamp_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::OpAmp,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "feedback".into(),
                lib: "Device:R".into(),
                count: 2,
                value: "computed".into(),
                params: HashMap::new(),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Analog,
            required_peripherals: vec![PeripheralTemplate {
                role: "input_filter".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "computed".into(),
                params: HashMap::new(),
            }],
            connector: "Pin_Header_Straight_1x03".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(3.3),
            current: Some(0.01),
            decoupling_count: 2,
            name: "VCC".into(),
        }],
    }
}

pub fn adc_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Adc,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "vref".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "10uF".into(),
                params: HashMap::from([("function".into(), "vref_bypass".into())]),
            },
            PeripheralTemplate {
                role: "input_filter".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "1nF".into(),
                params: HashMap::new(),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::I2c,
            required_peripherals: vec![PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 2,
                value: "4.7k".into(),
                params: HashMap::new(),
            }],
            connector: "Pin_Header_Straight_1x04".into(),
        }],
        power_rails: vec![
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.01),
                decoupling_count: 2,
                name: "VDD".into(),
            },
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.001),
                decoupling_count: 1,
                name: "VREF".into(),
            },
        ],
    }
}

pub fn dac_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Dac,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "output_filter".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "100pF".into(),
                params: HashMap::new(),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Spi,
            required_peripherals: vec![],
            connector: "Pin_Header_Straight_1x05".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(3.3),
            current: Some(0.01),
            decoupling_count: 2,
            name: "VDD".into(),
        }],
    }
}

pub fn usb_uart_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::UsbUart,
        peripherals: vec![
            PeripheralTemplate {
                role: "crystal".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "computed".into(),
                params: HashMap::from([("freq_mhz".into(), "12".into())]),
            },
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 3,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 1,
                value: "1.5k".into(),
                params: HashMap::from([("function".into(), "usb_dp_pullup".into())]),
            },
        ],
        interfaces: vec![
            InterfaceTemplate {
                interface_type: InterfaceType::Usb,
                required_peripherals: vec![PeripheralTemplate {
                    role: "esd".into(),
                    lib: "Device:D_TVS".into(),
                    count: 1,
                    value: "computed".into(),
                    params: HashMap::new(),
                }],
                connector: "USB_B_Mini".into(),
            },
            InterfaceTemplate {
                interface_type: InterfaceType::Uart,
                required_peripherals: vec![],
                connector: "Pin_Header_Straight_1x04".into(),
            },
        ],
        power_rails: vec![
            PowerRailTemplate {
                voltage: Some(5.0),
                current: Some(0.1),
                decoupling_count: 1,
                name: "VBUS".into(),
            },
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.05),
                decoupling_count: 2,
                name: "VCC".into(),
            },
        ],
    }
}

pub fn can_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Can,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "terminator".into(),
                lib: "Device:R".into(),
                count: 1,
                value: "120".into(),
                params: HashMap::from([("function".into(), "can_term".into())]),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Can,
            required_peripherals: vec![PeripheralTemplate {
                role: "esd".into(),
                lib: "Device:D_TVS".into(),
                count: 1,
                value: "computed".into(),
                params: HashMap::new(),
            }],
            connector: "Connector_CAN".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(5.0),
            current: Some(0.07),
            decoupling_count: 2,
            name: "VCC".into(),
        }],
    }
}

pub fn sensor_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Sensor,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 2,
                value: "4.7k".into(),
                params: HashMap::new(),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::I2c,
            required_peripherals: vec![],
            connector: "Pin_Header_Straight_1x04".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(3.3),
            current: Some(0.01),
            decoupling_count: 2,
            name: "VDD".into(),
        }],
    }
}

pub fn driver_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Driver,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "gate_resistor".into(),
                lib: "Device:R".into(),
                count: 1,
                value: "10".into(),
                params: HashMap::new(),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Gpio,
            required_peripherals: vec![],
            connector: "Pin_Header_Straight_1x03".into(),
        }],
        power_rails: vec![
            PowerRailTemplate {
                voltage: Some(3.3),
                current: Some(0.01),
                decoupling_count: 1,
                name: "VDD".into(),
            },
            PowerRailTemplate {
                voltage: None,
                current: None,
                decoupling_count: 1,
                name: "VCC_POWER".into(),
            },
        ],
    }
}

pub fn power_ic_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Power,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 4,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "decoupling_bulk".into(),
                lib: "Device:C".into(),
                count: 1,
                value: "10uF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "feedback".into(),
                lib: "Device:R".into(),
                count: 2,
                value: "computed".into(),
                params: HashMap::from([("function".into(), "voltage_divider".into())]),
            },
        ],
        interfaces: vec![
            InterfaceTemplate {
                interface_type: InterfaceType::PowerIn,
                required_peripherals: vec![],
                connector: "Pin_Header_Straight_1x02".into(),
            },
            InterfaceTemplate {
                interface_type: InterfaceType::PowerOut,
                required_peripherals: vec![],
                connector: "Pin_Header_Straight_1x02".into(),
            },
        ],
        power_rails: vec![PowerRailTemplate {
            voltage: None,
            current: None,
            decoupling_count: 2,
            name: "VIN".into(),
        }],
    }
}

pub fn logic_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Logic,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "pull_up".into(),
                lib: "Device:R".into(),
                count: 4,
                value: "10k".into(),
                params: HashMap::from([("function".into(), "input_pullup".into())]),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Gpio,
            required_peripherals: vec![],
            connector: "Pin_Header_Straight_1x08".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(3.3),
            current: Some(0.02),
            decoupling_count: 2,
            name: "VCC".into(),
        }],
    }
}

pub fn clock_rule() -> IcPeripheralRule {
    IcPeripheralRule {
        ic_type: IcType::Clock,
        peripherals: vec![
            PeripheralTemplate {
                role: "decoupling_bank".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "100nF".into(),
                params: HashMap::new(),
            },
            PeripheralTemplate {
                role: "load_cap".into(),
                lib: "Device:C".into(),
                count: 2,
                value: "computed".into(),
                params: HashMap::from([("function".into(), "crystal_load".into())]),
            },
        ],
        interfaces: vec![InterfaceTemplate {
            interface_type: InterfaceType::Gpio,
            required_peripherals: vec![],
            connector: "Pin_Header_Straight_1x02".into(),
        }],
        power_rails: vec![PowerRailTemplate {
            voltage: Some(3.3),
            current: Some(0.02),
            decoupling_count: 2,
            name: "VDD".into(),
        }],
    }
}
