//! Timestamps, in one place.
//!
//! On disk and on the wire a timestamp is an RFC 3339 string, which is what makes the
//! date-time library swappable: nothing outside this module names `jiff`.

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An instant, serialized as an RFC 3339 string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, JsonSchema)]
#[schemars(
    with = "String",
    description = "An RFC 3339 timestamp, e.g. 2026-08-08T10:11:12Z"
)]
pub struct Stamp(jiff::Timestamp);

impl Stamp {
    /// The current instant.
    #[must_use]
    pub fn now() -> Self {
        Stamp(jiff::Timestamp::now())
    }

    /// The Unix epoch — a deterministic value for fixtures and tests.
    #[must_use]
    pub fn epoch() -> Self {
        Stamp(jiff::Timestamp::UNIX_EPOCH)
    }

    /// Milliseconds since the Unix epoch.
    #[must_use]
    pub fn as_millis(self) -> i64 {
        self.0.as_millisecond()
    }

    /// Build from milliseconds since the Unix epoch.
    ///
    /// `None` when the value is outside the representable range — a corrupt file is a
    /// parse failure, not a panic.
    #[must_use]
    pub fn from_millis(millis: i64) -> Option<Self> {
        jiff::Timestamp::from_millisecond(millis).ok().map(Stamp)
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Stamp {
    type Err = jiff::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<jiff::Timestamp>().map(Stamp)
    }
}

impl Serialize for Stamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Stamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse::<Stamp>().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_round_trips_as_rfc_3339() {
        let stamp = Stamp::from_millis(1_754_654_400_000).expect("in range");
        let json = serde_json::to_string(&stamp).expect("serialize");
        assert!(json.starts_with("\"2025-08-"), "not RFC 3339: {json}");
        let back: Stamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, stamp);
    }

    #[test]
    fn a_malformed_timestamp_is_a_parse_error_not_a_panic() {
        let err = serde_json::from_str::<Stamp>("\"yesterday\"");
        assert!(err.is_err());
    }

    #[test]
    fn the_epoch_is_available_for_deterministic_fixtures() {
        assert_eq!(Stamp::epoch().as_millis(), 0);
    }
}
