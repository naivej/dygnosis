//! Validated total macro work allowance, independent of source and value bounds.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const INVALID_MESSAGE: &str =
    "macro_work_limit must be a whole number from 1 to 100000000; zero does not disable the limit";

/// Finite work units allowed for one checked model root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "u64")]
pub struct MacroWorkLimit(usize);

impl JsonSchema for MacroWorkLimit {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "MacroWorkLimit".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"integer", "minimum":1, "maximum":Self::MAX})
    }
}

impl MacroWorkLimit {
    pub const DEFAULT: usize = 1_000_000;
    pub const MAX: usize = 100_000_000;

    pub fn new(value: u64) -> Result<Self, &'static str> {
        if (1..=Self::MAX as u64).contains(&value) {
            Ok(Self(value as usize))
        } else {
            Err(INVALID_MESSAGE)
        }
    }

    pub fn get(self) -> usize {
        self.0
    }

    pub(crate) fn from_json(value: &serde_json::Value) -> Option<Self> {
        let value = value.as_f64()?;
        if !value.is_finite() || value.fract() != 0.0 || !(1.0..=Self::MAX as f64).contains(&value)
        {
            return None;
        }
        Self::new(value as u64).ok()
    }
}

impl<'de> Deserialize<'de> for MacroWorkLimit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Self::from_json(&value).ok_or_else(|| serde::de::Error::custom(INVALID_MESSAGE))
    }
}

impl Default for MacroWorkLimit {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl TryFrom<u64> for MacroWorkLimit {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<MacroWorkLimit> for u64 {
    fn from(value: MacroWorkLimit) -> Self {
        value.get() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn work_limit_accepts_only_finite_supported_integers() {
        // Guards inconsistent transport ranges and zero meaning unlimited.
        for value in [json!(1), json!(1.0), json!(MacroWorkLimit::MAX)] {
            let limit: MacroWorkLimit = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(Some(limit), MacroWorkLimit::from_json(&value));
        }
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("100"),
            json!(null),
            json!(MacroWorkLimit::MAX + 1),
        ] {
            assert!(serde_json::from_value::<MacroWorkLimit>(value.clone()).is_err());
            assert!(MacroWorkLimit::from_json(&value).is_none());
        }
    }
}
