//! One spelling for the pure cores' typed refusals.
//!
//! The error registry (D13) is closed, and the only variant that means "this operation
//! is not legal in this situation" is [`schema::Error::IllegalTransition`], which renders
//! as `{from}: cannot {op}` — the condition as a label and the operation last, so a
//! producer whose `op` is a whole instruction still ends the sentence (note **N212**). The
//! sweep planner and the session state machine both need it, and left to themselves they
//! would invent two phrasings for the same refusal — so this module pins the convention:
//!
//! - `from` is a compact machine-readable token naming the *condition that refused*:
//!   `empty_range(min=10,max=0)`, `blocked`, `undirected_metric(mean_luma)`. A caller
//!   that wants to branch matches the token rather than parsing prose.
//! - `op` is the attempted operation with its subject: `sweep pan_absolute`,
//!   `select focus_absolute`.
//!
//! Both halves are free text in the registry, which is exactly why they need a
//! convention: an error nobody can match on is a string with extra steps.

use schema::Error;

/// Build the engine's one refusal, in the convention this module documents.
pub(crate) fn illegal(from: impl Into<String>, op: impl Into<String>) -> Error {
    Error::IllegalTransition {
        from: from.into(),
        op: op.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_renders_as_a_sentence_naming_both_halves() {
        // The convention is only worth having if the rendered form reads; a `from` that
        // is a state token and an `op` that carries its subject produce one sentence.
        let err = illegal("motion(allow_motion=false)", "sweep pan_absolute");
        assert_eq!(
            err.to_string(),
            "motion(allow_motion=false): cannot sweep pan_absolute"
        );
    }
}
