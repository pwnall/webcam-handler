//! The metric vocabulary (design D8).
//!
//! Metrics **rank**; they do not **decide**. A Laplacian variance does not know what
//! "text legible on the DUT display" means, and the session record says who chose a
//! value — `metric:<name>`, `agent`, or `human` — precisely so nothing pretends
//! otherwise.
//!
//! The functions that compute these live in `webcam-handler-imaging`; the names live
//! here because sessions persist them and the wire carries them.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// A built-in image quality metric.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum MetricName {
        /// Variance of the Laplacian: higher is sharper.
        Sharpness,
        /// Fraction of pixels at or near full white. Higher means more blown highlights.
        ClippedHighlights,
        /// Fraction of pixels at or near full black.
        ClippedShadows,
        /// Mean luma, 0.0–1.0. Neither high nor low is "better" — it is context.
        MeanLuma,
        /// Root-mean-square contrast.
        RmsContrast,
    }
}

impl MetricName {
    /// The name used in `metric:<name>` selectors, `--json` output, and session files.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            MetricName::Sharpness => "sharpness",
            MetricName::ClippedHighlights => "clipped_highlights",
            MetricName::ClippedShadows => "clipped_shadows",
            MetricName::MeanLuma => "mean_luma",
            MetricName::RmsContrast => "rms_contrast",
        }
    }

    /// Parse a metric name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.as_str() == s)
    }

    /// Which direction is "better" when this metric is used to rank samples.
    ///
    /// Explicit because it is not obvious and getting it backwards would silently pick
    /// the worst sample: more sharpness is better, more clipping is worse, and mean luma
    /// has no direction at all.
    #[must_use]
    pub const fn preference(self) -> Preference {
        match self {
            MetricName::Sharpness | MetricName::RmsContrast => Preference::HigherIsBetter,
            MetricName::ClippedHighlights | MetricName::ClippedShadows => Preference::LowerIsBetter,
            MetricName::MeanLuma => Preference::Undirected,
        }
    }
}

impl fmt::Display for MetricName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which way a metric points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Preference {
    /// Rank descending.
    HigherIsBetter,
    /// Rank ascending.
    LowerIsBetter,
    /// This metric cannot rank on its own; a caller asking it to must be refused.
    Undirected,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_names_round_trip_and_are_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for &m in MetricName::ALL {
            assert_eq!(MetricName::parse(m.as_str()), Some(m));
            assert!(seen.insert(m.as_str()), "{m:?} duplicates a name");
        }
        assert_eq!(MetricName::parse("vibes"), None);
    }

    #[test]
    fn mean_luma_refuses_to_rank() {
        // The one metric with no direction. A selector built on it must be refused
        // rather than silently choosing "brightest".
        assert_eq!(MetricName::MeanLuma.preference(), Preference::Undirected);
        assert_eq!(
            MetricName::Sharpness.preference(),
            Preference::HigherIsBetter
        );
        assert_eq!(
            MetricName::ClippedHighlights.preference(),
            Preference::LowerIsBetter
        );
    }

    #[test]
    fn every_metric_serializes_as_its_selector_name() {
        for &m in MetricName::ALL {
            let json = serde_json::to_string(&m).expect("serialize");
            assert_eq!(json, format!("\"{}\"", m.as_str()));
        }
    }
}
