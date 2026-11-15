//! A minimal JSON writer for `--json` output. The shapes are small and fixed, so this avoids a
//! serialization dependency.

use std::fmt::{self, Display, Write};

/// A JSON value.
pub enum J {
    Null,
    Bool(bool),
    Int(u64),
    /// Finite numbers only; a non-finite value is written as `null`.
    Float(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(&'static str, J)>),
}

impl From<bool> for J {
    fn from(v: bool) -> Self {
        J::Bool(v)
    }
}

impl From<u64> for J {
    fn from(v: u64) -> Self {
        J::Int(v)
    }
}

impl From<usize> for J {
    fn from(v: usize) -> Self {
        J::Int(v as u64)
    }
}

impl From<u32> for J {
    fn from(v: u32) -> Self {
        J::Int(u64::from(v))
    }
}

impl From<&str> for J {
    fn from(v: &str) -> Self {
        J::Str(v.to_string())
    }
}

impl From<String> for J {
    fn from(v: String) -> Self {
        J::Str(v)
    }
}

impl From<&std::path::Path> for J {
    fn from(v: &std::path::Path) -> Self {
        J::Str(v.to_string_lossy().into_owned())
    }
}

impl Display for J {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            J::Null => f.write_str("null"),
            J::Bool(b) => write!(f, "{b}"),
            J::Int(n) => write!(f, "{n}"),
            J::Float(x) if x.is_finite() => write!(f, "{x}"),
            J::Float(_) => f.write_str("null"),
            J::Str(s) => write_string(f, s),
            J::Arr(items) => {
                f.write_char('[')?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_char(',')?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_char(']')
            }
            J::Obj(fields) => {
                f.write_char('{')?;
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_char(',')?;
                    }
                    write_string(f, key)?;
                    write!(f, ":{value}")?;
                }
                f.write_char('}')
            }
        }
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_char('"')?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\u{:04x}", c as u32)?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_values() {
        let v = J::Obj(vec![
            ("ok", J::Bool(true)),
            ("n", J::Int(5)),
            ("xs", J::Arr(vec![J::Int(1), J::Null, J::Float(0.5)])),
        ]);
        assert_eq!(v.to_string(), r#"{"ok":true,"n":5,"xs":[1,null,0.5]}"#);
    }

    #[test]
    fn escapes_quotes_backslashes_and_control_characters() {
        let v = J::Str("a\"b\\c\nd\u{1}é".into());
        assert_eq!(v.to_string(), "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }

    #[test]
    fn non_finite_floats_become_null() {
        assert_eq!(J::Float(f64::NAN).to_string(), "null");
    }
}
