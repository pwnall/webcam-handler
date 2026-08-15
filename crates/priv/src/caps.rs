//! The capability set this helper carries, and how it reaches a child process.
//!
//! ## How a capability reaches a child, and the trap on the way
//!
//! File capabilities do **not** cross `exec`. A blessed binary runs privileged and
//! anything it spawns gets nothing — and *everything privileged here is spawned*, because
//! nothing in this crate loads a module itself: [`super::vivid`] and [`super::uvcvideo`]
//! run `/usr/sbin/modprobe`, which is an `exec` like any other. A build that skipped the
//! raise below would be the most dangerous kind of tool: one that looks like it works,
//! with `getcap` agreeing.
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
/// One home: the justfile reads this string out of `webcam-handler-priv doctor
/// --setcap-argument` rather than repeating it, so the bless and the runtime check cannot
/// disagree about what "blessed" means. `scripts/gates/privileged-helper.sh` reads it out
/// of *this line* and compares it with what a blessed copy actually carries, which is the
/// half neither the recipe nor a test can see (note **N125**).
pub(crate) const BLESSING: &str = "cap_sys_module+ep";

/// Every capability this helper needs, with what each one is for.
///
/// A closed table, and since P6e a table of **one**. The security boundary is the file mode
/// on the blessed copy (see the crate docs), not this list — but the list is what `doctor`
/// reports and what the justfile blesses, so it stays the single place either is written
/// down, and `the_blessing_is_cap_sys_module_and_nothing_else` is what goes red if it
/// widens again.
///
/// `CAP_NET_ADMIN` was here until P6e, granted at P0 on a *prediction* that binding
/// `NETLINK_KOBJECT_UEVENT` would need it. P4d measured the prediction and disproved it —
/// the socket binds and delivers to a process with an empty capability set \[PF:21\] — so
/// the grant was one nothing ever spent, which is the easiest kind of privilege to remove
/// and the easiest to forget. Note **N8**'s G6 reckoning removed it; note **N125** records
/// what it would take to bring it back (`SO_RCVBUFFORCE`, which nothing here uses).
pub(crate) const REQUIRED: &[(Capability, &str)] = &[(
    Capability::CAP_SYS_MODULE,
    "load and unload vivid (the R2 rung) and cycle uvcvideo",
)];

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

/// Raise every required capability into the ambient set, so a child process carries them.
///
/// **Every privileged verb needs this**, and that sentence is why deleting the `exec` verb
/// at P6e did not delete this function with it (note **N125**). Nothing in this crate loads
/// a module itself: `vivid up` and `uvcvideo cycle` spawn `/usr/sbin/modprobe`, and a
/// subprocess is an `exec` like any other — `pP' = (X & fP) | (X & pI & fI) | pA`, and
/// with no file capabilities on `modprobe` the first two terms are empty. The child gets
/// the ambient set or it gets nothing. When the raise lived only in the delegating verb,
/// every module verb failed with `EPERM` while `getcap` and `doctor` both looked correct.
///
/// What *is* gone is the ability to point the delegation at a program the caller names:
/// the ambient set this raises now only ever reaches `/usr/sbin/modprobe`, spawned by
/// [`super::vivid`] or [`super::uvcvideo`] with a module name fixed at compile time.
///
/// # Errors
///
/// [`Error::NotBlessed`] when the binary carries no capabilities, [`Error::NoAmbientSet`]
/// on a kernel too old to have one.
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
                writeln!(f, "this copy of webcam-handler-priv is {what}.")?;
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
    fn the_blessing_is_cap_sys_module_and_nothing_else() {
        // **The narrowing, pinned.** The test above proves the two spellings agree with
        // each other, which is a property both of them widening together satisfies — it
        // would have stayed green through the whole of the grant this note's reckoning
        // removed. This one is about the *content*, and it is the thing that goes red when
        // a capability is added back "in case", which is exactly how `CAP_NET_ADMIN` was
        // granted at P0 and how it survived four phases nobody spending it (notes N8,
        // **N125**; the measurement that removed it is \[PF:21\]).
        //
        // A closed list rather than a rule, because there is no rule to write: whether a
        // capability is needed is a question about what this crate's verbs do, and the only
        // honest form of it is a human deciding once and leaving the argument behind. So a
        // future need for a second capability fails here, and the cost of that failure is
        // reading N8's three questions and amending N125 — which is the whole point.
        assert_eq!(
            REQUIRED.len(),
            1,
            "the grant widened; N8's reckoning narrowed it to one capability and N125 says \
             what it would take to add another"
        );
        assert_eq!(
            REQUIRED
                .iter()
                .map(|(cap, _)| cap.to_string())
                .collect::<Vec<_>>(),
            vec!["CAP_SYS_MODULE"],
            "the one capability this helper spends is the one `modprobe` needs"
        );
        // …and the string the justfile blesses with, because the two are separate
        // spellings and a gate reads this one off the source line.
        assert_eq!(BLESSING, "cap_sys_module+ep");
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
        //
        // Against `raise_ambient`, because that is now what *every* privileged verb calls
        // — the weaker `require_effective` check was what let the module verbs ship
        // believing a subprocess would inherit something it cannot.
        let held = Held::read().expect("capget works on Linux");
        if held.can_act() {
            // Only reachable if someone blessed the *test* binary, which nothing does.
            return;
        }
        let error = raise_ambient().expect_err("an unblessed process cannot delegate");
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
