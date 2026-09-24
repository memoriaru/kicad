use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::ComponentDb;

/// Single step execution record
#[derive(Debug, Serialize, serde::Deserialize)]
pub struct DesignStep {
    pub seq: usize,
    pub rule_name: String,
    pub description: String,
    pub inputs: HashMap<String, f64>,
    pub formula: String,
    pub outputs: HashMap<String, f64>,
    pub check_expr: String,
    pub passed: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

/// Complete design decision log
#[derive(Debug, Serialize, serde::Deserialize)]
pub struct DesignLog {
    pub pipeline_name: String,
    pub user_inputs: HashMap<String, f64>,
    pub steps: Vec<DesignStep>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// A single step in a pipeline definition
pub struct PipelineStep {
    pub rule_name: String,
    /// Optional condition expression (e.g. "iout > 2"). Step is skipped if false.
    pub condition: Option<String>,
}

/// A pipeline definition
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub user_inputs: Vec<String>,
    pub steps: Vec<PipelineStep>,
}

/// Get all built-in pipelines
pub fn builtin_pipelines() -> Vec<Pipeline> {
    vec![
        Pipeline {
            name: "buck".into(),
            description: "Buck Converter Design".into(),
            user_inputs: vec![
                "vin".into(),
                "vout".into(),
                "iout".into(),
                "fsw".into(),
                "ripple_ratio".into(),
                "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "buck_duty_cycle".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "buck_inductor_selection".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "buck_inductor_ripple".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "buck_output_capacitor".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "buck_input_capacitor".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "buck_catch_diode".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_dissipation".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "feedback_resistor_divider".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "boost".into(),
            description: "Boost Converter Design".into(),
            user_inputs: vec![
                "vin".into(),
                "vout".into(),
                "iout".into(),
                "fsw".into(),
                "ripple_ratio".into(),
                "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "boost_duty_cycle".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "boost_inductor_selection".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "boost_inductor_ripple".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "boost_output_capacitor".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "boost_switch_voltage".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "boost_diode_voltage".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_dissipation".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "feedback_resistor_divider".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "ldo".into(),
            description: "LDO Regulator Design".into(),
            user_inputs: vec![
                "vin".into(),
                "vout".into(),
                "iout".into(),
                "vdropout_max".into(),
                "p_max".into(),
                "eff_min".into(),
                "ripple_v".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "ldo_dropout_check".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "ldo_power_dissipation".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "ldo_efficiency".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "ldo_output_cap".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "led".into(),
            description: "LED Current Limiting Design".into(),
            user_inputs: vec!["vin".into(), "vf".into(), "i_led".into(), "r_power".into()],
            steps: vec![PipelineStep {
                rule_name: "led_current_resistor".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "si_high_speed".into(),
            description: "Signal Integrity — High-Speed Trace Analysis".into(),
            user_inputs: vec![
                "epsilon_r".into(),
                "h".into(),
                "w".into(),
                "z_min".into(),
                "z_max".into(),
                "rho".into(),
                "L".into(),
                "t".into(),
                "r_max".into(),
                "epsilon_eff".into(),
                "c".into(),
                "t_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "si_microstrip_impedance".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "si_trace_resistance".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "si_propagation_delay".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "emc_power_decouple".into(),
            description: "EMC — Power Supply Decoupling Design".into(),
            user_inputs: vec![
                "i_transient".into(),
                "dt".into(),
                "dv_target".into(),
                "i_ripple".into(),
                "L_esl".into(),
                "C_value".into(),
                "C_esr".into(),
                "epsilon_r".into(),
                "area".into(),
                "dielectric_thickness".into(),
                "c_min_required".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "emc_decouple_capacitance".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "emc_decouple_esr".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "emc_decouple_resonance".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "emc_power_plane_capacitance".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "timing_digital".into(),
            description: "Timing — Digital Interface Analysis".into(),
            user_inputs: vec![
                "t_clk_period".into(),
                "t_ckq".into(),
                "t_prop".into(),
                "t_setup".into(),
                "t_prop_min".into(),
                "t_hold".into(),
                "t_margin_min".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "timing_setup_margin".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "timing_hold_margin".into(),
                    condition: None,
                },
            ],
        },
        // ── B1: Interface Protocol Pipelines ──
        Pipeline {
            name: "interface_i2c".into(),
            description: "Interface — I2C Bus Design".into(),
            user_inputs: vec![
                "vcc".into(),
                "v_ol".into(),
                "i_sink".into(),
                "i_sink_max".into(),
                "v_ih_min".into(),
                "n_devices".into(),
                "i_leak".into(),
                "r_pullup".into(),
                "c_wire".into(),
                "c_pin".into(),
                "c_bus_max".into(),
                "t_rise_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "interface_i2c_pullup".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "interface_i2c_rise_time".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "interface_i2c_bus_capacitance".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "interface_can".into(),
            description: "Interface — CAN Bus Design".into(),
            user_inputs: vec![
                "z_cable".into(),
                "v_dominant".into(),
                "r_driver".into(),
                "c_wire_per_m".into(),
                "l_bus".into(),
                "n_nodes".into(),
                "c_node".into(),
                "t_prop_segment".into(),
                "c_bus_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "interface_can_termination".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "interface_can_bus_loading".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "interface_spi".into(),
            description: "Interface — SPI Bus Design".into(),
            user_inputs: vec![
                "t_prop".into(),
                "t_setup".into(),
                "f_clk".into(),
                "margin_min".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "interface_spi_clock_margin".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "interface_uart".into(),
            description: "Interface — UART Design".into(),
            user_inputs: vec!["f_clk".into(), "baud_target".into(), "error_max".into()],
            steps: vec![PipelineStep {
                rule_name: "interface_uart_baud_margin".into(),
                condition: None,
            }],
        },
        // ── B2: Protection Circuit Pipelines ──
        Pipeline {
            name: "protection_tvs".into(),
            description: "Protection — TVS Diode Selection".into(),
            user_inputs: vec![
                "v_wm".into(),
                "clamp_ratio".into(),
                "v_supply".into(),
                "i_pp".into(),
                "v_max_withstand".into(),
                "t_pulse".into(),
                "t_period".into(),
                "p_dissipation_max".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "protection_tvs_selection".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "protection_esd".into(),
            description: "Protection — ESD Clamp Analysis".into(),
            user_inputs: vec![
                "v_clamp".into(),
                "i_esd".into(),
                "r_dynamic".into(),
                "v_max_withstand".into(),
                "margin_min".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "protection_esd_clamp".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "protection_ovp_ocp".into(),
            description: "Protection — Over-Voltage & Over-Current".into(),
            user_inputs: vec![
                "v_trip".into(),
                "v_ref".into(),
                "i_div".into(),
                "v_max_input".into(),
                "v_nominal".into(),
                "r_hyst".into(),
                "i_trip".into(),
                "v_sense_max".into(),
                "i_max".into(),
                "p_resistor_max".into(),
                "v_adc_min".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "protection_overvoltage_threshold".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "protection_overcurrent_sense".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "protection_reverse".into(),
            description: "Protection — Reverse Polarity".into(),
            user_inputs: vec![
                "v_supply".into(),
                "v_diode".into(),
                "i_load".into(),
                "r_ds_on".into(),
                "v_gs_th".into(),
                "v_drop_max".into(),
                "p_dissipation_max".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "protection_reverse_polarity".into(),
                condition: None,
            }],
        },
        // ── B3: Crystal/Clock Pipelines ──
        Pipeline {
            name: "clock_crystal".into(),
            description: "Clock — Crystal Oscillator Design".into(),
            user_inputs: vec![
                "c1".into(),
                "c2".into(),
                "c_stray".into(),
                "c_load_target".into(),
                "c_load_min".into(),
                "c_load_max".into(),
                "f_xtal".into(),
                "esr".into(),
                "i_rms".into(),
                "p_max".into(),
                "stability_temp".into(),
                "stability_initial".into(),
                "stability_aging".into(),
                "ppm_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "clock_load_capacitance".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "clock_drive_level".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "clock_frequency_stability".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "clock_pll".into(),
            description: "Clock — PLL Loop Filter Design".into(),
            user_inputs: vec![
                "i_cp".into(),
                "n_div".into(),
                "c_loop".into(),
                "r_loop".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "clock_pll_bandwidth".into(),
                condition: None,
            }],
        },
        // ── B4: Analog Circuit Pipelines ──
        Pipeline {
            name: "analog_opamp_noninv".into(),
            description: "Analog — Non-Inverting Op-Amp".into(),
            user_inputs: vec![
                "r_f".into(),
                "r_g".into(),
                "v_in_max".into(),
                "v_supply".into(),
                "v_headroom".into(),
                "gain_max".into(),
                "v_cm_max".into(),
                "gbw".into(),
                "f_signal".into(),
                "v_out_peak".into(),
                "slew_rate".into(),
                "i_bias".into(),
                "v_os".into(),
                "v_signal_min".into(),
                "error_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "analog_opamp_gain".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "analog_opamp_bandwidth".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "analog_opamp_bias".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "analog_opamp_inv".into(),
            description: "Analog — Inverting Op-Amp".into(),
            user_inputs: vec![
                "r_f".into(),
                "r_in".into(),
                "v_in_max".into(),
                "v_supply".into(),
                "v_headroom".into(),
                "gain_max".into(),
                "gbw".into(),
                "f_signal".into(),
                "v_out_peak".into(),
                "slew_rate".into(),
                "i_bias".into(),
                "v_os".into(),
                "v_signal_min".into(),
                "error_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "analog_opamp_inverting_gain".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "analog_opamp_bandwidth".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "analog_opamp_bias".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "analog_filter".into(),
            description: "Analog — Active/Passive Filter Design".into(),
            user_inputs: vec![
                "r".into(),
                "c".into(),
                "f_signal".into(),
                "f_pass_max".into(),
                "f_stop_min".into(),
                "r1".into(),
                "r2".into(),
                "c1".into(),
                "c2".into(),
                "gain".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "analog_filter_rc_lowpass".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "analog_filter_sallen_key".into(),
                    condition: None,
                },
            ],
        },
        // ── B5: Battery Management Pipelines ──
        Pipeline {
            name: "battery_charge".into(),
            description: "Battery — Charge Profile Design".into(),
            user_inputs: vec![
                "v_sense".into(),
                "i_charge".into(),
                "capacity".into(),
                "soc_start".into(),
                "c_rate_max".into(),
                "p_resistor_max".into(),
                "cutoff_c_rate".into(),
                "i_cutoff_min".into(),
                "t_charge_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "battery_charge_cc".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "battery_charge_cv".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "battery_runtime".into(),
            description: "Battery — Runtime & Discharge Analysis".into(),
            user_inputs: vec![
                "capacity".into(),
                "dod".into(),
                "v_nominal".into(),
                "p_load".into(),
                "i_discharge_max".into(),
                "t_runtime_min".into(),
            ],
            steps: vec![PipelineStep {
                rule_name: "battery_discharge_runtime".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "battery_protection".into(),
            description: "Battery — Protection Thresholds".into(),
            user_inputs: vec![
                "n_cells".into(),
                "v_cell_full".into(),
                "v_cell_empty".into(),
                "v_margin".into(),
                "v_abs_max".into(),
                "v_abs_min".into(),
                "t_charge_max".into(),
                "t_charge_min".into(),
                "t_discharge_max".into(),
                "t_discharge_min".into(),
                "t_margin".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "battery_protection_ocv".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "battery_protection_temp".into(),
                    condition: None,
                },
            ],
        },
        // ── Thermal Design Pipelines ──────────────────────────────────
        Pipeline {
            name: "thermal_power".into(),
            description: "Calculate power dissipation for a voltage regulator".into(),
            user_inputs: vec!["vout".into(), "iout".into(), "efficiency".into()],
            steps: vec![PipelineStep {
                rule_name: "thermal_power_dissipation".into(),
                condition: None,
            }],
        },
        Pipeline {
            name: "thermal_analysis".into(),
            description: "Full thermal analysis: dissipation → junction temp → heatsink check"
                .into(),
            user_inputs: vec![
                "vout".into(),
                "iout".into(),
                "efficiency".into(),
                "ta".into(),
                "theta_jc".into(),
                "theta_cs".into(),
                "theta_sa".into(),
                "theta_ja_pcb".into(),
                "tj_max".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "thermal_power_dissipation".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_junction_temp".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_heatsink_required".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "thermal_copper".into(),
            description: "Copper area and thermal via calculation for PCB heat dissipation".into(),
            user_inputs: vec![
                "p_diss".into(),
                "ta".into(),
                "tj_target".into(),
                "h_conv".into(),
                "board_thickness".into(),
                "via_radius".into(),
                "cu_thickness".into(),
                "via_count".into(),
                "k_copper".into(),
                "theta_ja_pcb".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "thermal_copper_area".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_via_resistance".into(),
                    condition: None,
                },
            ],
        },
        Pipeline {
            name: "thermal_full".into(),
            description:
                "Complete thermal design check: dissipation, temperature, copper, vias, heatsink"
                    .into(),
            user_inputs: vec![
                "vout".into(),
                "iout".into(),
                "efficiency".into(),
                "ta".into(),
                "theta_jc".into(),
                "theta_cs".into(),
                "theta_sa".into(),
                "theta_ja_pcb".into(),
                "tj_max".into(),
                "tj_target".into(),
                "h_conv".into(),
                "board_thickness".into(),
                "via_radius".into(),
                "cu_thickness".into(),
                "via_count".into(),
                "k_copper".into(),
            ],
            steps: vec![
                PipelineStep {
                    rule_name: "thermal_power_dissipation".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_junction_temp".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_heatsink_required".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_copper_area".into(),
                    condition: None,
                },
                PipelineStep {
                    rule_name: "thermal_via_resistance".into(),
                    condition: None,
                },
            ],
        },
    ]
}

/// Get a built-in pipeline by name
pub fn get_builtin_pipeline(name: &str) -> Option<Pipeline> {
    builtin_pipelines().into_iter().find(|p| p.name == name)
}

/// Run a pipeline and produce a design decision log
pub fn run_pipeline(
    db: &ComponentDb,
    pipeline: &Pipeline,
    user_inputs: &HashMap<String, f64>,
) -> Result<DesignLog> {
    let rules = db.get_all_design_rules()?;
    let mut ctx: HashMap<String, f64> = user_inputs.clone();

    let mut steps = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for (i, step_def) in pipeline.steps.iter().enumerate() {
        // Check step-level condition gate
        if let Some(cond) = &step_def.condition {
            let mut eval = crate::rules::EvalContext::new();
            for (name, &val) in &ctx {
                eval.set(name, val);
            }
            match eval.eval_check(cond) {
                Ok(true) => {}
                Ok(false) => {
                    skipped += 1;
                    steps.push(DesignStep {
                        seq: i + 1,
                        rule_name: step_def.rule_name.clone(),
                        description: String::new(),
                        inputs: HashMap::new(),
                        formula: String::new(),
                        outputs: HashMap::new(),
                        check_expr: String::new(),
                        passed: false,
                        skipped: true,
                        skip_reason: Some(format!("Step condition '{}' not met", cond)),
                    });
                    continue;
                }
                Err(_) => {
                    skipped += 1;
                    steps.push(DesignStep {
                        seq: i + 1,
                        rule_name: step_def.rule_name.clone(),
                        description: String::new(),
                        inputs: HashMap::new(),
                        formula: String::new(),
                        outputs: HashMap::new(),
                        check_expr: String::new(),
                        passed: false,
                        skipped: true,
                        skip_reason: Some(format!(
                            "Step condition '{}' could not be evaluated",
                            cond
                        )),
                    });
                    continue;
                }
            }
        }

        let rule = match rules.iter().find(|r| r.name == step_def.rule_name) {
            Some(r) => r.clone(),
            None => {
                skipped += 1;
                steps.push(DesignStep {
                    seq: i + 1,
                    rule_name: step_def.rule_name.clone(),
                    description: String::new(),
                    inputs: HashMap::new(),
                    formula: String::new(),
                    outputs: HashMap::new(),
                    check_expr: String::new(),
                    passed: false,
                    skipped: true,
                    skip_reason: Some(format!(
                        "Rule '{}' not found in database",
                        step_def.rule_name
                    )),
                });
                continue;
            }
        };

        // Extract parameters declared by this rule from shared context
        let param_names: Vec<String> = rule
            .parameters
            .as_ref()
            .and_then(|p| serde_json::from_str::<Vec<String>>(p).ok())
            .unwrap_or_default();

        let mut step_inputs = serde_json::Map::new();
        for pname in &param_names {
            if let Some(&val) = ctx.get(pname.as_str()) {
                step_inputs.insert(pname.clone(), serde_json::Value::from(val));
            }
        }

        // apply_rule now validates input completeness and bails on missing params
        let result = db.apply_rule(
            &rule,
            &serde_json::Value::Object(step_inputs.clone()),
            None,
            None,
        )?;

        // Merge outputs into shared context
        for (name, val) in &result.outputs {
            ctx.insert(name.clone(), *val);
        }

        let is_skipped = result.check_expression.contains("(skipped:");

        if is_skipped {
            skipped += 1;
        } else if result.pass {
            passed += 1;
        } else {
            failed += 1;
        }

        steps.push(DesignStep {
            seq: i + 1,
            rule_name: rule.name.clone(),
            description: rule.description.clone().unwrap_or_default(),
            inputs: step_inputs
                .into_iter()
                .map(|(k, v)| (k, v.as_f64().unwrap_or(0.0)))
                .collect(),
            formula: rule.formula_expr.clone().unwrap_or_default(),
            outputs: result.outputs,
            check_expr: if is_skipped {
                String::new()
            } else {
                result.check_expression
            },
            passed: result.pass,
            skipped: is_skipped,
            skip_reason: if is_skipped {
                Some("condition not met".into())
            } else {
                None
            },
        });
    }

    Ok(DesignLog {
        pipeline_name: pipeline.name.clone(),
        user_inputs: user_inputs.clone(),
        steps,
        passed,
        failed,
        skipped,
    })
}

// ---------------------------------------------------------------------------
// Trace & Impact Analysis
// ---------------------------------------------------------------------------

/// Result of tracing a parameter back to its source step
#[derive(Debug, serde::Serialize)]
pub struct TraceResult {
    pub step_seq: usize,
    pub rule_name: String,
    pub description: String,
    pub formula: String,
    pub inputs: HashMap<String, f64>,
    pub value: f64,
}

/// Find which step produced a given parameter
pub fn trace_parameter(log: &DesignLog, param_name: &str) -> Option<TraceResult> {
    for step in &log.steps {
        if let Some(&val) = step.outputs.get(param_name) {
            return Some(TraceResult {
                step_seq: step.seq,
                rule_name: step.rule_name.clone(),
                description: step.description.clone(),
                formula: step.formula.clone(),
                inputs: step.inputs.clone(),
                value: val,
            });
        }
    }
    // Check user inputs
    if let Some(&val) = log.user_inputs.get(param_name) {
        return Some(TraceResult {
            step_seq: 0,
            rule_name: "user_input".to_string(),
            description: String::new(),
            formula: String::new(),
            inputs: HashMap::new(),
            value: val,
        });
    }
    None
}

/// A step affected by a parameter change
#[derive(Debug, serde::Serialize)]
pub struct ImpactedStep {
    pub seq: usize,
    pub rule_name: String,
    pub uses_param_directly: bool,
    pub produced_outputs: Vec<String>,
}

/// Result of impact analysis
#[derive(Debug, serde::Serialize)]
pub struct ImpactResult {
    pub changed_param: String,
    pub affected_steps: Vec<ImpactedStep>,
    pub affected_outputs: Vec<String>,
}

/// Analyze which downstream steps would be affected by changing a parameter.
/// Traces the dependency chain: param → steps that use it → their outputs → steps that use those, etc.
pub fn analyze_impact(log: &DesignLog, param_name: &str) -> ImpactResult {
    let mut affected = Vec::new();
    let mut affected_outputs = Vec::new();

    // Track variables whose change propagates
    let mut tainted: HashSet<String> = [param_name.to_string()].into_iter().collect();

    for step in &log.steps {
        if step.skipped {
            continue;
        }

        // Check if this step uses any tainted variable as input
        let uses_tainted: Vec<String> = step
            .inputs
            .keys()
            .filter(|k| tainted.contains(*k))
            .cloned()
            .collect();

        if !uses_tainted.is_empty() {
            let produced: Vec<String> = step.outputs.keys().cloned().collect();
            let uses_directly = uses_tainted.iter().any(|k| k == param_name);

            // Mark outputs as tainted too
            for out in &produced {
                tainted.insert(out.clone());
                if !affected_outputs.contains(out) {
                    affected_outputs.push(out.clone());
                }
            }

            affected.push(ImpactedStep {
                seq: step.seq,
                rule_name: step.rule_name.clone(),
                uses_param_directly: uses_directly,
                produced_outputs: produced,
            });
        }
    }

    ImpactResult {
        changed_param: param_name.to_string(),
        affected_steps: affected,
        affected_outputs,
    }
}
