use anyhow::Result;
use rusqlite::params;

use crate::models::DesignRule;
use crate::ComponentDb;

/// Default design rules seeded into the database
struct RuleDef {
    name: &'static str,
    description: &'static str,
    formula_expr: &'static str,
    check_expr: &'static str,
    parameters: &'static str,
    output_params: &'static str,
    source: &'static str,
}

const DEFAULT_RULES: &[RuleDef] = &[
    RuleDef {
        name: "buck_inductor_selection",
        description: "Minimum inductance for Buck converter based on ripple current ratio",
        formula_expr: "l_min = (vout * (1 - vout / vin)) / (fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "TI Application Note SLVA477",
    },
    RuleDef {
        name: "ldo_dropout_check",
        description: "Verify LDO dropout voltage is sufficient",
        formula_expr: "dropout = vin - vout",
        check_expr: "dropout >= vdropout_max",
        parameters: r#"["vin","vout","vdropout_max"]"#,
        output_params: r#"["dropout"]"#,
        source: "Standard LDO design practice",
    },
    RuleDef {
        name: "cap_voltage_derating",
        description: "Capacitor voltage rating should have margin over operating voltage",
        formula_expr: "min_rating = voperating * derating_factor",
        check_expr: "C_voltage_rating >= min_rating",
        parameters: r#"["voperating","derating_factor"]"#,
        output_params: r#"["min_rating"]"#,
        source: "IPC-9592: capacitor derating guideline",
    },
    RuleDef {
        name: "led_current_resistor",
        description: "Current-limiting resistor for LED circuit",
        formula_expr: "r_value = (vin - vf) / i_led",
        check_expr: "r_power >= (vin - vf) * i_led",
        parameters: r#"["vin","vf","i_led","r_power"]"#,
        output_params: r#"["r_value"]"#,
        source: "Ohm's law applied to LED circuit",
    },
    RuleDef {
        name: "buck_input_capacitor",
        description: "Minimum input capacitance for Buck converter ripple",
        formula_expr: "c_min = iout * (vout / vin) / (fsw * ripple_v)",
        check_expr: "C_value >= c_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["c_min"]"#,
        source: "TI Application Note SLTA055",
    },

    // ===== Shared auxiliary rules =====

    RuleDef {
        name: "cap_ripple_current",
        description: "RMS ripple current rating check (triangular waveform approximation)",
        formula_expr: "i_rms = i_rms_pk * 0.5774",
        check_expr: "C_ripple_rating >= i_rms",
        parameters: r#"["i_rms_pk","C_ripple_rating"]"#,
        output_params: r#"["i_rms"]"#,
        source: "Irms = Ipk/sqrt(3) for triangular ripple",
    },
    RuleDef {
        name: "inductor_saturation_check",
        description: "Inductor saturation current must exceed peak operating current",
        formula_expr: "",
        check_expr: "L_Isat >= iout + i_ripple_pp / 2",
        parameters: r#"["iout","i_ripple_pp","L_Isat"]"#,
        output_params: r#"[]"#,
        source: "Standard inductor sizing practice",
    },
    RuleDef {
        name: "inductor_derating",
        description: "Inductor saturation current with 20% headroom",
        formula_expr: "i_min_sat = (iout + i_ripple_pp / 2) * 1.2",
        check_expr: "L_Isat >= i_min_sat",
        parameters: r#"["iout","i_ripple_pp","L_Isat"]"#,
        output_params: r#"["i_min_sat"]"#,
        source: "Standard inductor derating practice (20% margin)",
    },
    RuleDef {
        name: "thermal_dissipation",
        description: "Power dissipation check for linear/pass elements",
        formula_expr: "p_dissipated = (vin - vout) * iout",
        check_expr: "p_dissipated <= p_max",
        parameters: r#"["vin","vout","iout","p_max"]"#,
        output_params: r#"["p_dissipated"]"#,
        source: "P = (Vin-Vout) * Iout",
    },
    RuleDef {
        name: "efficiency_linear",
        description: "Efficiency check for linear regulators",
        formula_expr: "eff = vout / vin",
        check_expr: "eff >= eff_min",
        parameters: r#"["vin","vout","eff_min"]"#,
        output_params: r#"["eff"]"#,
        source: "eta = Vout/Vin for linear regulators",
    },
    RuleDef {
        name: "thermal_ja_rise",
        description: "Junction temperature rise from thermal resistance",
        formula_expr: "t_rise = p_dissipated * theta_ja; t_junction = t_ambient + t_rise",
        check_expr: "t_junction <= t_max",
        parameters: r#"["p_dissipated","theta_ja","t_ambient","t_max"]"#,
        output_params: r#"["t_rise","t_junction"]"#,
        source: "Tj = Ta + P * Rja",
    },

    // ===== Buck augmentation =====

    RuleDef {
        name: "buck_output_capacitor",
        description: "Minimum output capacitance for Buck converter output voltage ripple",
        formula_expr: "c_out_min = iout * ripple_ratio / (8 * fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","ripple_ratio","fsw","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "dV = I_ripple / (8 * fsw * C)",
    },
    RuleDef {
        name: "buck_duty_cycle",
        description: "Buck converter duty cycle check (max ~90%)",
        formula_expr: "duty = vout / vin",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout"]"#,
        output_params: r#"["duty"]"#,
        source: "D = Vout/Vin",
    },
    RuleDef {
        name: "buck_inductor_ripple",
        description: "Verify inductor ripple current with selected inductor value",
        formula_expr: "i_ripple = (vout * (1 - vout / vin)) / (fsw * L_value)",
        check_expr: "i_ripple <= iout * 0.4",
        parameters: r#"["vin","vout","fsw","L_value","iout"]"#,
        output_params: r#"["i_ripple"]"#,
        source: "dI = Vout*(1-D)/(fsw*L)",
    },
    RuleDef {
        name: "buck_catch_diode",
        description: "Catch diode reverse voltage rating for Buck converter",
        formula_expr: "",
        check_expr: "D_vrrm >= vin * 1.25",
        parameters: r#"["vin","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on catch diode",
    },

    // ===== LDO augmentation =====

    RuleDef {
        name: "ldo_power_dissipation",
        description: "LDO power dissipation and thermal check",
        formula_expr: "p_dissipated = (vin - vout) * iout",
        check_expr: "p_dissipated <= p_max",
        parameters: r#"["vin","vout","iout","p_max"]"#,
        output_params: r#"["p_dissipated"]"#,
        source: "P = (Vin-Vout) * Iout for LDO",
    },
    RuleDef {
        name: "ldo_efficiency",
        description: "LDO efficiency estimation",
        formula_expr: "eff = vout / vin",
        check_expr: "eff >= eff_min",
        parameters: r#"["vin","vout","eff_min"]"#,
        output_params: r#"["eff"]"#,
        source: "eta = Vout/Vin for LDO",
    },
    RuleDef {
        name: "ldo_output_cap",
        description: "LDO output capacitor for transient response (estimation)",
        formula_expr: "c_out_min = iout * vdropout_max / ripple_v",
        check_expr: "",
        parameters: r#"["iout","vdropout_max","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "LDO transient response estimation",
    },

    // ===== Boost (step-up) =====

    RuleDef {
        name: "boost_duty_cycle",
        description: "Boost converter duty cycle check (max ~90%)",
        formula_expr: "duty = 1 - vin / vout",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout"]"#,
        output_params: r#"["duty"]"#,
        source: "D = 1 - Vin/Vout",
    },
    RuleDef {
        name: "boost_inductor_selection",
        description: "Minimum inductance for Boost converter based on ripple current",
        formula_expr: "l_min = (vin * (1 - vin / vout)) / (fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L_min = Vin*D/(fsw*dI)",
    },
    RuleDef {
        name: "boost_inductor_ripple",
        description: "Verify inductor ripple current in Boost converter",
        formula_expr: "i_ripple = (vin * (1 - vin / vout)) / (fsw * L_value)",
        check_expr: "i_ripple <= iout * 0.4",
        parameters: r#"["vin","vout","fsw","L_value","iout"]"#,
        output_params: r#"["i_ripple"]"#,
        source: "dI = Vin*D/(fsw*L) for Boost",
    },
    RuleDef {
        name: "boost_output_capacitor",
        description: "Minimum output capacitance for Boost converter",
        formula_expr: "duty = 1 - vin / vout; c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C) for Boost",
    },
    RuleDef {
        name: "boost_switch_voltage",
        description: "Switch/FET voltage stress for Boost converter",
        formula_expr: "",
        check_expr: "FET_vds >= vout * 1.25",
        parameters: r#"["vout","FET_vds"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on Boost switch",
    },
    RuleDef {
        name: "boost_diode_voltage",
        description: "Output diode reverse voltage rating for Boost converter",
        formula_expr: "",
        check_expr: "D_vrrm >= vout * 1.25",
        parameters: r#"["vout","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on Boost output diode",
    },

    // ===== Buck-Boost (non-inverting) =====

    RuleDef {
        name: "buckboost_duty_cycle",
        description: "Buck-Boost (non-inverting) duty cycle check",
        formula_expr: "duty = vout / (vin + vout)",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout"]"#,
        output_params: r#"["duty"]"#,
        source: "D = Vout/(Vin+Vout)",
    },
    RuleDef {
        name: "buckboost_inductor_selection",
        description: "Minimum inductance for Buck-Boost converter",
        formula_expr: "l_min = (vin * vout) / ((vin + vout) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L = Vin*D/(fsw*dI) with D=Vout/(Vin+Vout)",
    },
    RuleDef {
        name: "buckboost_output_capacitor",
        description: "Minimum output capacitance for Buck-Boost converter",
        formula_expr: "duty = vout / (vin + vout); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C)",
    },

    // ===== Inverting (buck-boost, negative output) =====

    RuleDef {
        name: "inverting_duty_cycle",
        description: "Inverting converter duty cycle check",
        formula_expr: "duty = vout_abs / (vin + vout_abs)",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout_abs"]"#,
        output_params: r#"["duty"]"#,
        source: "D = |Vout|/(Vin+|Vout|)",
    },
    RuleDef {
        name: "inverting_inductor_selection",
        description: "Minimum inductance for inverting converter",
        formula_expr: "l_min = (vin * vout_abs) / ((vin + vout_abs) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout_abs","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "Same as buck-boost with |Vout|",
    },
    RuleDef {
        name: "inverting_output_capacitor",
        description: "Minimum output capacitance for inverting converter",
        formula_expr: "duty = vout_abs / (vin + vout_abs); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout_abs","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C)",
    },
    RuleDef {
        name: "inverting_diode_voltage",
        description: "Diode reverse voltage for inverting converter (sees Vin+|Vout|)",
        formula_expr: "",
        check_expr: "D_vrrm >= (vin + vout_abs) * 1.25",
        parameters: r#"["vin","vout_abs","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "Diode sees Vin+|Vout| with 25% margin",
    },

    // ===== SEPIC =====

    RuleDef {
        name: "sepic_duty_cycle",
        description: "SEPIC converter duty cycle check",
        formula_expr: "duty = vout / (vin + vout)",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout"]"#,
        output_params: r#"["duty"]"#,
        source: "D = Vout/(Vin+Vout) for SEPIC",
    },
    RuleDef {
        name: "sepic_inductor_selection",
        description: "Minimum inductance for SEPIC converter",
        formula_expr: "l_min = (vin * vout) / ((vin + vout) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L1=L2 for coupled SEPIC",
    },
    RuleDef {
        name: "sepic_coupling_cap",
        description: "Minimum coupling capacitor for SEPIC converter",
        formula_expr: "duty = vout / (vin + vout); c_coup_min = iout * duty / (fsw * ripple_v_coup)",
        check_expr: "C_value >= c_coup_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v_coup"]"#,
        output_params: r#"["duty","c_coup_min"]"#,
        source: "Cs carries Iout*D",
    },
    RuleDef {
        name: "sepic_coupling_cap_voltage",
        description: "Coupling capacitor voltage rating for SEPIC (sees Vin)",
        formula_expr: "",
        check_expr: "C_voltage_rating >= vin * 1.25",
        parameters: r#"["vin","C_voltage_rating"]"#,
        output_params: r#"[]"#,
        source: "Coupling cap sees Vin with 25% margin",
    },
    RuleDef {
        name: "sepic_output_capacitor",
        description: "Minimum output capacitance for SEPIC converter",
        formula_expr: "duty = vout / (vin + vout); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "Standard output cap for SEPIC",
    },

    // ===== Charge Pump =====

    RuleDef {
        name: "chargepump_flying_cap",
        description: "Minimum flying capacitor for charge pump voltage doubler",
        formula_expr: "c_fly_min = iout / (2 * fsw * ripple_v)",
        check_expr: "C_value >= c_fly_min",
        parameters: r#"["iout","fsw","ripple_v"]"#,
        output_params: r#"["c_fly_min"]"#,
        source: "Cfly >= Iout/(2*fsw*dV) for doubler",
    },
    RuleDef {
        name: "chargepump_flying_cap_voltage",
        description: "Flying capacitor voltage rating for charge pump",
        formula_expr: "",
        check_expr: "C_voltage_rating >= vin * 1.25",
        parameters: r#"["vin","C_voltage_rating"]"#,
        output_params: r#"[]"#,
        source: "Flying cap sees Vin with 25% margin",
    },
    RuleDef {
        name: "chargepump_output_cap",
        description: "Minimum output capacitor for charge pump",
        formula_expr: "c_out_min = iout / (2 * fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","fsw","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "Cout >= Iout/(2*fsw*dV)",
    },

    // ===== Flyback (isolated) =====

    RuleDef {
        name: "flyback_duty_cycle",
        description: "Flyback converter duty cycle check (max ~75%)",
        formula_expr: "duty = vout / (vout + vin / n)",
        check_expr: "duty <= 0.75",
        parameters: r#"["vin","vout","n"]"#,
        output_params: r#"["duty"]"#,
        source: "D = Vout/(Vout+Vin/N), N=turns ratio",
    },
    RuleDef {
        name: "flyback_transformer_turns",
        description: "Minimum turns ratio for Flyback transformer",
        formula_expr: "n_min = (vin * duty_max) / (vout * (1 - duty_max))",
        check_expr: "n >= n_min",
        parameters: r#"["vin","vout","duty_max","n"]"#,
        output_params: r#"["n_min"]"#,
        source: "N_min = Vin*Dmax/(Vout*(1-Dmax))",
    },
    RuleDef {
        name: "flyback_primary_inductance",
        description: "Minimum primary inductance for Flyback (DCM boundary)",
        formula_expr: "l_pri_min = (vin * vin * duty * duty) / (2 * p_out * fsw)",
        check_expr: "L_pri >= l_pri_min",
        parameters: r#"["vin","duty","p_out","fsw","L_pri"]"#,
        output_params: r#"["l_pri_min"]"#,
        source: "DCM boundary: Lpri > Vin^2*D^2/(2*Pout*fsw)",
    },
    RuleDef {
        name: "flyback_primary_peak_current",
        description: "Primary peak current and saturation check for Flyback",
        formula_expr: "i_pri_peak = (vin * duty) / (fsw * L_pri)",
        check_expr: "L_Isat >= i_pri_peak * 1.2",
        parameters: r#"["vin","duty","fsw","L_pri","L_Isat"]"#,
        output_params: r#"["i_pri_peak"]"#,
        source: "Ipk = Vin*D/(fsw*Lpri) with 20% margin",
    },
    RuleDef {
        name: "flyback_snubber_rcd_cap",
        description: "RCD snubber capacitor for Flyback leakage inductance",
        formula_expr: "c_snub = l_leak * i_pri_peak * i_pri_peak / (v_clamp * v_clamp - v_reflect * v_reflect)",
        check_expr: "C_snub >= c_snub",
        parameters: r#"["l_leak","i_pri_peak","v_clamp","v_reflect","C_snub"]"#,
        output_params: r#"["c_snub"]"#,
        source: "Cs = Llk*Ipk^2/(Vc^2-Vr^2)",
    },
    RuleDef {
        name: "flyback_snubber_resistor",
        description: "RCD snubber resistor for Flyback",
        formula_expr: "r_snub = 1 / (2 * fsw * c_snub)",
        check_expr: "",
        parameters: r#"["fsw","c_snub"]"#,
        output_params: r#"["r_snub"]"#,
        source: "R = 1/(2*fsw*Csnub) time constant",
    },
    RuleDef {
        name: "flyback_output_capacitor",
        description: "Minimum output capacitance for Flyback converter",
        formula_expr: "c_out_min = iout * duty / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","duty","fsw","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "Standard output cap for Flyback",
    },
];

impl ComponentDb {
    /// Insert default design rules (idempotent: skips if rule already exists)
    pub fn seed_default_rules(&self) -> Result<usize> {
        let mut count = 0;
        for def in DEFAULT_RULES {
            // Check if rule already exists
            let exists: bool = self.conn.query_row(
                "SELECT COUNT(*) > 0 FROM design_rules WHERE name = ?1",
                params![def.name],
                |row| row.get(0),
            )?;

            if exists {
                continue;
            }

            self.conn.execute(
                "INSERT INTO design_rules (name, description, formula_expr, check_expr, parameters, output_params, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![def.name, def.description, def.formula_expr, def.check_expr, def.parameters, def.output_params, def.source],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Get all design rules
    pub fn get_all_design_rules(&self) -> Result<Vec<DesignRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source
             FROM design_rules ORDER BY name",
        )?;
        let rules = stmt.query_map([], |row| {
            Ok(DesignRule {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                category_id: row.get(2)?,
                description: row.get(3)?,
                condition_expr: row.get(4)?,
                formula_expr: row.get(5)?,
                check_expr: row.get(6)?,
                parameters: row.get(7)?,
                output_params: row.get(8)?,
                source: row.get(9)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rules)
    }
}

/// A topology candidate from suggestion engine
#[derive(Debug)]
pub struct TopologyCandidate {
    pub topology: String,
    pub estimated_efficiency: f64,
    pub score: f64,
    pub reason: String,
}

/// Suggest suitable topologies based on design requirements.
/// Returns candidates sorted by score (highest first).
pub fn suggest_topologies(vin: f64, vout: f64, iout: f64, isolated: bool) -> Vec<TopologyCandidate> {
    let mut candidates = Vec::new();

    let vin_higher = vin > vout;
    let vout_ratio = if vin > 0.0 { vout / vin } else { 1.0 };

    // LDO: linear regulator, simple but inefficient for large dropout
    if vin_higher {
        let eff = vout_ratio;
        let p_loss = (vin - vout) * iout;
        let score = if p_loss < 1.0 && eff > 0.7 {
            0.95
        } else if p_loss < 3.0 {
            0.6
        } else {
            0.2
        };
        candidates.push(TopologyCandidate {
            topology: "ldo".to_string(),
            estimated_efficiency: eff,
            score,
            reason: format!("LDO: eta={:.0}%, P_loss={:.2}W", eff * 100.0, p_loss),
        });
    }

    // Buck: step-down, high efficiency
    if vin_higher {
        candidates.push(TopologyCandidate {
            topology: "buck".to_string(),
            estimated_efficiency: 0.92,
            score: 0.9,
            reason: "Buck: high efficiency step-down (~90-95%)".to_string(),
        });
    }

    // Boost: step-up
    if !vin_higher && vin > 0.0 {
        candidates.push(TopologyCandidate {
            topology: "boost".to_string(),
            estimated_efficiency: 0.88,
            score: 0.88,
            reason: "Boost: step-up converter (~85-93%)".to_string(),
        });
    }

    // Buck-Boost: when Vin can be above or below Vout
    candidates.push(TopologyCandidate {
        topology: "buckboost".to_string(),
        estimated_efficiency: 0.85,
        score: 0.65,
        reason: "Buck-Boost: handles Vin above or below Vout".to_string(),
    });

    // SEPIC: similar to buck-boost but with non-inverting output and no polarity reversal
    candidates.push(TopologyCandidate {
        topology: "sepic".to_string(),
        estimated_efficiency: 0.83,
        score: 0.6,
        reason: "SEPIC: non-inverting step-up/down, good for battery apps".to_string(),
    });

    // Inverting: when negative output needed
    candidates.push(TopologyCandidate {
        topology: "inverting".to_string(),
        estimated_efficiency: 0.82,
        score: 0.4,
        reason: "Inverting: generates negative output voltage".to_string(),
    });

    // Charge Pump: simple, low current
    if iout < 0.05 {
        candidates.push(TopologyCandidate {
            topology: "chargepump".to_string(),
            estimated_efficiency: 0.85,
            score: 0.85,
            reason: "Charge Pump: ideal for low current (<50mA), no inductor".to_string(),
        });
    }

    // Flyback: isolated
    if isolated {
        candidates.push(TopologyCandidate {
            topology: "flyback".to_string(),
            estimated_efficiency: 0.80,
            score: 0.95,
            reason: "Flyback: isolated converter, multi-output capable".to_string(),
        });
    }

    // Sort by score descending
    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    candidates
}
