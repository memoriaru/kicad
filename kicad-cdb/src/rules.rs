use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::models::DesignRule;

/// Result of applying a design rule
#[derive(Debug, Serialize)]
pub struct RuleResult {
    pub pass: bool,
    pub outputs: HashMap<String, f64>,
    pub check_expression: String,
}

/// Evaluation context for math expressions
pub struct EvalContext {
    vars: HashMap<String, f64>,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self::new()
    }
}

impl EvalContext {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    pub fn set(&mut self, name: &str, value: f64) {
        self.vars.insert(name.to_string(), value);
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        self.vars.get(name).copied()
    }

    /// Evaluate a simple math expression.
    /// Supports: +, -, *, /, parentheses, variable substitution.
    /// No function calls or complex operators.
    pub fn eval(&self, expr: &str) -> Result<f64> {
        let tokens = tokenize(expr)?;
        let (result, _) = parse_additive(&tokens, 0, &self.vars)?;
        Ok(result)
    }

    /// Evaluate an assignment formula like "l_min = (vout * (1 - vout / vin)) / (fsw * 0.3 * iout)"
    /// Returns all assigned variable names and their computed values.
    pub fn eval_formula(&mut self, formula: &str) -> Result<Vec<(String, f64)>> {
        let parts: Vec<&str> = formula.splitn(2, '=').collect();
        if parts.len() != 2 {
            bail!("Formula must contain '=' assignment: {}", formula);
        }

        let var_name = parts[0].trim().to_string();
        let expr = parts[1].trim();

        let value = self.eval(expr)?;
        self.vars.insert(var_name.clone(), value);

        Ok(vec![(var_name, value)])
    }

    /// Evaluate a check expression like "L_value >= l_min * 0.8"
    /// Supports: >=, <=, >, <, ==, !=
    pub fn eval_check(&self, check: &str) -> Result<bool> {
        let (op_pos, op) = find_comparison_op(check)
            .context(format!("No comparison operator found in: {}", check))?;

        let left_expr = check[..op_pos].trim();
        let op_len = op.len();
        let right_expr = check[op_pos + op_len..].trim();

        let left = self.eval(left_expr)?;
        let right = self.eval(right_expr)?;

        let result = match op {
            ">=" => left >= right,
            "<=" => left <= right,
            ">" => left > right,
            "<" => left < right,
            "==" => (left - right).abs() < f64::EPSILON,
            "!=" => (left - right).abs() >= f64::EPSILON,
            _ => bail!("Unknown operator: {}", op),
        };

        Ok(result)
    }
}

/// Token types for expression parser
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
}

fn tokenize(expr: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' => {
                i += 1;
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || chars[i] == '.'
                        || chars[i] == 'e'
                        || chars[i] == 'E'
                        || (chars[i] == '-'
                            && i > 0
                            && (chars[i - 1] == 'e' || chars[i - 1] == 'E')))
                {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num: f64 = num_str
                    .parse()
                    .context(format!("Failed to parse number: {}", num_str))?;
                tokens.push(Token::Number(num));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push(Token::Ident(ident));
            }
            _ => bail!("Unexpected character '{}' in expression", chars[i]),
        }
    }

    Ok(tokens)
}

/// Parse additive: expr (+|-) expr
fn parse_additive(
    tokens: &[Token],
    pos: usize,
    vars: &HashMap<String, f64>,
) -> Result<(f64, usize)> {
    let (mut result, mut pos) = parse_multiplicative(tokens, pos, vars)?;

    while pos < tokens.len() {
        match tokens[pos] {
            Token::Plus => {
                let (right, new_pos) = parse_multiplicative(tokens, pos + 1, vars)?;
                result += right;
                pos = new_pos;
            }
            Token::Minus => {
                let (right, new_pos) = parse_multiplicative(tokens, pos + 1, vars)?;
                result -= right;
                pos = new_pos;
            }
            _ => break,
        }
    }

    Ok((result, pos))
}

/// Parse multiplicative: expr (*|/) expr
fn parse_multiplicative(
    tokens: &[Token],
    pos: usize,
    vars: &HashMap<String, f64>,
) -> Result<(f64, usize)> {
    let (mut result, mut pos) = parse_unary(tokens, pos, vars)?;

    while pos < tokens.len() {
        match tokens[pos] {
            Token::Star => {
                let (right, new_pos) = parse_unary(tokens, pos + 1, vars)?;
                result *= right;
                pos = new_pos;
            }
            Token::Slash => {
                let (right, new_pos) = parse_unary(tokens, pos + 1, vars)?;
                if right.abs() < f64::EPSILON {
                    bail!("Division by zero");
                }
                result /= right;
                pos = new_pos;
            }
            _ => break,
        }
    }

    Ok((result, pos))
}

/// Parse unary: -expr or +expr
fn parse_unary(tokens: &[Token], pos: usize, vars: &HashMap<String, f64>) -> Result<(f64, usize)> {
    if pos < tokens.len() {
        match tokens[pos] {
            Token::Minus => {
                let (val, new_pos) = parse_primary(tokens, pos + 1, vars)?;
                return Ok((-val, new_pos));
            }
            Token::Plus => {
                return parse_primary(tokens, pos + 1, vars);
            }
            _ => {}
        }
    }
    parse_primary(tokens, pos, vars)
}

/// Parse primary: number | variable | constant | function_call | (expr)
fn parse_primary(
    tokens: &[Token],
    pos: usize,
    vars: &HashMap<String, f64>,
) -> Result<(f64, usize)> {
    if pos >= tokens.len() {
        bail!("Unexpected end of expression");
    }

    match &tokens[pos] {
        Token::Number(n) => Ok((*n, pos + 1)),
        Token::Ident(name) => {
            // Check for built-in constants
            match name.as_str() {
                "pi" => return Ok((std::f64::consts::PI, pos + 1)),
                "e_base" => return Ok((std::f64::consts::E, pos + 1)),
                _ => {}
            }

            // Check for function call: name(...)
            if pos + 1 < tokens.len() && tokens[pos + 1] == Token::LParen {
                return parse_function_call(name, tokens, pos, vars);
            }

            // Variable lookup
            let val = vars
                .get(name.as_str())
                .copied()
                .context(format!("Undefined variable: {}", name))?;
            Ok((val, pos + 1))
        }
        Token::LParen => {
            let (result, new_pos) = parse_additive(tokens, pos + 1, vars)?;
            if new_pos >= tokens.len() || tokens[new_pos] != Token::RParen {
                bail!("Missing closing parenthesis");
            }
            Ok((result, new_pos + 1))
        }
        _ => bail!("Unexpected token at position {}: {:?}", pos, tokens[pos]),
    }
}

/// Parse a function call: name(arg1[, arg2])
fn parse_function_call(
    name: &str,
    tokens: &[Token],
    pos: usize,
    vars: &HashMap<String, f64>,
) -> Result<(f64, usize)> {
    // pos = function name, pos+1 = LParen
    let mut args = Vec::new();
    let mut cur = pos + 2; // skip name and LParen

    // Parse comma-separated arguments
    loop {
        let (val, new_pos) = parse_additive(tokens, cur, vars)?;
        args.push(val);
        cur = new_pos;

        if cur >= tokens.len() {
            bail!("Missing closing parenthesis in function call '{}'", name);
        }
        if tokens[cur] == Token::RParen {
            cur += 1; // skip RParen
            break;
        }
        if tokens[cur] == Token::Comma {
            cur += 1; // skip comma, parse next argument
        } else {
            bail!("Expected ')' or ',' in function call '{}'", name);
        }
    }

    // Evaluate function
    let nargs = args.len();
    let result = match name {
        "sqrt" if nargs == 1 => {
            if args[0] < 0.0 {
                bail!("sqrt() of negative number");
            }
            args[0].sqrt()
        }
        "ln" if nargs == 1 => {
            if args[0] <= 0.0 {
                bail!("ln() of non-positive number");
            }
            args[0].ln()
        }
        "log10" if nargs == 1 => {
            if args[0] <= 0.0 {
                bail!("log10() of non-positive number");
            }
            args[0].log10()
        }
        "abs" if nargs == 1 => args[0].abs(),
        "pow" if nargs == 2 => args[0].powf(args[1]),
        "exp" if nargs == 1 => args[0].exp(),
        _ if ["sqrt", "ln", "log10", "abs", "exp"].contains(&name) => {
            bail!("{}() requires 1 argument, got {}", name, nargs);
        }
        "pow" => {
            bail!("pow() requires 2 arguments, got {}", nargs);
        }
        _ => bail!("Unknown function: '{}'", name),
    };

    Ok((result, cur))
}

/// Find comparison operator position in a string
fn find_comparison_op(expr: &str) -> Option<(usize, &'static str)> {
    // Check multi-char operators first
    for op in [">=", "<=", "==", "!="] {
        if let Some(pos) = expr.find(op) {
            return Some((pos, op));
        }
    }
    // Single char operators — need to verify it's not part of >= <=
    for (i, c) in expr.char_indices() {
        if c == '>' && i + 1 < expr.len() && expr.chars().nth(i + 1) != Some('=') {
            return Some((i, ">"));
        }
        if c == '<' && i + 1 < expr.len() && expr.chars().nth(i + 1) != Some('=') {
            return Some((i, "<"));
        }
    }
    None
}

/// Parse a JSON array string like `["vin","vout"]` into Vec<String>
fn parse_json_array(s: &Option<String>) -> Vec<String> {
    s.as_ref()
        .and_then(|p| serde_json::from_str::<Vec<String>>(p).ok())
        .unwrap_or_default()
}

use crate::ComponentDb;

impl ComponentDb {
    /// Apply a design rule with given input parameters.
    /// Optionally check a candidate component value against the rule.
    /// Validates input completeness against declared `parameters` contract
    /// and output completeness against declared `output_params` contract.
    pub fn apply_rule(
        &self,
        rule: &DesignRule,
        inputs: &serde_json::Value,
        candidate_name: Option<&str>,
        candidate_value: Option<f64>,
    ) -> Result<RuleResult> {
        // --- Input contract validation ---
        let declared_params = parse_json_array(&rule.parameters);
        if !declared_params.is_empty() {
            let mut missing = Vec::new();
            for name in &declared_params {
                if inputs.get(name.as_str()).is_none() {
                    missing.push(name.clone());
                }
            }
            if !missing.is_empty() {
                bail!(
                    "Missing required inputs for '{}': {}",
                    rule.name,
                    missing.join(", ")
                );
            }
        }

        let mut ctx = EvalContext::new();

        // Load input parameters
        if let serde_json::Value::Object(map) = inputs {
            for (key, val) in map {
                if let Some(n) = val.as_f64() {
                    ctx.set(key, n);
                }
            }
        }

        // Set candidate value if provided
        if let (Some(name), Some(value)) = (candidate_name, candidate_value) {
            ctx.set(name, value);
        }

        // Evaluate condition gate (if present, rule is skipped when false)
        if let Some(condition) = &rule.condition_expr {
            if !condition.trim().is_empty() {
                let should_apply = ctx.eval_check(condition)?;
                if !should_apply {
                    return Ok(RuleResult {
                        pass: true,
                        outputs: HashMap::new(),
                        check_expression: format!("(skipped: condition '{}' not met)", condition),
                    });
                }
            }
        }

        // Evaluate formula(s) — semicolon-separated multi-assignment support
        let mut outputs = HashMap::new();
        if let Some(formula) = &rule.formula_expr {
            for stmt in formula.split(';') {
                let stmt = stmt.trim();
                if !stmt.is_empty() {
                    let results = ctx.eval_formula(stmt)?;
                    for (name, value) in results {
                        outputs.insert(name, value);
                    }
                }
            }
        }

        // --- Output contract validation ---
        let declared_outputs = parse_json_array(&rule.output_params);
        if !declared_outputs.is_empty() {
            let missing: Vec<String> = declared_outputs
                .iter()
                .filter(|name| !outputs.contains_key(name.as_str()))
                .cloned()
                .collect();
            if !missing.is_empty() {
                bail!(
                    "Rule '{}' declared outputs {} but formula did not produce: {}",
                    rule.name,
                    rule.output_params.as_deref().unwrap_or("[]"),
                    missing.join(", ")
                );
            }
        }

        // Evaluate check — semicolon-separated conditions (all must pass)
        let pass = if let Some(check) = &rule.check_expr {
            let mut all_pass = true;
            for cond in check.split(';') {
                let cond = cond.trim();
                if !cond.is_empty() && !ctx.eval_check(cond)? {
                    all_pass = false;
                    break;
                }
            }
            all_pass
        } else {
            true
        };

        Ok(RuleResult {
            pass,
            outputs,
            check_expression: rule.check_expr.clone().unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_sqrt() {
        let ctx = EvalContext::new();
        let result = ctx.eval("sqrt(4)").unwrap();
        assert!((result - 2.0).abs() < 1e-10);
        let result2 = ctx.eval("sqrt(2)").unwrap();
        assert!((result2 - std::f64::consts::SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn test_eval_ln() {
        let ctx = EvalContext::new();
        let result = ctx.eval("ln(e_base)").unwrap();
        assert!((result - 1.0).abs() < 1e-10);
        let result2 = ctx.eval("ln(10)").unwrap();
        assert!((result2 - std::f64::consts::LN_10).abs() < 1e-6);
    }

    #[test]
    fn test_eval_log10() {
        let ctx = EvalContext::new();
        let result = ctx.eval("log10(100)").unwrap();
        assert!((result - 2.0).abs() < 1e-10);
        let result2 = ctx.eval("log10(1000)").unwrap();
        assert!((result2 - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_pi() {
        let ctx = EvalContext::new();
        let result = ctx.eval("pi").unwrap();
        assert!((result - std::f64::consts::PI).abs() < 1e-10);
    }

    #[test]
    fn test_eval_pow() {
        let ctx = EvalContext::new();
        let result = ctx.eval("pow(2, 10)").unwrap();
        assert!((result - 1024.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_abs() {
        let ctx = EvalContext::new();
        let result = ctx.eval("abs(-5)").unwrap();
        assert!((result - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_nested_function() {
        let ctx = EvalContext::new();
        // sqrt(4) + abs(-3) = 2 + 3 = 5
        let result = ctx.eval("sqrt(4) + abs(-3)").unwrap();
        assert!((result - 5.0).abs() < 1e-10);
    }

    #[test]
    fn test_eval_function_in_formula() {
        let mut ctx = EvalContext::new();
        ctx.set("epsilon_r", 4.5);
        ctx.set("h", 0.2);
        ctx.set("w", 0.15);
        let results = ctx
            .eval_formula("z0 = (87 / sqrt(epsilon_r + 1.41)) * ln(5.98 * h / w)")
            .unwrap();
        let z0 = results[0].1;
        // Expected: (87/sqrt(5.91)) * ln(7.973) ≈ 35.76 * 2.076 ≈ 74.24
        assert!(z0 > 70.0 && z0 < 80.0, "z0 = {} (expected ~74)", z0);
    }

    #[test]
    fn test_eval_pi_in_formula() {
        let mut ctx = EvalContext::new();
        ctx.set("R", 1000.0);
        ctx.set("C", 1e-6);
        let results = ctx.eval_formula("fc = 1 / (2 * pi * R * C)").unwrap();
        let fc = results[0].1;
        assert!(
            (fc - 159.1549).abs() < 0.1,
            "fc = {} (expected ~159.15)",
            fc
        );
    }

    #[test]
    fn test_eval_function_with_expr_arg() {
        let ctx = EvalContext::new();
        // sqrt(3*3 + 4*4) = sqrt(25) = 5
        let result = ctx.eval("sqrt(3 * 3 + 4 * 4)").unwrap();
        assert!((result - 5.0).abs() < 1e-10);
    }
}
