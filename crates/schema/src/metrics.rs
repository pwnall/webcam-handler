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

/// What two photographs measured, side by side (design D17; FR-W3).
///
/// "Same scene, two paths" — two units of one model, before and after a firmware update,
/// one camera direct and one forwarded — wants a comparison *record*, and this is it: the
/// D8 metric set on each side, the per-metric delta, and an SSIM corroborator that
/// represents its own unavailability instead of refusing.
///
/// **Nothing here decides.** The metrics rank exactly as they do everywhere else, SSIM
/// corroborates, and "are these two photographs peers?" is answered with numbers a consumer
/// applies its own tolerance to. And nothing here resizes: a comparison that resampled one
/// side would be measuring a codec artifact it introduced itself (E6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhotoComparison {
    /// The first photograph.
    pub a: PhotoMeasurements,
    /// The second.
    pub b: PhotoMeasurements,
    /// `b - a`, per metric — every name in [`MetricName::ALL`], so a sixth metric joins the
    /// comparison by existing rather than by being added here.
    pub delta: std::collections::BTreeMap<MetricName, f64>,
    /// The structural-similarity corroborator, or why there is not one.
    pub ssim: Ssim,
}

/// One photograph's shape and scores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PhotoMeasurements {
    /// Width in pixels, as decoded.
    pub width: u32,
    /// Height in pixels, as decoded.
    pub height: u32,
    /// Every metric this build computes, over the luma plane.
    pub metrics: std::collections::BTreeMap<MetricName, f64>,
}

/// The SSIM corroborator, or a stated reason there is none.
///
/// **A dimension mismatch is represented, not refused** (D2's doctrine applied to a
/// comparison): the per-metric scalars above are well-defined on unequal images and stay,
/// while this one says why it could not answer. So an unattended consumer branches on data
/// rather than parsing a refusal, and D17 adds no error kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ssim {
    /// Mean structural similarity over the two luma planes: 1.0 is identical.
    Measured {
        /// The score.
        score: f64,
    },
    /// No score, and the reason — from a closed set, so a consumer can branch on it.
    Unavailable {
        /// Why.
        reason: SsimUnavailable,
    },
}

/// Why a comparison carries no SSIM score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SsimUnavailable {
    /// The two images are not the same size, and nothing here resamples (E6).
    DimensionsDiffer {
        /// The first image's size, as `[width, height]`.
        a: [u32; 2],
        /// The second image's size.
        b: [u32; 2],
    },
    /// The implementation refused for a reason of its own, carried verbatim.
    ///
    /// Represented rather than swallowed (AGENTS rule 6): a corroborator that failed
    /// silently would leave a consumer unable to tell "these differ" from "nobody looked".
    ComputationFailed {
        /// What it said.
        message: String,
    },
}

impl Ssim {
    /// The score, when there is one.
    #[must_use]
    pub fn score(&self) -> Option<f64> {
        match self {
            Ssim::Measured { score } => Some(*score),
            Ssim::Unavailable { .. } => None,
        }
    }
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
