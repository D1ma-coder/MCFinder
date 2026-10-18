//! Decimal-string input for lossless 64-bit seeds from JavaScript clients.
use serde::{Deserialize, Deserializer};
#[derive(Deserialize)]
#[serde(untagged)]
enum Number {
    Unsigned(u64),
    Signed(i64),
    Text(String),
}
pub fn seed<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match Number::deserialize(d)? {
        Number::Unsigned(v) => Ok(v),
        Number::Signed(v) => Ok(v as u64),
        Number::Text(s) => {
            if s.starts_with('-') {
                s.parse::<i64>()
                    .map(|v| v as u64)
                    .map_err(serde::de::Error::custom)
            } else {
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    }
}
pub fn count<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match Number::deserialize(d)? {
        Number::Unsigned(v) => Ok(v),
        Number::Signed(v) => u64::try_from(v).map_err(serde::de::Error::custom),
        Number::Text(s) => s.parse().map_err(serde::de::Error::custom),
    }
}
