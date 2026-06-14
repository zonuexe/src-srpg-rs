//! Function registry for the expression evaluator.
//!
//! Functions are registered at startup and can be called from expressions
//! like `Min(a, b)`, `Max(a, b)`, `Abs(x)`, `Len(s)`, etc.

use std::collections::HashMap;

mod math;
mod string;

pub use math::*;
pub use string::*;

use super::value_type::ValueType;

/// Trait for expression functions.
pub trait IFunction: Send + Sync {
    /// Invoke the function with the given arguments.
    fn invoke(&self, args: &[ValueType]) -> ValueType;
    /// The function's name (for registry lookup).
    fn name(&self) -> &str;
}

/// Global function registry.
pub struct FunctionRegistry {
    functions: HashMap<String, Box<dyn IFunction>>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            functions: HashMap::new(),
        };
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        self.register(Box::new(MinFn));
        self.register(Box::new(MaxFn));
        self.register(Box::new(AbsFn));
        self.register(Box::new(LenFn));
        // Math functions
        self.register(Box::new(RoundFn));
        self.register(Box::new(RoundUpFn));
        self.register(Box::new(RoundDownFn));
        self.register(Box::new(IntFn));
        self.register(Box::new(SqrFn));
        self.register(Box::new(SinFn));
        self.register(Box::new(CosFn));
        self.register(Box::new(TanFn));
        self.register(Box::new(AtnFn));
        self.register(Box::new(IIFFn));
        self.register(Box::new(RandomFn));
        self.register(Box::new(NotFn));
        self.register(Box::new(IsNumericFn));
        self.register(Box::new(RGBFn));
        self.register(Box::new(DirFn));
        self.register(Box::new(EvalFn));
        self.register(Box::new(IsDefinedFn));
        self.register(Box::new(IsVarDefinedFn));
        self.register(Box::new(ArgsFn));
        // String functions
        self.register(Box::new(LeftFn));
        self.register(Box::new(RightFn));
        self.register(Box::new(MidFn));
        self.register(Box::new(InStrFn));
        self.register(Box::new(InStrRevFn));
        self.register(Box::new(LCaseFn));
        self.register(Box::new(UCaseFn));
        self.register(Box::new(TrimFn));
        self.register(Box::new(AscFn));
        self.register(Box::new(ChrFn));
        self.register(Box::new(StringFn));
        self.register(Box::new(ReplaceFn));
        self.register(Box::new(StrCmpFn));
        self.register(Box::new(FormatFn));
        self.register(Box::new(WideFn));
        self.register(Box::new(LSetFn));
        self.register(Box::new(RSetFn));
    }

    /// Register a function.
    pub fn register(&mut self, func: Box<dyn IFunction>) {
        let name = func.name().to_string();
        self.functions.insert(name, func);
    }

    /// Look up a function by name (case-insensitive).
    pub fn lookup(&self, name: &str) -> Option<&dyn IFunction> {
        // Try exact match first
        if let Some(f) = self.functions.get(name) {
            return Some(f.as_ref());
        }
        // Try case-insensitive
        let lower = name.to_lowercase();
        for (key, func) in &self.functions {
            if key.to_lowercase() == lower {
                return Some(func.as_ref());
            }
        }
        None
    }

    /// Call a function by name.
    pub fn call(&self, name: &str, args: &[ValueType]) -> ValueType {
        if let Some(func) = self.lookup(name) {
            func.invoke(args)
        } else {
            ValueType::Undefined
        }
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// === Built-in functions ===

struct MinFn;
impl IFunction for MinFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let min = args
            .iter()
            .map(|v| v.as_f64())
            .fold(f64::INFINITY, f64::min);
        ValueType::Numeric(min)
    }
    fn name(&self) -> &str {
        "Min"
    }
}

struct MaxFn;
impl IFunction for MaxFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let max = args
            .iter()
            .map(|v| v.as_f64())
            .fold(f64::NEG_INFINITY, f64::max);
        ValueType::Numeric(max)
    }
    fn name(&self) -> &str {
        "Max"
    }
}

struct AbsFn;
impl IFunction for AbsFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::Numeric(args[0].as_f64().abs())
    }
    fn name(&self) -> &str {
        "Abs"
    }
}

struct LenFn;
impl IFunction for LenFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        ValueType::Numeric(s.len() as f64)
    }
    fn name(&self) -> &str {
        "Len"
    }
}

// === Tests ===

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_registry_lookup() {
        let reg = FunctionRegistry::new();
        assert!(reg.lookup("Min").is_some());
        assert!(reg.lookup("Max").is_some());
        assert!(reg.lookup("Abs").is_some());
        assert!(reg.lookup("Len").is_some());
    }

    #[test]
    fn function_registry_case_insensitive() {
        let reg = FunctionRegistry::new();
        assert!(reg.lookup("min").is_some());
        assert!(reg.lookup("ABS").is_some());
        assert!(reg.lookup("MAX").is_some());
    }

    #[test]
    fn min_function_returns_smallest() {
        let reg = FunctionRegistry::new();
        let args = &[
            ValueType::Numeric(3.0),
            ValueType::Numeric(1.0),
            ValueType::Numeric(4.0),
        ];
        let result = reg.call("Min", args);
        assert_eq!(result, ValueType::Numeric(1.0));
    }

    #[test]
    fn max_function_returns_largest() {
        let reg = FunctionRegistry::new();
        let args = &[
            ValueType::Numeric(3.0),
            ValueType::Numeric(1.0),
            ValueType::Numeric(4.0),
        ];
        let result = reg.call("Max", args);
        assert_eq!(result, ValueType::Numeric(4.0));
    }

    #[test]
    fn abs_function_returns_absolute() {
        let reg = FunctionRegistry::new();
        let args = &[ValueType::Numeric(-5.0)];
        let result = reg.call("Abs", args);
        assert_eq!(result, ValueType::Numeric(5.0));
    }

    #[test]
    fn len_function_returns_string_length() {
        let reg = FunctionRegistry::new();
        let args = &[ValueType::String("hello".to_string())];
        let result = reg.call("Len", args);
        assert_eq!(result, ValueType::Numeric(5.0));
    }
}
