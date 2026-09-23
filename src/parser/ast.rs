//! AST (Abstract Syntax Tree) for S-expression

use std::fmt;

/// S-expression AST node
#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    /// A list of S-expressions: (item1 item2 ...)
    List(Vec<SExpr>),
    /// An atom (identifier, string, number, or boolean)
    Atom(Atom),
}

/// Atom types in S-expression
#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    /// String value
    String(String),
    /// Number value
    Number(f64),
    /// Boolean value
    Bool(bool),
    /// Identifier (symbol name)
    Identifier(String),
}

impl SExpr {
    /// Create a new list from items
    pub fn list(items: Vec<SExpr>) -> Self {
        SExpr::List(items)
    }

    /// Create an identifier atom
    pub fn ident(s: impl Into<String>) -> Self {
        SExpr::Atom(Atom::Identifier(s.into()))
    }

    /// Create a string atom
    pub fn string(s: impl Into<String>) -> Self {
        SExpr::Atom(Atom::String(s.into()))
    }

    /// Create a number atom
    pub fn number(n: f64) -> Self {
        SExpr::Atom(Atom::Number(n))
    }

    /// Create a boolean atom
    pub fn bool(b: bool) -> Self {
        SExpr::Atom(Atom::Bool(b))
    }

    /// Check if this is a list
    pub fn is_list(&self) -> bool {
        matches!(self, SExpr::List(_))
    }

    /// Check if this is an identifier with specific name
    pub fn is_ident(&self, name: &str) -> bool {
        match self {
            SExpr::Atom(Atom::Identifier(s)) => s == name,
            _ => false,
        }
    }

    /// Get as list
    pub fn as_list(&self) -> Option<&[SExpr]> {
        match self {
            SExpr::List(items) => Some(items),
            _ => None,
        }
    }

    /// Get as string
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SExpr::Atom(Atom::String(s)) => Some(s),
            _ => None,
        }
    }

    /// Get as identifier
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            SExpr::Atom(Atom::Identifier(s)) => Some(s),
            _ => None,
        }
    }

    /// Get as number
    pub fn as_number(&self) -> Option<f64> {
        match self {
            SExpr::Atom(Atom::Number(n)) => Some(*n),
            _ => None,
        }
    }

    /// Get as boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SExpr::Atom(Atom::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// Get the first element of a list (head)
    pub fn head(&self) -> Option<&SExpr> {
        match self {
            SExpr::List(items) => items.first(),
            _ => None,
        }
    }

    /// Get all elements except the first (tail)
    pub fn tail(&self) -> Option<&[SExpr]> {
        match self {
            SExpr::List(items) if !items.is_empty() => Some(&items[1..]),
            _ => None,
        }
    }

    /// Find a property by name in a list of properties
    /// e.g., in ((reference "R1") (value "10k")), find "reference" returns "R1"
    pub fn find_property(&self, name: &str) -> Option<&SExpr> {
        match self {
            SExpr::List(items) => {
                for item in items {
                    if let SExpr::List(prop_items) = item {
                        if prop_items.len() >= 2 {
                            if prop_items[0].is_ident(name) {
                                return Some(&prop_items[1]);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Get property as string
    pub fn get_string_property(&self, name: &str) -> Option<&str> {
        self.find_property(name)?.as_string()
    }

    /// Get property as identifier
    pub fn get_ident_property(&self, name: &str) -> Option<&str> {
        self.find_property(name)?.as_ident()
    }

    /// Get property as number
    pub fn get_number_property(&self, name: &str) -> Option<f64> {
        self.find_property(name)?.as_number()
    }
}

impl fmt::Display for SExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SExpr::List(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, ")")
            }
            SExpr::Atom(atom) => match atom {
                Atom::String(s) => write!(f, "\"{}\"", s),
                Atom::Number(n) => write!(f, "{}", n),
                Atom::Bool(b) => write!(f, "{}", if *b { "yes" } else { "no" }),
                Atom::Identifier(s) => write!(f, "{}", s),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_construction() {
        let list = SExpr::list(vec![SExpr::ident("symbol"), SExpr::string("R1")]);
        assert!(list.is_list());
        assert!(list.is_ident("symbol") == false);
        assert_eq!(list.head().unwrap().as_ident(), Some("symbol"));
    }

    #[test]
    fn test_property_finding() {
        let props = SExpr::list(vec![
            SExpr::list(vec![SExpr::ident("reference"), SExpr::string("R1")]),
            SExpr::list(vec![SExpr::ident("value"), SExpr::string("10k")]),
        ]);

        assert_eq!(props.get_string_property("reference"), Some("R1"));
        assert_eq!(props.get_string_property("value"), Some("10k"));
        assert_eq!(props.get_string_property("footprint"), None);
    }
}
