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
    domain: &'static str,
    tags: &'static [&'static str],
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
        domain: "power",
        tags: &["buck", "dc-dc"],
    },
    RuleDef {
        name: "ldo_dropout_check",
        description: "Verify LDO dropout voltage is sufficient",
        formula_expr: "dropout = vin - vout",
        check_expr: "dropout >= vdropout_max",
        parameters: r#"["vin","vout","vdropout_max"]"#,
        output_params: r#"["dropout"]"#,
        source: "Standard LDO design practice",
        domain: "power",
        tags: &["ldo", "linear"],
    },
    RuleDef {
        name: "cap_voltage_derating",
        description: "Capacitor voltage rating should have margin over operating voltage",
        formula_expr: "min_rating = voperating * derating_factor",
        check_expr: "C_voltage_rating >= min_rating",
        parameters: r#"["voperating","derating_factor"]"#,
        output_params: r#"["min_rating"]"#,
        source: "IPC-9592: capacitor derating guideline",
        domain: "power",
        tags: &["capacitor", "passive"],
    },
    RuleDef {
        name: "led_current_resistor",
        description: "Current-limiting resistor for LED circuit",
        formula_expr: "r_value = (vin - vf) / i_led",
        check_expr: "r_power >= (vin - vf) * i_led",
        parameters: r#"["vin","vf","i_led","r_power"]"#,
        output_params: r#"["r_value"]"#,
        source: "Ohm's law applied to LED circuit",
        domain: "power",
        tags: &["led"],
    },
    RuleDef {
        name: "buck_input_capacitor",
        description: "Minimum input capacitance for Buck converter ripple",
        formula_expr: "c_min = iout * (vout / vin) / (fsw * ripple_v)",
        check_expr: "C_value >= c_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["c_min"]"#,
        source: "TI Application Note SLTA055",
        domain: "power",
        tags: &["buck", "dc-dc"],
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
        domain: "power",
        tags: &["capacitor", "passive"],
    },
    RuleDef {
        name: "inductor_saturation_check",
        description: "Inductor saturation current must exceed peak operating current",
        formula_expr: "",
        check_expr: "L_Isat >= iout + i_ripple_pp / 2",
        parameters: r#"["iout","i_ripple_pp","L_Isat"]"#,
        output_params: r#"[]"#,
        source: "Standard inductor sizing practice",
        domain: "power",
        tags: &["inductor", "passive"],
    },
    RuleDef {
        name: "inductor_derating",
        description: "Inductor saturation current with 20% headroom",
        formula_expr: "i_min_sat = (iout + i_ripple_pp / 2) * 1.2",
        check_expr: "L_Isat >= i_min_sat",
        parameters: r#"["iout","i_ripple_pp","L_Isat"]"#,
        output_params: r#"["i_min_sat"]"#,
        source: "Standard inductor derating practice (20% margin)",
        domain: "power",
        tags: &["inductor", "passive"],
    },
    RuleDef {
        name: "thermal_dissipation",
        description: "Power dissipation check for linear/pass elements",
        formula_expr: "p_dissipated = (vin - vout) * iout",
        check_expr: "p_dissipated <= p_max",
        parameters: r#"["vin","vout","iout","p_max"]"#,
        output_params: r#"["p_dissipated"]"#,
        source: "P = (Vin-Vout) * Iout",
        domain: "thermal",
        tags: &["thermal"],
    },
    RuleDef {
        name: "efficiency_linear",
        description: "Efficiency check for linear regulators",
        formula_expr: "eff = vout / vin",
        check_expr: "eff >= eff_min",
        parameters: r#"["vin","vout","eff_min"]"#,
        output_params: r#"["eff"]"#,
        source: "eta = Vout/Vin for linear regulators",
        domain: "thermal",
        tags: &["efficiency"],
    },
    RuleDef {
        name: "thermal_ja_rise",
        description: "Junction temperature rise from thermal resistance",
        formula_expr: "t_rise = p_dissipated * theta_ja; t_junction = t_ambient + t_rise",
        check_expr: "t_junction <= t_max",
        parameters: r#"["p_dissipated","theta_ja","t_ambient","t_max"]"#,
        output_params: r#"["t_rise","t_junction"]"#,
        source: "Tj = Ta + P * Rja",
        domain: "thermal",
        tags: &["thermal"],
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
        domain: "power",
        tags: &["buck", "dc-dc"],
    },
    RuleDef {
        name: "buck_duty_cycle",
        description: "Buck converter duty cycle check (max ~90%)",
        formula_expr: "duty = vout / vin",
        check_expr: "duty <= 0.9",
        parameters: r#"["vin","vout"]"#,
        output_params: r#"["duty"]"#,
        source: "D = Vout/Vin",
        domain: "power",
        tags: &["buck", "dc-dc"],
    },
    RuleDef {
        name: "buck_inductor_ripple",
        description: "Verify inductor ripple current with selected inductor value",
        formula_expr: "i_ripple = (vout * (1 - vout / vin)) / (fsw * L_value)",
        check_expr: "i_ripple <= iout * 0.4",
        parameters: r#"["vin","vout","fsw","L_value","iout"]"#,
        output_params: r#"["i_ripple"]"#,
        source: "dI = Vout*(1-D)/(fsw*L)",
        domain: "power",
        tags: &["buck", "dc-dc"],
    },
    RuleDef {
        name: "buck_catch_diode",
        description: "Catch diode reverse voltage rating for Buck converter",
        formula_expr: "",
        check_expr: "D_vrrm >= vin * 1.25",
        parameters: r#"["vin","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on catch diode",
        domain: "power",
        tags: &["buck", "dc-dc"],
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
        domain: "power",
        tags: &["ldo", "linear"],
    },
    RuleDef {
        name: "ldo_efficiency",
        description: "LDO efficiency estimation",
        formula_expr: "eff = vout / vin",
        check_expr: "eff >= eff_min",
        parameters: r#"["vin","vout","eff_min"]"#,
        output_params: r#"["eff"]"#,
        source: "eta = Vout/Vin for LDO",
        domain: "power",
        tags: &["ldo", "linear"],
    },
    RuleDef {
        name: "ldo_output_cap",
        description: "LDO output capacitor for transient response (estimation)",
        formula_expr: "c_out_min = iout * vdropout_max / ripple_v",
        check_expr: "",
        parameters: r#"["iout","vdropout_max","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "LDO transient response estimation",
        domain: "power",
        tags: &["ldo", "linear"],
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
        domain: "power",
        tags: &["boost", "dc-dc"],
    },
    RuleDef {
        name: "boost_inductor_selection",
        description: "Minimum inductance for Boost converter based on ripple current",
        formula_expr: "l_min = (vin * (1 - vin / vout)) / (fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L_min = Vin*D/(fsw*dI)",
        domain: "power",
        tags: &["boost", "dc-dc"],
    },
    RuleDef {
        name: "boost_inductor_ripple",
        description: "Verify inductor ripple current in Boost converter",
        formula_expr: "i_ripple = (vin * (1 - vin / vout)) / (fsw * L_value)",
        check_expr: "i_ripple <= iout * 0.4",
        parameters: r#"["vin","vout","fsw","L_value","iout"]"#,
        output_params: r#"["i_ripple"]"#,
        source: "dI = Vin*D/(fsw*L) for Boost",
        domain: "power",
        tags: &["boost", "dc-dc"],
    },
    RuleDef {
        name: "boost_output_capacitor",
        description: "Minimum output capacitance for Boost converter",
        formula_expr: "duty = 1 - vin / vout; c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C) for Boost",
        domain: "power",
        tags: &["boost", "dc-dc"],
    },
    RuleDef {
        name: "boost_switch_voltage",
        description: "Switch/FET voltage stress for Boost converter",
        formula_expr: "",
        check_expr: "FET_vds >= vout * 1.25",
        parameters: r#"["vout","FET_vds"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on Boost switch",
        domain: "power",
        tags: &["boost", "dc-dc"],
    },
    RuleDef {
        name: "boost_diode_voltage",
        description: "Output diode reverse voltage rating for Boost converter",
        formula_expr: "",
        check_expr: "D_vrrm >= vout * 1.25",
        parameters: r#"["vout","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "25% voltage margin on Boost output diode",
        domain: "power",
        tags: &["boost", "dc-dc"],
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
        domain: "power",
        tags: &["buck-boost", "dc-dc"],
    },
    RuleDef {
        name: "buckboost_inductor_selection",
        description: "Minimum inductance for Buck-Boost converter",
        formula_expr: "l_min = (vin * vout) / ((vin + vout) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L = Vin*D/(fsw*dI) with D=Vout/(Vin+Vout)",
        domain: "power",
        tags: &["buck-boost", "dc-dc"],
    },
    RuleDef {
        name: "buckboost_output_capacitor",
        description: "Minimum output capacitance for Buck-Boost converter",
        formula_expr: "duty = vout / (vin + vout); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C)",
        domain: "power",
        tags: &["buck-boost", "dc-dc"],
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
        domain: "power",
        tags: &["inverting", "dc-dc"],
    },
    RuleDef {
        name: "inverting_inductor_selection",
        description: "Minimum inductance for inverting converter",
        formula_expr: "l_min = (vin * vout_abs) / ((vin + vout_abs) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout_abs","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "Same as buck-boost with |Vout|",
        domain: "power",
        tags: &["inverting", "dc-dc"],
    },
    RuleDef {
        name: "inverting_output_capacitor",
        description: "Minimum output capacitance for inverting converter",
        formula_expr: "duty = vout_abs / (vin + vout_abs); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout_abs","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "dV = Iout*D/(fsw*C)",
        domain: "power",
        tags: &["inverting", "dc-dc"],
    },
    RuleDef {
        name: "inverting_diode_voltage",
        description: "Diode reverse voltage for inverting converter (sees Vin+|Vout|)",
        formula_expr: "",
        check_expr: "D_vrrm >= (vin + vout_abs) * 1.25",
        parameters: r#"["vin","vout_abs","D_vrrm"]"#,
        output_params: r#"[]"#,
        source: "Diode sees Vin+|Vout| with 25% margin",
        domain: "power",
        tags: &["inverting", "dc-dc"],
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
        domain: "power",
        tags: &["sepic", "dc-dc"],
    },
    RuleDef {
        name: "sepic_inductor_selection",
        description: "Minimum inductance for SEPIC converter",
        formula_expr: "l_min = (vin * vout) / ((vin + vout) * fsw * ripple_ratio * iout)",
        check_expr: "L_value >= l_min * 0.8",
        parameters: r#"["vin","vout","iout","fsw","ripple_ratio"]"#,
        output_params: r#"["l_min"]"#,
        source: "L1=L2 for coupled SEPIC",
        domain: "power",
        tags: &["sepic", "dc-dc"],
    },
    RuleDef {
        name: "sepic_coupling_cap",
        description: "Minimum coupling capacitor for SEPIC converter",
        formula_expr: "duty = vout / (vin + vout); c_coup_min = iout * duty / (fsw * ripple_v_coup)",
        check_expr: "C_value >= c_coup_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v_coup"]"#,
        output_params: r#"["duty","c_coup_min"]"#,
        source: "Cs carries Iout*D",
        domain: "power",
        tags: &["sepic", "dc-dc"],
    },
    RuleDef {
        name: "sepic_coupling_cap_voltage",
        description: "Coupling capacitor voltage rating for SEPIC (sees Vin)",
        formula_expr: "",
        check_expr: "C_voltage_rating >= vin * 1.25",
        parameters: r#"["vin","C_voltage_rating"]"#,
        output_params: r#"[]"#,
        source: "Coupling cap sees Vin with 25% margin",
        domain: "power",
        tags: &["sepic", "dc-dc"],
    },
    RuleDef {
        name: "sepic_output_capacitor",
        description: "Minimum output capacitance for SEPIC converter",
        formula_expr: "duty = vout / (vin + vout); c_out_min = (iout * duty) / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","vin","vout","fsw","ripple_v"]"#,
        output_params: r#"["duty","c_out_min"]"#,
        source: "Standard output cap for SEPIC",
        domain: "power",
        tags: &["sepic", "dc-dc"],
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
        domain: "power",
        tags: &["charge-pump"],
    },
    RuleDef {
        name: "chargepump_flying_cap_voltage",
        description: "Flying capacitor voltage rating for charge pump",
        formula_expr: "",
        check_expr: "C_voltage_rating >= vin * 1.25",
        parameters: r#"["vin","C_voltage_rating"]"#,
        output_params: r#"[]"#,
        source: "Flying cap sees Vin with 25% margin",
        domain: "power",
        tags: &["charge-pump"],
    },
    RuleDef {
        name: "chargepump_output_cap",
        description: "Minimum output capacitor for charge pump",
        formula_expr: "c_out_min = iout / (2 * fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","fsw","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "Cout >= Iout/(2*fsw*dV)",
        domain: "power",
        tags: &["charge-pump"],
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
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_transformer_turns",
        description: "Minimum turns ratio for Flyback transformer",
        formula_expr: "n_min = (vin * duty_max) / (vout * (1 - duty_max))",
        check_expr: "n >= n_min",
        parameters: r#"["vin","vout","duty_max","n"]"#,
        output_params: r#"["n_min"]"#,
        source: "N_min = Vin*Dmax/(Vout*(1-Dmax))",
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_primary_inductance",
        description: "Minimum primary inductance for Flyback (DCM boundary)",
        formula_expr: "l_pri_min = (vin * vin * duty * duty) / (2 * p_out * fsw)",
        check_expr: "L_pri >= l_pri_min",
        parameters: r#"["vin","duty","p_out","fsw","L_pri"]"#,
        output_params: r#"["l_pri_min"]"#,
        source: "DCM boundary: Lpri > Vin^2*D^2/(2*Pout*fsw)",
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_primary_peak_current",
        description: "Primary peak current and saturation check for Flyback",
        formula_expr: "i_pri_peak = (vin * duty) / (fsw * L_pri)",
        check_expr: "L_Isat >= i_pri_peak * 1.2",
        parameters: r#"["vin","duty","fsw","L_pri","L_Isat"]"#,
        output_params: r#"["i_pri_peak"]"#,
        source: "Ipk = Vin*D/(fsw*Lpri) with 20% margin",
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_snubber_rcd_cap",
        description: "RCD snubber capacitor for Flyback leakage inductance",
        formula_expr: "c_snub = l_leak * i_pri_peak * i_pri_peak / (v_clamp * v_clamp - v_reflect * v_reflect)",
        check_expr: "C_snub >= c_snub",
        parameters: r#"["l_leak","i_pri_peak","v_clamp","v_reflect","C_snub"]"#,
        output_params: r#"["c_snub"]"#,
        source: "Cs = Llk*Ipk^2/(Vc^2-Vr^2)",
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_snubber_resistor",
        description: "RCD snubber resistor for Flyback",
        formula_expr: "r_snub = 1 / (2 * fsw * c_snub)",
        check_expr: "",
        parameters: r#"["fsw","c_snub"]"#,
        output_params: r#"["r_snub"]"#,
        source: "R = 1/(2*fsw*Csnub) time constant",
        domain: "power",
        tags: &["flyback", "isolated"],
    },
    RuleDef {
        name: "flyback_output_capacitor",
        description: "Minimum output capacitance for Flyback converter",
        formula_expr: "c_out_min = iout * duty / (fsw * ripple_v)",
        check_expr: "C_value >= c_out_min",
        parameters: r#"["iout","duty","fsw","ripple_v"]"#,
        output_params: r#"["c_out_min"]"#,
        source: "Standard output cap for Flyback",
        domain: "power",
        tags: &["flyback", "isolated"],
    },

    // ===== Signal Integrity (SI) =====

    RuleDef {
        name: "si_microstrip_impedance",
        description: "Microstrip trace impedance using IPC-2141A simplified formula",
        formula_expr: "z0 = (87 / sqrt(epsilon_r + 1.41)) * ln(5.98 * h / w)",
        check_expr: "z0 >= z_min; z0 <= z_max",
        parameters: r#"["epsilon_r","h","w","z_min","z_max"]"#,
        output_params: r#"["z0"]"#,
        source: "IPC-2141A: Z0 = (87/sqrt(er+1.41)) * ln(5.98*h/w)",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_stripline_impedance",
        description: "Stripline trace impedance using IPC-2141A formula",
        formula_expr: "z0 = (60 / sqrt(epsilon_r)) * ln(1.9 * h / (0.8 * w + t))",
        check_expr: "z0 >= z_min; z0 <= z_max",
        parameters: r#"["epsilon_r","h","w","t","z_min","z_max"]"#,
        output_params: r#"["z0"]"#,
        source: "IPC-2141A: Z0 = (60/sqrt(er)) * ln(1.9*h/(0.8*w+t))",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_trace_resistance",
        description: "DC resistance of a copper trace (R = rho * L / (w * t))",
        formula_expr: "r_trace = rho * L / (w * t)",
        check_expr: "r_trace <= r_max",
        parameters: r#"["rho","L","w","t","r_max"]"#,
        output_params: r#"["r_trace"]"#,
        source: "R = rho*L/(w*t), copper rho=1.72e-8 Ohm*m",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_propagation_delay",
        description: "Signal propagation delay through a trace (tpd = sqrt(er_eff) * L / c)",
        formula_expr: "t_delay = sqrt(epsilon_eff) * L / c",
        check_expr: "t_delay <= t_max",
        parameters: r#"["epsilon_eff","L","c","t_max"]"#,
        output_params: r#"["t_delay"]"#,
        source: "tpd = sqrt(eeff) * L / c, c=3e8 m/s",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_trace_current_capacity",
        description: "IPC-2221 internal trace current capacity (simplified)",
        formula_expr: "i_max = k * pow(trace_width * copper_weight * temp_rise, 0.5)",
        check_expr: "i_max >= i_required",
        parameters: r#"["trace_width","copper_weight","temp_rise","k","i_required"]"#,
        output_params: r#"["i_max"]"#,
        source: "IPC-2221: I = k*(A*dT)^0.5, k=0.048 (internal), 0.024 (external)",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_via_resistance",
        description: "DC resistance of a via (annular ring cross-section)",
        formula_expr: "r_via = rho * h / (pi * (r * r - (r - t) * (r - t)))",
        check_expr: "r_via <= r_max",
        parameters: r#"["rho","h","r","t","r_max"]"#,
        output_params: r#"["r_via"]"#,
        source: "R_via = rho*h / (pi*(r^2 - (r-t)^2))",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },
    RuleDef {
        name: "si_crosstalk_next",
        description: "Near-end crosstalk voltage estimation (simplified capacitive coupling)",
        formula_expr: "next_voltage = v_signal * coupling_length * signal_freq * 1e-12 / spacing",
        check_expr: "next_voltage <= v_noise_max",
        parameters: r#"["v_signal","coupling_length","signal_freq","spacing","v_noise_max"]"#,
        output_params: r#"["next_voltage"]"#,
        source: "NEXT simplified: proportional to L*f, inversely proportional to spacing",
        domain: "si",
        tags: &["signal-integrity", "impedance"],
    },

    // ===== EMC (Electromagnetic Compatibility) =====

    RuleDef {
        name: "emc_decouple_capacitance",
        description: "Minimum decoupling capacitance for transient current (C = I * dt / dV)",
        formula_expr: "c_min = i_transient * dt / dv_target",
        check_expr: "C_value >= c_min",
        parameters: r#"["i_transient","dt","dv_target","C_value"]"#,
        output_params: r#"["c_min"]"#,
        source: "C = I * dt / dV for transient decoupling",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_decouple_esr",
        description: "Maximum ESR for decoupling capacitor (ESR <= dV / I_ripple)",
        formula_expr: "esr_max = dv_target / i_ripple",
        check_expr: "C_esr <= esr_max",
        parameters: r#"["dv_target","i_ripple","C_esr"]"#,
        output_params: r#"["esr_max"]"#,
        source: "ESR <= dV/I_ripple for effective decoupling",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_decouple_resonance",
        description: "Self-resonance frequency of decoupling capacitor (f_res = 1/(2*pi*sqrt(L*C)))",
        formula_expr: "f_resonance = 1 / (2 * pi * sqrt(L_esl * C_value))",
        check_expr: "f_resonance >= f_target",
        parameters: r#"["L_esl","C_value","f_target"]"#,
        output_params: r#"["f_resonance"]"#,
        source: "f_res = 1/(2*pi*sqrt(L_ESL*C))",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_lc_filter_cutoff",
        description: "LC low-pass filter cutoff frequency (fc = 1/(2*pi*sqrt(L*C)))",
        formula_expr: "f_cutoff = 1 / (2 * pi * sqrt(L_value * C_value))",
        check_expr: "f_cutoff <= f_target",
        parameters: r#"["L_value","C_value","f_target"]"#,
        output_params: r#"["f_cutoff"]"#,
        source: "fc = 1/(2*pi*sqrt(L*C)) for LC low-pass",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_rc_filter_cutoff",
        description: "RC low-pass filter cutoff frequency (fc = 1/(2*pi*R*C))",
        formula_expr: "f_cutoff = 1 / (2 * pi * R_value * C_value)",
        check_expr: "f_cutoff <= f_target",
        parameters: r#"["R_value","C_value","f_target"]"#,
        output_params: r#"["f_cutoff"]"#,
        source: "fc = 1/(2*pi*R*C) for RC low-pass",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_cm_choke_impedance",
        description: "Common mode choke impedance at target frequency (Z = 2*pi*f*L)",
        formula_expr: "z_choke = 2 * pi * frequency * L_cm",
        check_expr: "z_choke >= z_min",
        parameters: r#"["frequency","L_cm","z_min"]"#,
        output_params: r#"["z_choke"]"#,
        source: "Z = 2*pi*f*L for common mode choke",
        domain: "emc",
        tags: &["emc", "filter"],
    },
    RuleDef {
        name: "emc_power_plane_capacitance",
        description: "Parasitic capacitance between power and ground planes (C = e0*er*A/d)",
        formula_expr: "c_plane = 8.854e-12 * epsilon_r * area / dielectric_thickness",
        check_expr: "c_plane >= c_min_required",
        parameters: r#"["epsilon_r","area","dielectric_thickness","c_min_required"]"#,
        output_params: r#"["c_plane"]"#,
        source: "C = e0*er*A/d, e0=8.854e-12 F/m",
        domain: "emc",
        tags: &["emc", "filter"],
    },

    // ===== Timing =====

    RuleDef {
        name: "timing_setup_margin",
        description: "Setup time margin check (margin = Tclk - Tckq - Tprop - Tsetup)",
        formula_expr: "t_setup_margin = t_clk_period - t_ckq - t_prop - t_setup",
        check_expr: "t_setup_margin >= t_margin_min",
        parameters: r#"["t_clk_period","t_ckq","t_prop","t_setup","t_margin_min"]"#,
        output_params: r#"["t_setup_margin"]"#,
        source: "Tmargin = Tclk - Tckq - Tprop - Tsetup",
        domain: "timing",
        tags: &["timing", "digital"],
    },
    RuleDef {
        name: "timing_hold_margin",
        description: "Hold time margin check (margin = Tckq + Tprop_min - Thold)",
        formula_expr: "t_hold_margin = t_ckq + t_prop_min - t_hold",
        check_expr: "t_hold_margin >= t_margin_min",
        parameters: r#"["t_ckq","t_prop_min","t_hold","t_margin_min"]"#,
        output_params: r#"["t_hold_margin"]"#,
        source: "Tmargin = Tckq + Tprop_min - Thold",
        domain: "timing",
        tags: &["timing", "digital"],
    },
    RuleDef {
        name: "timing_clock_jitter_budget",
        description: "Clock jitter budget allocation across n sources (equal partition)",
        formula_expr: "t_jitter_per_source = t_jitter_total / sqrt(n_sources)",
        check_expr: "t_jitter_per_source <= t_jitter_device",
        parameters: r#"["t_jitter_total","n_sources","t_jitter_device"]"#,
        output_params: r#"["t_jitter_per_source"]"#,
        source: "RSS: total_jitter = sqrt(sum(jitter_i^2)), equal partition",
        domain: "timing",
        tags: &["timing", "digital"],
    },
    RuleDef {
        name: "timing_spi_max_freq",
        description: "SPI maximum clock frequency from propagation delay and setup time",
        formula_expr: "f_max_spi = 1 / (2 * (t_prop + t_setup))",
        check_expr: "f_clk <= f_max_spi",
        parameters: r#"["t_prop","t_setup","f_clk"]"#,
        output_params: r#"["f_max_spi"]"#,
        source: "fmax = 1/(2*(Tprop+Tsetup)) for SPI",
        domain: "timing",
        tags: &["timing", "digital"],
    },
    RuleDef {
        name: "timing_uart_baud_error",
        description: "UART baud rate error percentage",
        formula_expr: "baud_error_pct = abs(baud_actual - baud_target) / baud_target * 100",
        check_expr: "baud_error_pct <= error_max",
        parameters: r#"["baud_target","baud_actual","error_max"]"#,
        output_params: r#"["baud_error_pct"]"#,
        source: "Error% = |Bactual-Btarget|/Btarget * 100",
        domain: "timing",
        tags: &["timing", "digital"],
    },
    // ── B1: Interface Protocol Skills ──────────────────────────────────
    RuleDef {
        name: "interface_i2c_pullup",
        description: "I2C pull-up resistor value from supply voltage and required sink current",
        formula_expr: "r_pullup = (vcc - v_ol) / i_sink; r_min = (vcc - 0.4) / i_sink_max; r_max = (vcc - v_ih_min) / (n_devices * i_leak)",
        check_expr: "r_pullup >= r_min; r_pullup <= r_max",
        parameters: r#"["vcc","v_ol","i_sink","i_sink_max","v_ih_min","n_devices","i_leak"]"#,
        output_params: r#"["r_pullup","r_min","r_max"]"#,
        source: "I2C-bus spec: Rp = (Vcc - Vol) / I_sink; range limited by rise time and leakage",
        domain: "interface",
        tags: &["interface", "i2c"],
    },
    RuleDef {
        name: "interface_i2c_rise_time",
        description: "I2C bus rise time from pull-up resistor and total bus capacitance",
        formula_expr: "t_rise = 0.8473 * r_pullup * c_bus",
        check_expr: "t_rise <= t_rise_max",
        parameters: r#"["r_pullup","c_bus","t_rise_max"]"#,
        output_params: r#"["t_rise"]"#,
        source: "I2C spec: tr = 0.8473 * Rp * Cb (RC charge curve 30%→70%)",
        domain: "interface",
        tags: &["interface", "i2c"],
    },
    RuleDef {
        name: "interface_i2c_bus_capacitance",
        description: "I2C total bus capacitance vs maximum allowed (400pF standard limit)",
        formula_expr: "c_total = c_wire + n_devices * c_pin",
        check_expr: "c_total <= c_bus_max",
        parameters: r#"["c_wire","n_devices","c_pin","c_bus_max"]"#,
        output_params: r#"["c_total"]"#,
        source: "I2C spec: max bus capacitance 400pF (standard) / 550pF (FM+)",
        domain: "interface",
        tags: &["interface", "i2c"],
    },
    RuleDef {
        name: "interface_can_termination",
        description: "CAN bus termination resistor matching cable impedance",
        formula_expr: "r_term = z_cable; v_diff_terminated = v_dominant * r_term / (r_term + r_term + r_driver)",
        check_expr: "r_term >= 100; r_term <= 130; v_diff_terminated >= 1.5",
        parameters: r#"["z_cable","v_dominant","r_driver"]"#,
        output_params: r#"["r_term","v_diff_terminated"]"#,
        source: "ISO 11898: termination = cable impedance (~120 ohm), min diff voltage 1.5V",
        domain: "interface",
        tags: &["interface", "can"],
    },
    RuleDef {
        name: "interface_can_bus_loading",
        description: "CAN bus total loading from all node capacitances and wire",
        formula_expr: "c_bus_total = c_wire_per_m * l_bus + n_nodes * c_node; f_bit_max = 1 / (2 * t_prop_segment)",
        check_expr: "c_bus_total <= c_bus_max",
        parameters: r#"["c_wire_per_m","l_bus","n_nodes","c_node","t_prop_segment","c_bus_max"]"#,
        output_params: r#"["c_bus_total","f_bit_max"]"#,
        source: "CAN bus: total C affects propagation delay and max bitrate",
        domain: "interface",
        tags: &["interface", "can"],
    },
    RuleDef {
        name: "interface_spi_clock_margin",
        description: "SPI clock margin: actual freq vs max from propagation+setup",
        formula_expr: "f_max_spi = 1 / (2 * (t_prop + t_setup)); margin_pct = (f_max_spi - f_clk) / f_max_spi * 100",
        check_expr: "f_clk <= f_max_spi; margin_pct >= margin_min",
        parameters: r#"["t_prop","t_setup","f_clk","margin_min"]"#,
        output_params: r#"["f_max_spi","margin_pct"]"#,
        source: "SPI: fmax = 1/(2*(Tprop+Tsetup)), need positive margin",
        domain: "interface",
        tags: &["interface", "spi"],
    },
    RuleDef {
        name: "interface_uart_baud_margin",
        description: "UART baud rate error margin from clock frequency and desired baud",
        formula_expr: "baud_div = round(f_clk / (16 * baud_target)); baud_actual = f_clk / (16 * baud_div); baud_error_pct = abs(baud_actual - baud_target) / baud_target * 100",
        check_expr: "baud_error_pct <= error_max",
        parameters: r#"["f_clk","baud_target","error_max"]"#,
        output_params: r#"["baud_div","baud_actual","baud_error_pct"]"#,
        source: "UART: divider = round(Fclk/(16*Baud)), error% must be <2-3%",
        domain: "interface",
        tags: &["interface", "uart"],
    },
    // ── B2: Protection Circuit Skills ──────────────────────────────────
    RuleDef {
        name: "protection_tvs_selection",
        description: "TVS diode clamping voltage and power dissipation check",
        formula_expr: "v_clamp_max = v_wm * clamp_ratio; p_peak = (v_clamp_max - v_supply) / (v_clamp_max / i_pp); p_avg = p_peak * t_pulse / t_period",
        check_expr: "v_clamp_max <= v_max_withstand; p_avg <= p_dissipation_max",
        parameters: r#"["v_wm","clamp_ratio","v_supply","i_pp","v_max_withstand","t_pulse","t_period","p_dissipation_max"]"#,
        output_params: r#"["v_clamp_max","p_peak","p_avg"]"#,
        source: "TVS: Vclamp = Vwm * ratio, Ppp = (Vc-Vs)/(Vc/Ipp), check against abs max",
        domain: "protection",
        tags: &["protection", "tvs"],
    },
    RuleDef {
        name: "protection_esd_clamp",
        description: "ESD protection clamp voltage check per IEC 61000-4-2",
        formula_expr: "v_dynamic = v_clamp + i_esd * r_dynamic; margin_pct = (v_max_withstand - v_dynamic) / v_max_withstand * 100",
        check_expr: "v_dynamic <= v_max_withstand; margin_pct >= margin_min",
        parameters: r#"["v_clamp","i_esd","r_dynamic","v_max_withstand","margin_min"]"#,
        output_params: r#"["v_dynamic","margin_pct"]"#,
        source: "ESD: Vdyn = Vclamp + Iesd*Rdyn, must be below IC abs max",
        domain: "protection",
        tags: &["protection", "esd"],
    },
    RuleDef {
        name: "protection_overvoltage_threshold",
        description: "Over-voltage protection threshold with hysteresis",
        formula_expr: "r_div_upper = (v_trip - v_ref) / i_div; r_div_lower = v_ref / i_div; v_hyst = v_ref * (1 + r_hyst / r_div_lower)",
        check_expr: "v_trip <= v_max_input; v_trip >= v_nominal * 1.1",
        parameters: r#"["v_trip","v_ref","i_div","v_max_input","v_nominal","r_hyst"]"#,
        output_params: r#"["r_div_upper","r_div_lower","v_hyst"]"#,
        source: "OVP: resistor divider sets trip point, hysteresis prevents chattering",
        domain: "protection",
        tags: &["protection", "ovp"],
    },
    RuleDef {
        name: "protection_overcurrent_sense",
        description: "Over-current protection sense resistor and threshold",
        formula_expr: "r_sense = v_sense_max / i_trip; p_sense = i_max * i_max * r_sense; v_out_fault = i_trip * r_sense",
        check_expr: "p_sense <= p_resistor_max; v_out_fault >= v_adc_min",
        parameters: r#"["i_trip","v_sense_max","i_max","p_resistor_max","v_adc_min"]"#,
        output_params: r#"["r_sense","p_sense","v_out_fault"]"#,
        source: "OCP: Rsense = Vsense_max/Itrip, check power and ADC resolution",
        domain: "protection",
        tags: &["protection", "ocp"],
    },
    RuleDef {
        name: "protection_reverse_polarity",
        description: "Reverse polarity protection MOSFET selection",
        formula_expr: "v_gs_on = v_supply - v_diode; p_mos = i_load * i_load * r_ds_on; v_drop = i_load * r_ds_on",
        check_expr: "v_gs_on >= v_gs_th * 1.5; v_drop <= v_drop_max; p_mos <= p_dissipation_max",
        parameters: r#"["v_supply","v_diode","i_load","r_ds_on","v_gs_th","v_drop_max","p_dissipation_max"]"#,
        output_params: r#"["v_gs_on","p_mos","v_drop"]"#,
        source: "Rev-polarity PMOS: Vgs=Vs-Vdiode, must exceed Vth*1.5 for full enhancement",
        domain: "protection",
        tags: &["protection", "reverse-polarity"],
    },
    // ── B3: Crystal/Clock Skills ───────────────────────────────────────
    RuleDef {
        name: "clock_load_capacitance",
        description: "Crystal load capacitance from external caps and stray capacitance",
        formula_expr: "c_load_required = (c1 * c2) / (c1 + c2) + c_stray; c_calc = 2 * (c_load_target - c_stray)",
        check_expr: "c_load_required >= c_load_min; c_load_required <= c_load_max",
        parameters: r#"["c1","c2","c_stray","c_load_target","c_load_min","c_load_max"]"#,
        output_params: r#"["c_load_required","c_calc"]"#,
        source: "XTAL: CL = C1*C2/(C1+C2) + Cstray, typically Cstray=3-7pF",
        domain: "clock",
        tags: &["clock", "crystal"],
    },
    RuleDef {
        name: "clock_drive_level",
        description: "Crystal drive level power dissipation check",
        formula_expr: "p_xtal = 2 * pi() * f_xtal * esr * i_rms * i_rms; p_margin_pct = (p_max - p_xtal) / p_max * 100",
        check_expr: "p_xtal <= p_max; p_margin_pct >= 10",
        parameters: r#"["f_xtal","esr","i_rms","p_max"]"#,
        output_params: r#"["p_xtal","p_margin_pct"]"#,
        source: "Crystal: P = 2*pi*F*ESR*Irms^2, must be below max drive level",
        domain: "clock",
        tags: &["clock", "crystal"],
    },
    RuleDef {
        name: "clock_frequency_stability",
        description: "Crystal total frequency stability budget over temperature and aging",
        formula_expr: "stability_total = stability_temp + stability_initial + stability_aging; ppm_total = stability_total; f_error = f_xtal * ppm_total / 1e6",
        check_expr: "ppm_total <= ppm_max",
        parameters: r#"["stability_temp","stability_initial","stability_aging","f_xtal","ppm_max"]"#,
        output_params: r#"["stability_total","ppm_total","f_error"]"#,
        source: "XTAL: total ppm = temp + initial + aging, check against system requirement",
        domain: "clock",
        tags: &["clock", "crystal"],
    },
    RuleDef {
        name: "clock_pll_bandwidth",
        description: "PLL loop bandwidth and phase margin check",
        formula_expr: "f_loop = i_cp / (2 * pi() * n_div * c_loop); phase_margin = 90 - 57.3 * f_loop / f_pole; f_pole = 1 / (2 * pi() * c_loop * r_loop)",
        check_expr: "phase_margin >= 45; phase_margin <= 80",
        parameters: r#"["i_cp","n_div","c_loop","r_loop"]"#,
        output_params: r#"["f_loop","phase_margin","f_pole"]"#,
        source: "PLL: fBW = Icp/(2*pi*N*CLoop), PM=90-arctan(fBW/fpole)*57.3",
        domain: "clock",
        tags: &["clock", "pll"],
    },
    // ── B4: Analog Circuit Skills ──────────────────────────────────────
    RuleDef {
        name: "analog_opamp_gain",
        description: "Non-inverting op-amp gain from feedback resistor network",
        formula_expr: "gain = 1 + r_f / r_g; v_out_max = gain * v_in_max; v_cm = v_in_max",
        check_expr: "gain <= gain_max; v_out_max <= v_supply - v_headroom; v_cm <= v_cm_max",
        parameters: r#"["r_f","r_g","v_in_max","v_supply","v_headroom","gain_max","v_cm_max"]"#,
        output_params: r#"["gain","v_out_max","v_cm"]"#,
        source: "OpAmp: Av = 1 + Rf/Rg (non-inverting), check output swing and CM range",
        domain: "analog",
        tags: &["analog", "opamp"],
    },
    RuleDef {
        name: "analog_opamp_inverting_gain",
        description: "Inverting op-amp gain and input impedance",
        formula_expr: "gain = -r_f / r_in; v_out_max = abs(gain) * v_in_max; z_in = r_in; r_bias = r_in * r_f / (r_in + r_f)",
        check_expr: "abs(gain) <= gain_max; v_out_max <= v_supply - v_headroom",
        parameters: r#"["r_f","r_in","v_in_max","v_supply","v_headroom","gain_max"]"#,
        output_params: r#"["gain","v_out_max","z_in","r_bias"]"#,
        source: "OpAmp: Av = -Rf/Rin (inverting), Zin = Rin, Rbias = Rin||Rf",
        domain: "analog",
        tags: &["analog", "opamp"],
    },
    RuleDef {
        name: "analog_opamp_bandwidth",
        description: "Op-amp closed-loop bandwidth from GBW product",
        formula_expr: "f_3db = gbw / gain; sr_required = 2 * pi() * f_signal * v_out_peak",
        check_expr: "f_3db >= f_signal * 10; sr_required <= slew_rate",
        parameters: r#"["gbw","gain","f_signal","v_out_peak","slew_rate"]"#,
        output_params: r#"["f_3db","sr_required"]"#,
        source: "OpAmp: f3dB = GBW/Av, SR >= 2*pi*f*Vpeak, need 10x signal bandwidth",
        domain: "analog",
        tags: &["analog", "opamp"],
    },
    RuleDef {
        name: "analog_opamp_bias",
        description: "Op-amp input bias current error analysis",
        formula_expr: "v_offset_bias = i_bias * r_bias_eq; v_offset_total = v_offset_bias + v_os; error_pct = v_offset_total / v_signal_min * 100",
        check_expr: "error_pct <= error_max",
        parameters: r#"["i_bias","r_bias_eq","v_os","v_signal_min","error_max"]"#,
        output_params: r#"["v_offset_bias","v_offset_total","error_pct"]"#,
        source: "OpAmp: Voff = Ib*Rbias + Vos, check against min signal",
        domain: "analog",
        tags: &["analog", "opamp"],
    },
    RuleDef {
        name: "analog_filter_rc_lowpass",
        description: "RC low-pass filter cutoff frequency and attenuation",
        formula_expr: "f_c = 1 / (2 * pi() * r * c); atten_db = -10 * log10(1 + pow(f_signal / f_c, 2))",
        check_expr: "f_c >= f_pass_max; f_c <= f_stop_min",
        parameters: r#"["r","c","f_signal","f_pass_max","f_stop_min"]"#,
        output_params: r#"["f_c","atten_db"]"#,
        source: "RC LPF: fc = 1/(2*pi*R*C), atten = -10*log10(1+(f/fc)^2)",
        domain: "analog",
        tags: &["analog", "filter"],
    },
    RuleDef {
        name: "analog_filter_sallen_key",
        description: "Sallen-Key 2nd-order active low-pass filter",
        formula_expr: "f_c = 1 / (2 * pi() * sqrt(r1 * r2 * c1 * c2)); q = sqrt(r1 * r2 * c1 * c2) / (c2 * r1 + c2 * r2 - c1 * r2 * (gain - 1))",
        check_expr: "q >= 0.5; q <= 1.5",
        parameters: r#"["r1","r2","c1","c2","gain"]"#,
        output_params: r#"["f_c","q"]"#,
        source: "Sallen-Key: fc=1/(2*pi*sqrt(R1*R2*C1*C2)), Q from component ratios",
        domain: "analog",
        tags: &["analog", "filter"],
    },
    RuleDef {
        name: "analog_comparator_threshold",
        description: "Comparator threshold voltage with hysteresis",
        formula_expr: "v_th_high = v_ref * (r1 + r2) / r2; v_th_low = v_ref * (r1 + r2) / (r2 + r1 * r2 / r3); v_hyst = v_th_high - v_th_low",
        check_expr: "v_hyst >= v_noise_min; v_th_low >= v_signal_min",
        parameters: r#"["v_ref","r1","r2","r3","v_noise_min","v_signal_min"]"#,
        output_params: r#"["v_th_high","v_th_low","v_hyst"]"#,
        source: "Comparator: hysteresis via R3 feedback, Vth± from resistor network",
        domain: "analog",
        tags: &["analog", "comparator"],
    },
    // ── B5: Battery Management Skills ──────────────────────────────────
    RuleDef {
        name: "battery_charge_cc",
        description: "Li-ion CC charge current and resistor selection",
        formula_expr: "r_sense = v_sense / i_charge; p_sense = i_charge * i_charge * r_sense; t_charge_cc = (capacity * (1 - soc_start) - capacity * 0.1) / i_charge * 1.1",
        check_expr: "i_charge <= capacity * c_rate_max; p_sense <= p_resistor_max",
        parameters: r#"["v_sense","i_charge","capacity","soc_start","c_rate_max","p_resistor_max"]"#,
        output_params: r#"["r_sense","p_sense","t_charge_cc"]"#,
        source: "Li-ion CC: Rsense=Vsense/Ich, Ich<=Capacity*Crate, CC phase ~90% of charge",
        domain: "battery",
        tags: &["battery", "charging"],
    },
    RuleDef {
        name: "battery_charge_cv",
        description: "Li-ion CV phase termination current and total charge time",
        formula_expr: "i_cutoff = capacity * cutoff_c_rate; t_charge_total = capacity * (1 - soc_start) / i_charge * 1.4",
        check_expr: "i_cutoff >= i_cutoff_min; t_charge_total <= t_charge_max",
        parameters: r#"["capacity","soc_start","i_charge","cutoff_c_rate","i_cutoff_min","t_charge_max"]"#,
        output_params: r#"["i_cutoff","t_charge_total"]"#,
        source: "Li-ion CV: Icutoff = Cap*Crate_min, total time ~1.4x CC time",
        domain: "battery",
        tags: &["battery", "charging"],
    },
    RuleDef {
        name: "battery_discharge_runtime",
        description: "Battery runtime from capacity and load current",
        formula_expr: "t_runtime = capacity * dod * v_nominal / p_load; i_avg = p_load / v_nominal; t_runtime_alt = capacity * dod / i_avg",
        check_expr: "i_avg <= i_discharge_max; t_runtime >= t_runtime_min",
        parameters: r#"["capacity","dod","v_nominal","p_load","i_discharge_max","t_runtime_min"]"#,
        output_params: r#"["t_runtime","i_avg","t_runtime_alt"]"#,
        source: "Runtime = Capacity*DoD*Vnom / Pload, check against max discharge current",
        domain: "battery",
        tags: &["battery", "discharge"],
    },
    RuleDef {
        name: "battery_protection_ocv",
        description: "Battery over-charge and over-discharge voltage thresholds",
        formula_expr: "v_cell_max = n_cells * v_cell_full; v_cell_min = n_cells * v_cell_empty; v_overcharge = v_cell_max + v_margin; v_overdischarge = v_cell_min - v_margin",
        check_expr: "v_overcharge <= v_abs_max; v_overdischarge >= v_abs_min",
        parameters: r#"["n_cells","v_cell_full","v_cell_empty","v_margin","v_abs_max","v_abs_min"]"#,
        output_params: r#"["v_cell_max","v_cell_min","v_overcharge","v_overdischarge"]"#,
        source: "Battery OVP/UVP: thresholds from cell count and chemistry, with safety margin",
        domain: "battery",
        tags: &["battery", "protection"],
    },
    RuleDef {
        name: "battery_protection_temp",
        description: "Battery temperature protection thresholds for charge/discharge",
        formula_expr: "t_charge_high_margin = t_charge_max - t_margin; t_charge_low_margin = t_charge_min + t_margin; t_discharge_high_margin = t_discharge_max - t_margin; t_discharge_low_margin = t_discharge_min + t_margin",
        check_expr: "t_charge_high_margin > t_charge_low_margin; t_discharge_high_margin > t_discharge_low_margin",
        parameters: r#"["t_charge_max","t_charge_min","t_discharge_max","t_discharge_min","t_margin"]"#,
        output_params: r#"["t_charge_high_margin","t_charge_low_margin","t_discharge_high_margin","t_discharge_low_margin"]"#,
        source: "Li-ion: charge 0-45°C, discharge -20-60°C (typical), with margin",
        domain: "battery",
        tags: &["battery", "protection"],
    },
    // ── C2: Feedback Network Rules ─────────────────────────────────────
    RuleDef {
        name: "feedback_resistor_divider",
        description: "Feedback voltage divider: Rfb1 = Rfb2 * (Vout/Vref - 1)",
        formula_expr: "r_fb1 = r_fb2 * (vout / vref - 1); i_fb = vref / r_fb2; p_fb1 = i_fb * i_fb * r_fb1; p_fb2 = i_fb * i_fb * r_fb2",
        check_expr: "r_fb1 >= 1000; r_fb1 <= 1000000; i_fb >= 1e-6",
        parameters: r#"["vout","vref","r_fb2"]"#,
        output_params: r#"["r_fb1","i_fb","p_fb1","p_fb2"]"#,
        source: "Vout = Vref*(1+Rfb1/Rfb2), choose Rfb2 then solve for Rfb1",
        domain: "power",
        tags: &["power", "feedback", "resistor"],
    },
    // ── Thermal Design Rules ──────────────────────────────────────────
    RuleDef {
        name: "thermal_power_dissipation",
        description: "Power dissipation from efficiency: Pdiss = Pin * (1 - efficiency) = (Vout * Iout / efficiency) * (1 - efficiency)",
        formula_expr: "p_in = vout * iout / efficiency; p_diss = p_in * (1 - efficiency); p_out = vout * iout",
        check_expr: "p_diss > 0; p_diss < p_in",
        parameters: r#"["vout","iout","efficiency"]"#,
        output_params: r#"["p_in","p_diss","p_out"]"#,
        source: "Pdiss = Pin - Pout = (Vout*Iout/η) * (1-η)",
        domain: "thermal",
        tags: &["thermal", "power", "dissipation"],
    },
    RuleDef {
        name: "thermal_junction_temp",
        description: "Junction temperature estimate: Tj = Ta + Pdiss * θja (with/without heatsink)",
        formula_expr: "theta_ja = theta_jc + theta_cs + theta_sa; tj_no_heatsink = ta + p_diss * theta_ja_pcb; tj_heatsink = ta + p_diss * theta_ja; thermal_margin = tj_max - tj_heatsink",
        check_expr: "tj_no_heatsink < tj_max; tj_heatsink < tj_max",
        parameters: r#"["ta","p_diss","theta_jc","theta_cs","theta_sa","theta_ja_pcb","tj_max"]"#,
        output_params: r#"["theta_ja","tj_no_heatsink","tj_heatsink","thermal_margin"]"#,
        source: "Tj = Ta + Pdiss * θja; θja = θjc + θcs + θsa (heatsink path) or θja_pcb (PCB only)",
        domain: "thermal",
        tags: &["thermal", "temperature", "junction"],
    },
    RuleDef {
        name: "thermal_copper_area",
        description: "Required copper area for heat dissipation: A = Pdiss / (h * ΔT) where h is convection coefficient",
        formula_expr: "delta_t = tj_target - ta; copper_area = p_diss / (h_conv * delta_t); copper_area_mm2 = copper_area * 1e6; copper_side_mm = sqrt(copper_area_mm2)",
        check_expr: "copper_area > 0; delta_t > 0; copper_area_mm2 < 2500",
        parameters: r#"["p_diss","ta","tj_target","h_conv"]"#,
        output_params: r#"["delta_t","copper_area","copper_area_mm2","copper_side_mm"]"#,
        source: "A = P/(h·ΔT); h_conv ≈ 10-25 W/(m²·K) for natural convection on PCB",
        domain: "thermal",
        tags: &["thermal", "copper", "area", "pcb"],
    },
    RuleDef {
        name: "thermal_via_resistance",
        description: "Thermal via array resistance: θvia = t / (N * k_cu * A_via) where A_via = π*(r²-(r-t_cu)²)",
        formula_expr: "a_via = 3.14159 * (via_radius * via_radius - (via_radius - cu_thickness) * (via_radius - cu_thickness)); theta_via = board_thickness / (via_count * k_copper * a_via); thermal_resistance_improvement = theta_ja_pcb - theta_via",
        check_expr: "via_count >= 1; a_via > 0; theta_via > 0",
        parameters: r#"["board_thickness","via_radius","cu_thickness","via_count","k_copper","theta_ja_pcb"]"#,
        output_params: r#"["a_via","theta_via","thermal_resistance_improvement"]"#,
        source: "θvia = t/(N·k·A); k_copper ≈ 385 W/(m·K); typical via: 0.3mm radius, 35μm plating",
        domain: "thermal",
        tags: &["thermal", "via", "resistance"],
    },
    RuleDef {
        name: "thermal_heatsink_required",
        description: "Determine if heatsink is needed: compare PCB-only θja vs required θja_max = (Tj_max - Ta) / Pdiss",
        formula_expr: "theta_ja_required = (tj_max - ta) / p_diss; heatsink_needed = theta_ja_pcb > theta_ja_required; theta_sa_required = theta_ja_required - theta_jc - theta_cs",
        check_expr: "theta_ja_required > 0; theta_sa_required > 0",
        parameters: r#"["tj_max","ta","p_diss","theta_jc","theta_cs","theta_ja_pcb"]"#,
        output_params: r#"["theta_ja_required","heatsink_needed","theta_sa_required"]"#,
        source: "θja_max = (Tj_max - Ta)/Pdiss; if θja_pcb > θja_max, heatsink needed with θsa ≤ θsa_required",
        domain: "thermal",
        tags: &["thermal", "heatsink", "requirement"],
    },
];

impl ComponentDb {
    /// Insert default design rules (idempotent: skips if rule already exists)
    pub fn seed_default_rules(&self) -> Result<usize> {
        let mut count = 0;
        for def in DEFAULT_RULES {
            let exists: bool = self.conn.query_row(
                "SELECT COUNT(*) > 0 FROM design_rules WHERE name = ?1",
                params![def.name],
                |row| row.get(0),
            )?;

            if exists {
                // Update domain/tags for existing rules (migration path)
                let tags_json = serde_json::to_string(def.tags).unwrap_or_default();
                self.conn.execute(
                    "UPDATE design_rules SET domain = ?1, tags = ?2 WHERE name = ?3 AND domain IS NULL",
                    params![def.domain, tags_json, def.name],
                )?;
                continue;
            }

            let tags_json = serde_json::to_string(def.tags).unwrap_or_default();
            self.conn.execute(
                "INSERT INTO design_rules (name, description, formula_expr, check_expr, parameters, output_params, source, domain, tags)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![def.name, def.description, def.formula_expr, def.check_expr,
                        def.parameters, def.output_params, def.source, def.domain, tags_json],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Get all design rules
    pub fn get_all_design_rules(&self) -> Result<Vec<DesignRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, category_id, description, condition_expr, formula_expr, check_expr, parameters, output_params, source, domain, tags
             FROM design_rules ORDER BY name",
        )?;
        let rules = stmt
            .query_map([], |row| {
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
                    domain: row.get(10)?,
                    tags: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rules)
    }

    /// Get design rules filtered by domain
    pub fn get_rules_by_domain(&self, domain: &str) -> Result<Vec<DesignRule>> {
        let rules = self.get_all_design_rules()?;
        Ok(rules
            .into_iter()
            .filter(|r| r.domain.as_deref() == Some(domain))
            .collect())
    }

    /// Get design rules filtered by tag
    pub fn get_rules_by_tag(&self, tag: &str) -> Result<Vec<DesignRule>> {
        let rules = self.get_all_design_rules()?;
        Ok(rules
            .into_iter()
            .filter(|r| r.tags.as_ref().is_some_and(|t| t.contains(tag)))
            .collect())
    }
}

/// A topology candidate from suggestion engine
#[derive(Debug, serde::Serialize)]
pub struct TopologyCandidate {
    pub topology: String,
    pub estimated_efficiency: f64,
    pub score: f64,
    pub reason: String,
}

/// Suggest suitable topologies based on design requirements.
/// Returns candidates sorted by score (highest first).
pub fn suggest_topologies(
    vin: f64,
    vout: f64,
    iout: f64,
    isolated: bool,
) -> Vec<TopologyCandidate> {
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
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}
