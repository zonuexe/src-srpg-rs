use serde::{Deserialize, Serialize};

/// Value type in SRC expressions.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum ValueType {
    #[default]
    /// Undefined variable or empty value.
    Undefined,
    /// String value.
    String(String),
    /// Numeric value (f64 internally).
    Numeric(f64),
}

impl ValueType {
    /// Check if this value is numeric.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Numeric(_))
    }

    /// Convert to f64. Returns 0.0 for non-numeric values.
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Numeric(v) => *v,
            Self::String(s) => s.parse::<f64>().unwrap_or(0.0),
            Self::Undefined => 0.0,
        }
    }

    /// Convert to i64. Truncates for numeric, 0 for non-numeric.
    pub fn as_i64(&self) -> i64 {
        self.as_f64() as i64
    }

    /// Convert to string representation.
    pub fn as_string(&self) -> String {
        match self {
            Self::Undefined => String::new(),
            Self::String(s) => s.clone(),
            Self::Numeric(v) => {
                if *v == v.floor() {
                    format!("{}", *v as i64)
                } else {
                    format!("{}", v)
                }
            }
        }
    }
}
