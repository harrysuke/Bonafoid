//! A tiny expression evaluator used for gateway conditions and conditional
//! start events.
//!
//! Supported syntax:
//! - booleans: `true`, `false`, `null`
//! - identifiers resolved against the process variables (e.g. `amount`)
//! - string literals with single or double quotes
//! - numeric literals
//! - comparisons: `==`, `!=`, `>`, `<`, `>=`, `<=`
//! - boolean operators: `&&`, `||`, `!` and parentheses
//!
//! Values that reference an unknown variable evaluate to `null`.

use indexmap::IndexMap;
use serde_json::Value;

use crate::error::{BpmnError, Result};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Op(&'static str),
    LParen,
    RParen,
    And,
    Or,
    Not,
}

/// Evaluate a condition expression against a variable map.
///
/// An empty or whitespace-only expression is treated as `true`.
pub fn evaluate(expr: &str, vars: &IndexMap<String, Value>) -> Result<bool> {
    let src = expr.trim();
    if src.is_empty() {
        return Ok(true);
    }
    let tokens = tokenize(src)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        vars,
    };
    let result = parser.parse_or()?;
    if parser.pos != parser.tokens.len() {
        return Err(BpmnError::Expression(format!(
            "unexpected trailing token in `{expr}`"
        )));
    }
    match result {
        Value::Bool(b) => Ok(b),
        Value::Null => Ok(false),
        other => Ok(truthy(&other)),
    }
}

/// Evaluate an expression that must produce a scalar value.
pub fn evaluate_value(expr: &str, vars: &IndexMap<String, Value>) -> Result<Value> {
    let tokens = tokenize(expr.trim())?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        vars,
    };
    parser.parse_value_expr()
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        Value::Null => false,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn tokenize(src: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        match c {
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Op("!="));
                    i += 2;
                } else {
                    tokens.push(Token::Not);
                    i += 1;
                }
            }
            '=' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Op("=="));
                    i += 2;
                } else {
                    return Err(BpmnError::Expression("expected `==`".into()));
                }
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Op(">="));
                    i += 2;
                } else {
                    tokens.push(Token::Op(">"));
                    i += 1;
                }
            }
            '<' => {
                if chars.get(i + 1) == Some(&'=') {
                    tokens.push(Token::Op("<="));
                    i += 2;
                } else {
                    tokens.push(Token::Op("<"));
                    i += 1;
                }
            }
            '&' => {
                if chars.get(i + 1) == Some(&'&') {
                    tokens.push(Token::And);
                    i += 2;
                } else {
                    return Err(BpmnError::Expression("expected `&&`".into()));
                }
            }
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    tokens.push(Token::Or);
                    i += 2;
                } else {
                    return Err(BpmnError::Expression("expected `||`".into()));
                }
            }
            '\'' | '"' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != quote {
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err(BpmnError::Expression("unterminated string literal".into()));
                }
                i += 1;
                tokens.push(Token::Str(s));
            }
            c if c.is_ascii_digit() || (c == '-' && matches!(chars.get(i + 1), Some(d) if d.is_ascii_digit())) => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let num = text
                    .parse::<f64>()
                    .map_err(|_| BpmnError::Expression(format!("invalid number `{text}`")))?;
                tokens.push(Token::Number(num));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.'))
                {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match text.as_str() {
                    "true" => tokens.push(Token::Bool(true)),
                    "false" => tokens.push(Token::Bool(false)),
                    "null" => tokens.push(Token::Null),
                    "and" => tokens.push(Token::And),
                    "or" => tokens.push(Token::Or),
                    "not" => tokens.push(Token::Not),
                    _ => tokens.push(Token::Ident(text)),
                }
            }
            other => {
                return Err(BpmnError::Expression(format!(
                    "unexpected character `{other}`"
                )))
            }
        }
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    vars: &'a IndexMap<String, Value>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn parse_or(&mut self) -> Result<Value> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.next();
            let right = self.parse_and()?;
            left = Value::Bool(truthy(&left) || truthy(&right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Value> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.next();
            let right = self.parse_not()?;
            left = Value::Bool(truthy(&left) && truthy(&right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Value> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.next();
            let value = self.parse_not()?;
            return Ok(Value::Bool(!truthy(&value)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Value> {
        let left = self.parse_primary()?;
        if let Some(Token::Op(op)) = self.peek().cloned() {
            self.next();
            let right = self.parse_primary()?;
            return Ok(Value::Bool(compare(&left, op, &right)));
        }
        Ok(left)
    }

    /// Parse a value expression without comparison handling.
    fn parse_value_expr(&mut self) -> Result<Value> {
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Value> {
        match self.next() {
            Some(Token::Number(n)) => Ok(serde_json::json!(n)),
            Some(Token::Str(s)) => Ok(Value::String(s)),
            Some(Token::Bool(b)) => Ok(Value::Bool(b)),
            Some(Token::Null) => Ok(Value::Null),
            Some(Token::Ident(name)) => Ok(lookup(&name, self.vars)),
            Some(Token::LParen) => {
                let value = self.parse_or()?;
                match self.next() {
                    Some(Token::RParen) => Ok(value),
                    _ => Err(BpmnError::Expression("expected `)`".into())),
                }
            }
            other => Err(BpmnError::Expression(format!(
                "unexpected token {other:?}"
            ))),
        }
    }
}

fn lookup(name: &str, vars: &IndexMap<String, Value>) -> Value {
    if let Some(value) = vars.get(name) {
        return value.clone();
    }
    // Support dotted paths into objects.
    let mut parts = name.split('.');
    if let Some(first) = parts.next() {
        if let Some(mut current) = vars.get(first).cloned() {
            for part in parts {
                match current {
                    Value::Object(ref map) => {
                        current = map.get(part).cloned().unwrap_or(Value::Null);
                    }
                    _ => return Value::Null,
                }
            }
            return current;
        }
    }
    Value::Null
}

fn compare(left: &Value, op: &str, right: &Value) -> bool {
    match op {
        "==" => values_equal(left, right),
        "!=" => !values_equal(left, right),
        ">" | "<" | ">=" | "<=" => {
            let (l, r) = match (as_number(left), as_number(right)) {
                (Some(l), Some(r)) => (l, r),
                _ => return false,
            };
            match op {
                ">" => l > r,
                "<" => l < r,
                ">=" => l >= r,
                "<=" => l <= r,
                _ => false,
            }
        }
        _ => false,
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(_), Value::Number(_)) => as_number(left) == as_number(right),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Null, Value::Null) => true,
        // Allow string/number coercion for convenience.
        _ => match (as_number(left), as_number(right)) {
            (Some(a), Some(b)) => a == b,
            _ => left == right,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> IndexMap<String, Value> {
        let mut vars = IndexMap::new();
        vars.insert("amount".to_string(), serde_json::json!(150));
        vars.insert("status".to_string(), serde_json::json!("approved"));
        vars.insert("vip".to_string(), serde_json::json!(true));
        vars.insert(
            "customer".to_string(),
            serde_json::json!({"tier": "gold"}),
        );
        vars
    }

    #[test]
    fn empty_is_true() {
        assert!(evaluate("", &vars()).unwrap());
        assert!(evaluate("   ", &vars()).unwrap());
    }

    #[test]
    fn literals() {
        assert!(evaluate("true", &vars()).unwrap());
        assert!(!evaluate("false", &vars()).unwrap());
    }

    #[test]
    fn numeric_comparisons() {
        assert!(evaluate("amount > 100", &vars()).unwrap());
        assert!(!evaluate("amount > 200", &vars()).unwrap());
        assert!(evaluate("amount >= 150", &vars()).unwrap());
        assert!(evaluate("amount <= 150", &vars()).unwrap());
        assert!(evaluate("amount != 200", &vars()).unwrap());
    }

    #[test]
    fn string_comparisons() {
        assert!(evaluate("status == 'approved'", &vars()).unwrap());
        assert!(!evaluate("status == \"rejected\"", &vars()).unwrap());
    }

    #[test]
    fn boolean_operators() {
        assert!(evaluate("amount > 100 && status == 'approved'", &vars()).unwrap());
        assert!(evaluate("amount > 1000 || vip", &vars()).unwrap());
        assert!(!evaluate("!(amount > 100)", &vars()).unwrap());
        assert!(evaluate("(amount > 100 || false) && true", &vars()).unwrap());
    }

    #[test]
    fn unknown_variable_is_null() {
        assert!(!evaluate("missing == true", &vars()).unwrap());
        assert!(evaluate("missing == null", &vars()).unwrap());
    }

    #[test]
    fn dotted_paths() {
        assert!(evaluate("customer.tier == 'gold'", &vars()).unwrap());
    }

    #[test]
    fn rejects_malformed() {
        assert!(evaluate("amount >", &vars()).is_err());
        assert!(evaluate("amount > 100 &&", &vars()).is_err());
        assert!(evaluate("(amount > 100", &vars()).is_err());
    }

    #[test]
    fn evaluates_values() {
        assert_eq!(evaluate_value("amount", &vars()).unwrap(), serde_json::json!(150));
        assert_eq!(
            evaluate_value("'hello'", &vars()).unwrap(),
            serde_json::json!("hello")
        );
    }
}
