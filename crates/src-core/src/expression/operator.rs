/// Operator types in SRC expressions, ordered by precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorType {
    /// Exponentiation `^` (highest precedence)
    Power,
    /// Multiplication / Division `* /`
    MulDiv,
    /// Addition / Subtraction `+ -`
    AddSub,
    /// String concatenation `&`
    Concat,
    /// Comparison `= <> < > <= >=`
    Comparison,
    /// Logical NOT `Not`
    Not,
    /// Logical AND `And`
    And,
    /// Logical OR `Or` (lowest precedence)
    Or,
}

impl OperatorType {
    /// Precedence level (higher = binds tighter).
    pub fn precedence(self) -> u8 {
        match self {
            Self::Power => 7,
            Self::MulDiv => 6,
            Self::AddSub => 5,
            Self::Concat => 4,
            Self::Comparison => 3,
            Self::Not => 2,
            Self::And => 1,
            Self::Or => 0,
        }
    }

    /// Parse operator from string.
    pub fn parse_token(s: &str) -> Option<Self> {
        match s {
            "^" => Some(Self::Power),
            "*" | "/" => Some(Self::MulDiv),
            "+" | "-" => Some(Self::AddSub),
            "&" => Some(Self::Concat),
            "=" | "<>" | "<" | ">" | "<=" | ">=" => Some(Self::Comparison),
            "And" | "and" | "AND" => Some(Self::And),
            "Or" | "or" | "OR" => Some(Self::Or),
            "Not" | "not" | "NOT" => Some(Self::Not),
            _ => None,
        }
    }
}
