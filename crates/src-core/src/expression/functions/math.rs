//! Math functions for the expression evaluator.

use super::super::value_type::ValueType;
use super::IFunction;

pub struct RoundFn;
impl IFunction for RoundFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let v = args[0].as_f64();
        let decimals = if args.len() > 1 {
            args[1].as_f64() as i32
        } else {
            0
        };
        let factor = 10f64.powi(decimals);
        ValueType::Numeric((v * factor).round() / factor)
    }
    fn name(&self) -> &str {
        "Round"
    }
}

pub struct RoundUpFn;
impl IFunction for RoundUpFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let v = args[0].as_f64();
        let decimals = if args.len() > 1 {
            args[1].as_f64() as i32
        } else {
            0
        };
        let factor = 10f64.powi(decimals);
        ValueType::Numeric((v * factor).ceil() / factor)
    }
    fn name(&self) -> &str {
        "RoundUp"
    }
}

pub struct RoundDownFn;
impl IFunction for RoundDownFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let v = args[0].as_f64();
        let decimals = if args.len() > 1 {
            args[1].as_f64() as i32
        } else {
            0
        };
        let factor = 10f64.powi(decimals);
        ValueType::Numeric((v * factor).floor() / factor)
    }
    fn name(&self) -> &str {
        "RoundDown"
    }
}

pub struct IntFn;
impl IFunction for IntFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        // Int(x) — floor for negative numbers too
        ValueType::Numeric(args[0].as_f64().floor())
    }
    fn name(&self) -> &str {
        "Int"
    }
}

pub struct SqrFn;
impl IFunction for SqrFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let v = args[0].as_f64();
        if v < 0.0 {
            return ValueType::Undefined;
        }
        ValueType::Numeric(v.sqrt())
    }
    fn name(&self) -> &str {
        "Sqr"
    }
}

pub struct SinFn;
impl IFunction for SinFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::Numeric(args[0].as_f64().sin())
    }
    fn name(&self) -> &str {
        "Sin"
    }
}

pub struct CosFn;
impl IFunction for CosFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::Numeric(args[0].as_f64().cos())
    }
    fn name(&self) -> &str {
        "Cos"
    }
}

pub struct TanFn;
impl IFunction for TanFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::Numeric(args[0].as_f64().tan())
    }
    fn name(&self) -> &str {
        "Tan"
    }
}

pub struct AtnFn;
impl IFunction for AtnFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::Numeric(args[0].as_f64().atan())
    }
    fn name(&self) -> &str {
        "Atn"
    }
}

pub struct IIFFn;
impl IFunction for IIFFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 3 {
            return ValueType::Undefined;
        }
        // IIF(cond, a, b) — cond is truthy if non-empty and not "0"
        let cond_val = args[0].as_string();
        let cond = !cond_val.is_empty() && cond_val != "0";
        if cond {
            args[1].clone()
        } else {
            args[2].clone()
        }
    }
    fn name(&self) -> &str {
        "IIF"
    }
}

pub struct RandomFn;
impl IFunction for RandomFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        // Random(n) — returns 1..n
        // Stub implementation for expression-only context (no App/RNG access)
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let n = args[0].as_f64() as i64;
        if n <= 1 {
            return ValueType::Numeric(n.max(0) as f64);
        }
        // Deterministic stub for expression module (no RNG)
        ValueType::Numeric(1.0)
    }
    fn name(&self) -> &str {
        "Random"
    }
}

pub struct NotFn;
impl IFunction for NotFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        if s.is_empty() || s == "0" {
            ValueType::Numeric(1.0)
        } else {
            ValueType::Numeric(0.0)
        }
    }
    fn name(&self) -> &str {
        "Not"
    }
}

pub struct IsNumericFn;
impl IFunction for IsNumericFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        ValueType::Numeric(if s.trim().parse::<f64>().is_ok() {
            1.0
        } else {
            0.0
        })
    }
    fn name(&self) -> &str {
        "IsNumeric"
    }
}

pub struct RGBFn;
impl IFunction for RGBFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 3 {
            return ValueType::Undefined;
        }
        let r = args[0].as_f64() as i32;
        let g = args[1].as_f64() as i32;
        let b = args[2].as_f64() as i32;
        let r = r.clamp(0, 255);
        let g = g.clamp(0, 255);
        let b = b.clamp(0, 255);
        // VB6 RGB: r + g*256 + b*65536
        let color = r + g * 256 + b * 65536;
        ValueType::Numeric(color as f64)
    }
    fn name(&self) -> &str {
        "RGB"
    }
}

pub struct DirFn;
impl IFunction for DirFn {
    fn invoke(&self, _args: &[ValueType]) -> ValueType {
        ValueType::String(String::new())
    }
    fn name(&self) -> &str {
        "Dir"
    }
}

pub struct EvalFn;
impl IFunction for EvalFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let expr = args[0].as_string();
        super::super::eval::eval(&expr)
    }
    fn name(&self) -> &str {
        "Eval"
    }
}

pub struct IsDefinedFn;
impl IFunction for IsDefinedFn {
    fn invoke(&self, _args: &[ValueType]) -> ValueType {
        ValueType::Numeric(0.0)
    }
    fn name(&self) -> &str {
        "IsDefined"
    }
}

pub struct IsVarDefinedFn;
impl IFunction for IsVarDefinedFn {
    fn invoke(&self, _args: &[ValueType]) -> ValueType {
        ValueType::Undefined
    }
    fn name(&self) -> &str {
        "IsVarDefined"
    }
}

pub struct ArgsFn;
impl IFunction for ArgsFn {
    fn invoke(&self, _args: &[ValueType]) -> ValueType {
        ValueType::String(String::new())
    }
    fn name(&self) -> &str {
        "Args"
    }
}
