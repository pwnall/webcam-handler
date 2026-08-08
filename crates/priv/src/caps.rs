//! The capability set this helper carries, and how it reaches a child process.
//!
//! ## How a capability reaches a child, and the trap on the way
//!
//! File capabilities do **not** cross `exec`. A blessed binary runs privileged and
//! anything it spawns gets nothing, which would make [`super::exec`] a wrapper that grants
//! exactly zero privilege — the most dangerous kind of tool: one that looks like it works.
//!
//! Four sets are involved:
//!
//! - **Permitted** (`pP`) — what this process may use. `setcap … +p` puts it here.
//! - **Inheritable** (`pI`) — half of what may be raised into ambient.
//! - **Ambient** (`pA`) — what a child actually receives across `exec` (Linux 4.3+).
//! - **Effective** (`pE`) — what is active right now. `setcap … +e` puts `pP` here on exec.
//!
//! **The trap:** `setcap … +i` sets the *file's* inheritable set, `fI`, and `fI` is not
//! `pI`. The kernel computes `pI' = pI` across `exec` — unchanged from whoever called us —
//! and `fI` only ever appears as the term `pI & fI`, which is empty unless the *caller*
//! already held the capability inheritable. A shell does not. So blessing `+eip` and then
//! calling `PR_CAP_AMBIENT_RAISE`, which requires the capability in both `pP` and `pI`,
//! fails every time — with the file looking exactly right in `getcap`.
//!
//! **The fix, and why it is allowed:** [`raise_ambient`] adds each capability to `pI`
//! itself before raising it into `pA`. `capabilities(7)` permits that without
//! `CAP_SETPCAP` — the rule for `capset` is `pI' ⊆ (pI | pP)`, so holding a capability in
//! *permitted* is licence enough to make it inheritable. `pP` is exactly what the file's
//! `+p` gave us.
//!
//! [`BLESSING`] is therefore `+ep`: the inheritable bit on the file is dead weight, and
//! carrying it would preserve a false explanation of why delegation works.
//!
//! ## What this crate deliberately does not do
//!
//! It never drops privilege before doing the work, because there is no work here that
//! does not need it. It never re-raises a capability it was not blessed with — that is
//! not possible, and code that tried would be code pretending the bless is optional.

use std::collections::BTreeSet;
use std::fmt;

use caps::{CapSet, Capability};

/// The capabilities `just bless` grants, and the argument it passes to `setcap`.
///
/// One home: the justfile reads this string out of `wch-priv doctor --setcap-argument`
/// rather than repeating it, so the bless and the runtime check cannot disagree about
/// what "blessed" means.
pub(crate) const BLESSING: &str = "cap_sys_module,cap_net_admin+ep";

/// Every capability this helper needs, with what each one is for.
///
/// A closed table. The security boundary is the file mode on the blessed copy (see the
/// crate docs), not this list — but the list is what `doctor` reports and what the
/// justfile blesses, so it stays the single place either is written down.
pub(crate) const REQUIRED: &[(Capability, &str)] = &[
    (
        Capability::CAP_SYS_MODULE,
        "load and unload vivid (the R2 rung) and cycle uvcvideo",
    ),
    (
        Capability::CAP_NET_ADMIN,
        "bind the NETLINK_KOBJECT_UEVENT socket the P4 hotplug watch needs",
    ),
];

/// What the process is actually holding right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    /// Capabilities in the permitted set — what this process may use itself.
    pub(crate) permitted: BTreeSet<String>,
    /// Capabilities in the inheritable set. Informational: this is the set
    /// [`raise_ambient`] *writes*, so what it holds before the raise says nothing about
    /// whether delegation will work.
    pub(crate) inheritable: BTreeSet<String>,
    /// Capabilities in the ambient set, or `None` on a kernel without one (pre-4.3).
    /// `None` is the only thing that makes delegation impossible for a blessed binary.
    pub(crate) ambient: Option<BTreeSet<String>>,
    /// The required capabilities that are missing from permitted — the one signal that
    /// means "not blessed".
    pub(crate) missing_permitted: Vec<&'static str>,
}

impl Held {
    /// Read the current process's sets.
    ///
    /// # Errors
    ///
    /// When the kernel refuses the `capget` — which on Linux means something is very
    /// wrong, not that we lack privilege.
    pub(crate) fn read() -> Result<Held, Error> {
        let permitted = read_set(CapSet::Permitted)?;
        let inheritable = read_set(CapSet::Inheritable)?;
        Ok(Held {
            missing_permitted: missing(&permitted),
            // A kernel too old for ambient capabilities cannot delegate at all, and that
            // is a different answer from "not blessed" — so it is `None`, not empty.
            ambient: read_set(CapSet::Ambient).ok(),
            permitted,
            inheritable,
        })
    }

    /// Whether this process can do the privileged work itself.
    #[must_use]
    pub(crate) fn can_act(&self) -> bool {
        self.missing_permitted.is_empty()
    }

    /// Every required capability this process lacks, once each.
    ///
    /// A capability missing from both permitted and inheritable is one problem with one
    /// fix — `just bless` — so it is reported once.
    #[must_use]
    pub(crate) fn missing(&self) -> Vec<&'static str> {
        self.missing_permitted.clone()
    }
}

fn read_set(set: CapSet) -> Result<BTreeSet<String>, Error> {
    let held = caps::read(None, set).map_err(|error| Error::Capget {
        set: format!("{set:?}"),
        message: error.to_string(),
    })?;
    Ok(held.into_iter().map(|cap| cap.to_string()).collect())
}

fn missing(held: &BTreeSet<String>) -> Vec<&'static str> {
    REQUIRED
        .iter()
        .filter(|(cap, _)| !held.contains(&cap.to_string()))
        .map(|(_, why)| *why)
        .collect()
}

/// Raise every required capability into the ambient set, so `exec` carries them.
///
/// # Errors
///
/// [`Error::NotBlessed`] when the binary was not blessed, or was blessed `+ep` instead of
/// `+eip` — the second case is the one worth distinguishing, because the helper *works*
/// for its own verbs and silently grants nothing to children.
pub(crate) fn raise_ambient() -> Result<(), Error> {
    let held = Held::read()?;
    if !held.can_act() {
        return Err(Error::NotBlessed {
            what: "not blessed".to_owned(),
            held: Box::new(held),
        });
    }
    if held.ambient.is_none() {
        return Err(Error::NoAmbientSet);
    }

    for (cap, why) in REQUIRED {
        // Inheritable first, and this order is the whole fix: `PR_CAP_AMBIENT_RAISE`
        // requires the capability in *both* permitted and inheritable, and the file's
        // `+i` bit never reaches a process's inheritable set. Permitted is what licenses
        // this write (`capabilities(7)`: `pI' ⊆ (pI | pP)`).
        for set in [CapSet::Inheritable, CapSet::Ambient] {
            caps::raise(None, set, *cap).map_err(|error| Error::Ambient {
                capability: cap.to_string(),
                set: format!("{set:?}"),
                purpose: (*why).to_owned(),
                message: error.to_string(),
            })?;
        }
    }
    Ok(())
}

/// Insist this process can do the work itself, with an error that says how to fix it.
///
/// # Errors
///
/// [`Error::NotBlessed`] with the full held/missing breakdown.
pub(crate) fn require_effective() -> Result<(), Error> {
    let held = Held::read()?;
    if held.can_act() {
        Ok(())
    } else {
        Err(Error::NotBlessed {
            what: "not blessed".to_owned(),
            held: Box::new(held),
        })
    }
}

/// What can go wrong before any privileged work is attempted.
#[derive(Debug)]
pub(crate) enum Error {
    /// The binary does not carry what it needs.
    NotBlessed {
        /// How it falls short, in a phrase.
        what: String,
        /// Everything read, so the message can be specific.
        held: Box<Held>,
    },
    /// `capget` failed.
    Capget {
        /// Which set was being read.
        set: String,
        /// The system's description.
        message: String,
    },
    /// Raising a capability into one of the sets failed.
    Ambient {
        /// Which capability.
        capability: String,
        /// Which set it could not reach.
        set: String,
        /// What it was for.
        purpose: String,
        /// The system's description.
        message: String,
    },
    /// The kernel has no ambient capability set (pre-4.3), so nothing can be delegated
    /// however well this binary is blessed. Distinct from "not blessed": no amount of
    /// `just bless` fixes it.
    NoAmbientSet,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotBlessed { what, held } => {
                writeln!(f, "this copy of wch-priv is {what}.")?;
                // Deduplicated: a capability absent from both sets is one problem with
                // one fix, and printing it twice reads like two.
                for purpose in held.missing() {
                    writeln!(f, "  missing: {purpose}")?;
                }
                write!(
                    f,
                    "\nRun `just bless` from the repository root. It needs sudo once, and \
                     again only when this binary's own source changes."
                )
            }
            Error::Capget { set, message } => {
                write!(f, "could not read the {set} capability set: {message}")
            }
            Error::Ambient {
                capability,
                set,
                purpose,
                message,
            } => write!(
                f,
                "could not raise {capability} (needed to {purpose}) into the {set} set: \
                 {message}"
            ),
            Error::NoAmbientSet => write!(
                f,
                "this kernel has no ambient capability set (it arrived in Linux 4.3), so \
                 capabilities cannot be passed to a child process at all"
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blessing_string_and_the_required_table_name_the_same_capabilities() {
        // One home, checked: the justfile blesses with `BLESSING` and the runtime checks
        // `REQUIRED`, so a capability added to one and not the other would produce a
        // binary that passes `doctor` and cannot do its job (or the reverse — a
        // capability granted that nothing needs, which is privilege for free).
        let blessed: BTreeSet<String> = BLESSING
            .split('+')
            .next()
            .expect("the blessing has a capability list")
            .split(',')
            .map(str::to_owned)
            .collect();
        let required: BTreeSet<String> = REQUIRED
            .iter()
            .map(|(cap, _)| cap.to_string().to_lowercase())
            .collect();
        assert_eq!(blessed, required);
    }

    #[test]
    fn the_blessing_asks_for_permitted_and_effective_and_not_inheritable() {
        // The corrected story, pinned so it cannot quietly revert. `+i` on the *file*
        // does not populate a process's inheritable set — `pI' = pI` across exec — so it
        // would be dead weight carrying a false explanation of why delegation works.
        // `raise_ambient` writes inheritable itself, licensed by permitted.
        let flags = BLESSING
            .split('+')
            .nth(1)
            .expect("the blessing has a flag set");
        assert!(flags.contains('p'), "{BLESSING} grants nothing");
        assert!(flags.contains('e'), "{BLESSING} is not effective on entry");
        assert!(
            !flags.contains('i'),
            "{BLESSING} carries a file-inheritable bit that cannot affect a process's \
             inheritable set; delegation comes from raising it at runtime"
        );
    }

    #[test]
    fn every_required_capability_says_what_it_is_for() {
        // A capability nobody can justify in a sentence is a capability nobody should
        // have granted. `doctor` prints these, so they are the audit trail.
        for (cap, why) in REQUIRED {
            assert!(why.len() > 20, "{cap:?} has no stated purpose");
        }
    }

    #[test]
    fn an_unblessed_process_reports_what_is_missing_and_how_to_fix_it() {
        // The ordinary case for a fresh checkout, and for `cargo test` itself: the test
        // binary is not blessed, so this is the branch a developer meets first.
        let held = Held::read().expect("capget works on Linux");
        if held.can_act() {
            // Only reachable if someone blessed the *test* binary, which nothing does.
            return;
        }
        let error = require_effective().expect_err("an unblessed process cannot act");
        let rendered = error.to_string();
        assert!(rendered.contains("just bless"), "{rendered}");
        assert!(rendered.contains("sudo"), "{rendered}");
        for (_, why) in REQUIRED {
            assert!(rendered.contains(why), "{rendered} omits {why}");
            // Once each: a capability absent from both sets is one problem, and listing
            // it twice makes a two-line fix look like a four-line one.
            assert_eq!(
                rendered.matches(why).count(),
                1,
                "{why} is reported more than once in:\n{rendered}"
            );
        }
    }
}
