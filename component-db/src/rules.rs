use std::collections::HashMap;

use anyhow::{bail, Context, Result};

use crate::models::DesignRule;

/// Result of applying a design rule
#[derive(Debug)]
pub struct RuleResult {
    pub pass: bool,
    pub outputs: HashMap<String, f64>,
    pub check_expression: String,
}

/// Evaluation context for math expressions
pub struct EvalContext {
    vars: HashMap<String, f64>,
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
}

fn tokenize(expr: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' => { i += 1; }
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Star); i += 1; }
            '/' => { tokens.push(Token::Slash); i += 1; }
            '(' => { tokens.push(Token::LParen); i += 1; }
            ')' => { tokens.push(Token::RParen); i += 1; }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E' || (chars[i] == '-' && i > 0 && (chars[i-1] == 'e' || chars[i-1] == 'E'))) {
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num: f64 = num_str.parse()
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
fn parse_additive(tokens: &[Token], pos: usize, vars: &HashMap<String, f64>) -> Result<(f64, usize)> {
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
fn parse_multiplicative(tokens: &[Token], pos: usize, vars: &HashMap<String, f64>) -> Result<(f64, usize)> {
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

/// Parse primary: number | variable | (expr)
fn parse_primary(tokens: &[Token], pos: usize, vars: &HashMap<String, f64>) -> Result<(f64, usize)> {
    if pos >= tokens.len() {
        bail!("Unexpected end of expression");
    }

    match &tokens[pos] {
        Token::Number(n) => Ok((*n, pos + 1)),
        Token::Ident(name) => {
            let val = vars.get(name.as_str())
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

use crate::ComponentDb;

impl ComponentDb {
    /// Apply a design rule with given input parameters.
    /// Optionally check a candidate component value against the rule.
    pub fn apply_rule(
        &self,
        rule: &DesignRule,
        inputs: &serde_json::Value,
        candidate_name: Option<&str>,
        candidate_value: Option<f64>,
    ) -> Result<RuleResult> {
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

        // Evaluate formula
        let mut outputs = HashMap::new();
        if let Some(formula) = &rule.formula_expr {
            let results = ctx.eval_formula(formula)?;
            for (name, value) in results {
                outputs.insert(name, value);
            }
        }

        // Evaluate check
        let pass = if let Some(check) = &rule.check_expr {
            ctx.eval_check(check)?
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
