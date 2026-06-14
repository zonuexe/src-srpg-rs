//! String functions for the expression evaluator.

use super::super::value_type::ValueType;
use super::IFunction;

pub struct LeftFn;
impl IFunction for LeftFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let n = args[1].as_f64() as usize;
        ValueType::String(s.chars().take(n).collect())
    }
    fn name(&self) -> &str {
        "Left"
    }
}

pub struct RightFn;
impl IFunction for RightFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let n = args[1].as_f64() as usize;
        let chars: Vec<char> = s.chars().collect();
        let start = chars.len().saturating_sub(n);
        ValueType::String(chars[start..].iter().collect())
    }
    fn name(&self) -> &str {
        "Right"
    }
}

pub struct MidFn;
impl IFunction for MidFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let start = args[1].as_f64() as usize;
        let n = if args.len() > 2 {
            args[2].as_f64() as usize
        } else {
            s.len()
        };
        let chars: Vec<char> = s.chars().collect();
        let start = (start - 1).min(chars.len()); // 1-indexed
        let end = (start + n).min(chars.len());
        ValueType::String(chars[start..end].iter().collect())
    }
    fn name(&self) -> &str {
        "Mid"
    }
}

pub struct InStrFn;
impl IFunction for InStrFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let haystack = args[0].as_string();
        let needle = args[1].as_string();
        let start: usize = if args.len() > 2 {
            args[2].as_f64() as usize
        } else {
            1
        };
        let start_idx = start.saturating_sub(1);
        let chars: Vec<char> = haystack.chars().collect();
        if start_idx >= chars.len() {
            return ValueType::Numeric(0.0);
        }
        let sub: String = chars[start_idx..].iter().collect();
        if let Some(pos) = sub.find(&needle) {
            // Convert byte position to char position
            let char_pos = sub[..pos].chars().count();
            ValueType::Numeric((start_idx + char_pos + 1) as f64)
        } else {
            ValueType::Numeric(0.0)
        }
    }
    fn name(&self) -> &str {
        "InStr"
    }
}

pub struct InStrRevFn;
impl IFunction for InStrRevFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let haystack = args[0].as_string();
        let needle = args[1].as_string();
        if needle.is_empty() {
            return ValueType::Numeric(0.0);
        }
        let chars: Vec<char> = haystack.chars().collect();
        let start: i64 = if args.len() > 2 {
            args[2].as_f64() as i64
        } else {
            -1
        };
        let upto: usize = if start < 0 {
            chars.len()
        } else {
            (start as usize).min(chars.len())
        };
        let sub: String = chars[..upto].iter().collect();
        if let Some(pos) = sub.rfind(&needle) {
            let char_pos = sub[..pos].chars().count();
            ValueType::Numeric((char_pos + 1) as f64)
        } else {
            ValueType::Numeric(0.0)
        }
    }
    fn name(&self) -> &str {
        "InStrRev"
    }
}

pub struct LCaseFn;
impl IFunction for LCaseFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::String(args[0].as_string().to_lowercase())
    }
    fn name(&self) -> &str {
        "LCase"
    }
}

pub struct UCaseFn;
impl IFunction for UCaseFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        ValueType::String(args[0].as_string().to_uppercase())
    }
    fn name(&self) -> &str {
        "UCase"
    }
}

pub struct TrimFn;
impl IFunction for TrimFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        // VB6 Trim$ — only trims ASCII space, not full-width
        ValueType::String(args[0].as_string().trim_matches(' ').to_string())
    }
    fn name(&self) -> &str {
        "Trim"
    }
}

pub struct AscFn;
impl IFunction for AscFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let binding = args[0].as_string();
        let s = binding.trim_matches('"');
        let Some(first) = s.chars().next() else {
            return ValueType::Numeric(0.0);
        };
        ValueType::Numeric(first as u32 as f64)
    }
    fn name(&self) -> &str {
        "Asc"
    }
}

pub struct ChrFn;
impl IFunction for ChrFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let code = args[0].as_f64() as u32;
        let c = char::from_u32(code).unwrap_or('\0');
        ValueType::String(c.to_string())
    }
    fn name(&self) -> &str {
        "Chr"
    }
}

pub struct StringFn;
impl IFunction for StringFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let n = args[0].as_f64() as usize;
        let s = args[1].as_string();
        let c = s.chars().next().unwrap_or(' ');
        ValueType::String(c.to_string().repeat(n))
    }
    fn name(&self) -> &str {
        "String"
    }
}

pub struct ReplaceFn;
impl IFunction for ReplaceFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 3 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let from = args[1].as_string();
        let to = args[2].as_string();
        ValueType::String(s.replace(&from, &to))
    }
    fn name(&self) -> &str {
        "Replace"
    }
}

pub struct StrCmpFn;
impl IFunction for StrCmpFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let a = args[0].as_string();
        let b = args[1].as_string();
        let result = a.cmp(&b);
        let v = match result {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        };
        ValueType::Numeric(v)
    }
    fn name(&self) -> &str {
        "StrCmp"
    }
}

pub struct FormatFn;
impl IFunction for FormatFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.is_empty() {
            return ValueType::Undefined;
        }
        // Simplified — just return the value as string
        // Full implementation would handle format patterns like "#,##0", "0.00"
        ValueType::String(args[0].as_string())
    }
    fn name(&self) -> &str {
        "Format"
    }
}

pub struct WideFn;
impl IFunction for WideFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        // Wide(s) — converts ASCII to full-width (Wide characters)
        if args.is_empty() {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let out: String = s
            .chars()
            .map(|c| match c as u32 {
                0x20 => '\u{3000}',
                0x21..=0x7E => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
                _ => c,
            })
            .collect();
        ValueType::String(out)
    }
    fn name(&self) -> &str {
        "Wide"
    }
}

pub struct LSetFn;
impl IFunction for LSetFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let w = args[1].as_f64() as usize;
        let len = s.chars().count();
        if len >= w {
            ValueType::String(s.chars().take(w).collect())
        } else {
            let mut result = s;
            result.push_str(&" ".repeat(w - len));
            ValueType::String(result)
        }
    }
    fn name(&self) -> &str {
        "LSet"
    }
}

pub struct RSetFn;
impl IFunction for RSetFn {
    fn invoke(&self, args: &[ValueType]) -> ValueType {
        if args.len() < 2 {
            return ValueType::Undefined;
        }
        let s = args[0].as_string();
        let w = args[1].as_f64() as usize;
        let len = s.chars().count();
        if len >= w {
            ValueType::String(s.chars().take(w).collect())
        } else {
            let mut result = " ".repeat(w - len);
            result.push_str(&s);
            ValueType::String(result)
        }
    }
    fn name(&self) -> &str {
        "RSet"
    }
}
