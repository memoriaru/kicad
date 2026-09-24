use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

use crate::DesignRule;

// ---------------------------------------------------------------------------
// Skill match result
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct SkillMatch {
    pub rule_name: String,
    pub description: String,
    pub domain: String,
    pub tags: Vec<String>,
    pub score: f64,
    pub matched_keywords: Vec<String>,
    pub extracted_params: HashMap<String, f64>,
    pub parameters: Vec<String>,
    pub output_params: Vec<String>,
}

// ---------------------------------------------------------------------------
// Keyword → domain/tags mapping
// ---------------------------------------------------------------------------

struct KeywordEntry {
    domain: &'static str,
    tags: &'static [&'static str],
    keywords: &'static [&'static str],
}

static KEYWORD_MAP: &[KeywordEntry] = &[
    // Power - Buck
    KeywordEntry {
        domain: "power",
        tags: &["buck"],
        keywords: &[
            "buck",
            "降压",
            "step-down",
            "stepdown",
            "降压转换",
            "buck converter",
        ],
    },
    // Power - Boost
    KeywordEntry {
        domain: "power",
        tags: &["boost"],
        keywords: &[
            "boost",
            "升压",
            "step-up",
            "stepup",
            "升压转换",
            "boost converter",
        ],
    },
    // Power - Buck-Boost
    KeywordEntry {
        domain: "power",
        tags: &["buck-boost"],
        keywords: &["buck-boost", "升降压", "buckboost"],
    },
    // Power - Inverting
    KeywordEntry {
        domain: "power",
        tags: &["inverting"],
        keywords: &[
            "inverting",
            "反相",
            "负压",
            "反相输出",
            "inverting converter",
        ],
    },
    // Power - SEPIC
    KeywordEntry {
        domain: "power",
        tags: &["sepic"],
        keywords: &["sepic", "sepic转换"],
    },
    // Power - Charge Pump
    KeywordEntry {
        domain: "power",
        tags: &["charge-pump"],
        keywords: &["charge pump", "电荷泵", "chargepump"],
    },
    // Power - Flyback
    KeywordEntry {
        domain: "power",
        tags: &["flyback"],
        keywords: &[
            "flyback",
            "反激",
            "反激式",
            "隔离电源",
            "隔离转换",
            "flyback converter",
        ],
    },
    // Power - LDO
    KeywordEntry {
        domain: "power",
        tags: &["ldo"],
        keywords: &[
            "ldo",
            "线性稳压",
            "低压差",
            "线性电源",
            "ldo regulator",
            "dropout",
            "压差",
        ],
    },
    // Power - LED
    KeywordEntry {
        domain: "power",
        tags: &["led"],
        keywords: &["led", "发光二极管", "限流电阻", "led驱动"],
    },
    // Power - general
    KeywordEntry {
        domain: "power",
        tags: &[],
        keywords: &[
            "电源",
            "power supply",
            "dc-dc",
            "dcdc",
            "转换器",
            "converter",
            "regulator",
            "稳压器",
            "开关电源",
            "效率",
            "efficiency",
            "占空比",
            "duty cycle",
        ],
    },
    // Capacitor/Inductor
    KeywordEntry {
        domain: "power",
        tags: &["capacitor", "passive"],
        keywords: &[
            "电容",
            "capacitor",
            "cap",
            "去耦",
            "滤波电容",
            "输出电容",
            "输入电容",
        ],
    },
    KeywordEntry {
        domain: "power",
        tags: &["inductor", "passive"],
        keywords: &["电感", "inductor", "inductance", "饱和电流", "纹波电流"],
    },
    // Thermal
    KeywordEntry {
        domain: "thermal",
        tags: &["thermal"],
        keywords: &[
            "热",
            "thermal",
            "功耗",
            "power dissipation",
            "结温",
            "junction temperature",
            "散热",
            "温升",
        ],
    },
    // SI
    KeywordEntry {
        domain: "si",
        tags: &["signal-integrity", "impedance"],
        keywords: &[
            "阻抗",
            "impedance",
            "微带",
            "microstrip",
            "带状线",
            "stripline",
            "信号完整性",
            "signal integrity",
            "si",
            "走线",
            "trace",
            "传输线",
            "transmission line",
            "特征阻抗",
        ],
    },
    KeywordEntry {
        domain: "si",
        tags: &[],
        keywords: &[
            "传播延迟",
            "propagation delay",
            "串扰",
            "crosstalk",
            "next",
            "载流",
            "current capacity",
            "过孔",
            "via",
            "走线电阻",
            "trace resistance",
        ],
    },
    // EMC
    KeywordEntry {
        domain: "emc",
        tags: &["emc", "filter"],
        keywords: &[
            "emc",
            "emi",
            "电磁兼容",
            "emc设计",
            "去耦",
            "decoupling",
            "esr",
            "谐振",
            "resonance",
            "滤波器",
            "filter",
            "rc滤波",
            "lc滤波",
            "共模",
            "common mode",
            "choke",
        ],
    },
    // Timing
    KeywordEntry {
        domain: "timing",
        tags: &["timing", "digital"],
        keywords: &[
            "时序",
            "timing",
            "建立时间",
            "setup time",
            "保持时间",
            "hold time",
            "时钟",
            "clock",
            "jitter",
            "抖动",
            "spi",
            "uart",
            "波特率",
            "baud rate",
            "频率",
            "frequency",
        ],
    },
];

// ---------------------------------------------------------------------------
// Number extraction patterns
// ---------------------------------------------------------------------------

fn extract_numbers(query: &str) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    let lower = query.to_lowercase();
    let query_lower = lower.as_str();

    // Voltage patterns: "5V", "3.3V", "12v", "vin=12"
    extract_voltage_param(query_lower, &mut params);

    // Current patterns: "2A", "500mA", "iout=2"
    extract_current_param(query_lower, &mut params);

    // Frequency patterns: "500kHz", "1MHz", "fsw=500000"
    extract_freq_param(query_lower, &mut params);

    params
}

fn extract_voltage_param(query: &str, params: &mut HashMap<String, f64>) {
    // Look for "XXV到YYV" or "XXV转YYV" or "XXV -> YYV" pattern
    // Pattern: "XV到YV" / "XV转YV" / "XV->YV"
    for sep in &["到", "转", "->", "→", " to "] {
        if let Some(idx) = query.find(sep) {
            let left = &query[..idx];
            let right = &query[idx + sep.len()..];

            if let (Some(vin), Some(vout)) = (find_last_voltage(left), find_first_voltage(right)) {
                if vin > vout {
                    params.insert("vin".into(), vin);
                    params.insert("vout".into(), vout);
                } else {
                    params.insert("vin".into(), vout);
                    params.insert("vout".into(), vin);
                }
                return;
            }
        }
    }

    // Named params: vin=12, vout=3.3
    if let Some(val) = find_named_value(query, "vin") {
        params.insert("vin".into(), val);
    }
    if let Some(val) = find_named_value(query, "vout") {
        params.insert("vout".into(), val);
    }

    // Standalone voltages
    let volts = find_all_voltages(query);
    if volts.len() == 2 && !params.contains_key("vin") {
        let (v1, v2) = (volts[0], volts[1]);
        if v1 > v2 {
            params.insert("vin".into(), v1);
            params.insert("vout".into(), v2);
        } else {
            params.insert("vin".into(), v2);
            params.insert("vout".into(), v1);
        }
    }
}

fn extract_current_param(query: &str, params: &mut HashMap<String, f64>) {
    if let Some(val) = find_named_value(query, "iout") {
        params.insert("iout".into(), val);
        return;
    }

    // "2A" / "500mA" / "100ma"
    for (i, c) in query.char_indices() {
        if c == 'a' || c == 'A' {
            let before = &query[..i].trim_end_matches(' ');
            if let Some(val) = before
                .rsplit(|c: char| !c.is_ascii_digit() && c != '.' && c != 'm' && c != 'M')
                .next()
            {
                let val_lower = val.to_lowercase();
                if val_lower.ends_with('m') || val_lower.ends_with("ma") {
                    if let Ok(v) = val_lower
                        .trim_end_matches('m')
                        .trim_end_matches("ma")
                        .parse::<f64>()
                    {
                        params.insert("iout".into(), v / 1000.0);
                        return;
                    }
                } else if let Ok(v) = val.parse::<f64>() {
                    params.insert("iout".into(), v);
                    return;
                }
            }
        }
    }
}

fn extract_freq_param(query: &str, params: &mut HashMap<String, f64>) {
    if let Some(val) = find_named_value(query, "fsw") {
        params.insert("fsw".into(), val);
        return;
    }

    let lower = query.to_lowercase();
    if let Some(idx) = lower.find("khz") {
        let before = &lower[..idx].trim_end_matches(' ');
        if let Some(val_str) = before
            .rsplit(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
        {
            if let Ok(v) = val_str.parse::<f64>() {
                params.insert("fsw".into(), v * 1e3);
                return;
            }
        }
    }
    if let Some(idx) = lower.find("mhz") {
        let before = &lower[..idx].trim_end_matches(' ');
        if let Some(val_str) = before
            .rsplit(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
        {
            if let Ok(v) = val_str.parse::<f64>() {
                params.insert("fsw".into(), v * 1e6);
            }
        }
    }
}

#[allow(dead_code)]
fn regex_lazy() {}

fn find_last_voltage(s: &str) -> Option<f64> {
    let volts = find_all_voltages(s);
    volts.last().copied()
}

fn find_first_voltage(s: &str) -> Option<f64> {
    let volts = find_all_voltages(s);
    volts.first().copied()
}

fn find_all_voltages(s: &str) -> Vec<f64> {
    let mut results = Vec::new();
    let lower = s.to_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'v' {
            // Look backwards for number
            let mut j = i;
            while j > 0 && (bytes[j - 1].is_ascii_digit() || bytes[j - 1] == b'.') {
                j -= 1;
            }
            if j < i {
                if let Ok(v) = lower[j..i].parse::<f64>() {
                    results.push(v);
                }
            }
        }
        i += 1;
    }
    results
}

fn find_named_value(query: &str, name: &str) -> Option<f64> {
    let lower = query.to_lowercase().replace(' ', "");
    let pattern = format!("{}=", name);
    if let Some(idx) = lower.find(&pattern) {
        let rest = &lower[idx + pattern.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != 'e' && c != 'E')
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Matching engine
// ---------------------------------------------------------------------------

/// Match a natural language query against available design rules.
pub fn match_skills(query: &str, rules: &[DesignRule]) -> Result<Vec<SkillMatch>> {
    let lower = query.to_lowercase();
    let extracted_params = extract_numbers(query);

    let mut matches: Vec<SkillMatch> = rules
        .iter()
        .map(|rule| {
            let rule_domain = rule.domain.as_deref().unwrap_or("");
            let rule_tags: Vec<String> = rule
                .tags
                .as_ref()
                .and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
                .unwrap_or_default();
            let rule_params: Vec<String> = rule
                .parameters
                .as_ref()
                .and_then(|p| serde_json::from_str::<Vec<String>>(p).ok())
                .unwrap_or_default();
            let rule_outputs: Vec<String> = rule
                .output_params
                .as_ref()
                .and_then(|p| serde_json::from_str::<Vec<String>>(p).ok())
                .unwrap_or_default();

            let mut score = 0.0;
            let mut matched_keywords = Vec::new();

            // Keyword matching
            for entry in KEYWORD_MAP {
                for kw in entry.keywords {
                    if lower.contains(kw) {
                        let mut keyword_score = 2.0;

                        // Domain bonus
                        if !rule_domain.is_empty() && rule_domain == entry.domain {
                            keyword_score += 3.0;
                        }

                        // Tag overlap bonus
                        let tag_overlap = entry
                            .tags
                            .iter()
                            .filter(|t| rule_tags.iter().any(|rt| rt == **t))
                            .count() as f64;
                        keyword_score += tag_overlap * 1.5;

                        if keyword_score > 0.0 {
                            matched_keywords.push(kw.to_string());
                            score += keyword_score;
                        }
                    }
                }
            }

            // Rule name keyword matching (e.g. "buck" in "buck_inductor_selection")
            let name_parts: Vec<&str> = rule.name.split('_').collect();
            for part in &name_parts {
                if lower.contains(part) && part.len() > 2 {
                    score += 1.0;
                    if !matched_keywords.contains(&part.to_string()) {
                        matched_keywords.push(part.to_string());
                    }
                }
            }

            // Description matching
            if let Some(ref desc) = rule.description {
                let desc_lower = desc.to_lowercase();
                for word in lower.split_whitespace() {
                    if word.len() > 2 && desc_lower.contains(word) {
                        score += 0.5;
                    }
                }
            }

            // Parameter extractability bonus
            let param_coverage = rule_params
                .iter()
                .filter(|p| extracted_params.contains_key(p.as_str()))
                .count() as f64;
            let total_params = rule_params.len().max(1) as f64;
            score += (param_coverage / total_params) * 2.0;

            // Deduplicate matched keywords
            matched_keywords.sort();
            matched_keywords.dedup();

            SkillMatch {
                rule_name: rule.name.clone(),
                description: rule.description.clone().unwrap_or_default(),
                domain: rule_domain.to_string(),
                tags: rule_tags,
                score,
                matched_keywords,
                extracted_params: extracted_params.clone(),
                parameters: rule_params,
                output_params: rule_outputs,
            }
        })
        .filter(|m| m.score > 0.0)
        .collect();

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_voltage_buck() {
        let params = extract_numbers("5V到3.3V降压电源");
        assert_eq!(params.get("vin"), Some(&5.0));
        assert_eq!(params.get("vout"), Some(&3.3));
    }

    #[test]
    fn test_extract_current() {
        let params = extract_numbers("输出2A");
        assert_eq!(params.get("iout"), Some(&2.0));
    }

    #[test]
    fn test_extract_current_ma() {
        let params = extract_numbers("500mA输出");
        assert_eq!(params.get("iout"), Some(&0.5));
    }

    #[test]
    fn test_extract_freq_khz() {
        let params = extract_numbers("开关频率500kHz");
        assert_eq!(params.get("fsw"), Some(&500e3));
    }

    #[test]
    fn test_extract_named_params() {
        let params = extract_numbers("vin=12,vout=3.3,iout=2,fsw=500000");
        assert_eq!(params.get("vin"), Some(&12.0));
        assert_eq!(params.get("vout"), Some(&3.3));
        assert_eq!(params.get("iout"), Some(&2.0));
        assert_eq!(params.get("fsw"), Some(&500000.0));
    }

    #[test]
    fn test_match_buck_query() {
        let rules = vec![DesignRule {
            id: Some(1),
            name: "buck_inductor_selection".into(),
            category_id: Some(1),
            description: Some("Calculate minimum inductance for buck converter".into()),
            condition_expr: None,
            formula_expr: Some(
                "l_min = (vout * (1 - vout / vin)) / (fsw * ripple_ratio * iout)".into(),
            ),
            check_expr: Some("L_value >= l_min * 0.8".into()),
            parameters: Some(r#"["vin", "vout", "iout", "fsw", "ripple_ratio"]"#.into()),
            output_params: Some(r#"["l_min"]"#.into()),
            source: None,
            domain: Some("power".into()),
            tags: Some(r#"["buck", "dc-dc"]"#.into()),
        }];

        let matches = match_skills("5V到3.3V降压电源2A", &rules).unwrap();
        assert!(!matches.is_empty());
        assert!(matches[0].score > 0.0);
        assert_eq!(matches[0].rule_name, "buck_inductor_selection");
        assert!(matches[0].extracted_params.contains_key("vin"));
    }

    #[test]
    fn test_match_si_query() {
        let rules = vec![DesignRule {
            id: Some(2),
            name: "si_microstrip_impedance".into(),
            category_id: None,
            description: Some("Microstrip impedance calculation (IPC-2141A)".into()),
            condition_expr: None,
            formula_expr: None,
            check_expr: None,
            parameters: Some(r#"["epsilon_r", "h", "w", "z_min", "z_max"]"#.into()),
            output_params: Some(r#"["z0"]"#.into()),
            source: None,
            domain: Some("si".into()),
            tags: Some(r#"["signal-integrity", "impedance"]"#.into()),
        }];

        let matches = match_skills("高速信号走线阻抗控制", &rules).unwrap();
        assert!(!matches.is_empty());
        assert_eq!(matches[0].rule_name, "si_microstrip_impedance");
    }
}
