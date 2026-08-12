//! D11's bind × token matrix, decided from values before anything is bound (docs/7 P5a).
//!
//! Design D11 states the matrix once, and docs/7 P5a asks for it "enforced *as written in
//! D11*", so it is quoted here rather than paraphrased:
//!
//! > The full bind × token matrix, stated once (docs/7 G5 and the docs/9 token gate cite
//! > this paragraph rather than paraphrasing): loopback + token is the default; token-less
//! > loopback exists only behind one named explicit flag (`--http-insecure-loopback`);
//! > non-loopback **always** requires the token — there is no flag that removes it — and
//! > additionally prints a warning naming what it exposes (a live camera).
//!
//! Four cells, and all four are reachable: [`Posture::of`] is total over its two arguments
//! and every combination of them has a test named for its cell.
//!
//! ## Why this is a function over values and not three lines in the listener
//!
//! AGENTS's first rule about writing code — "pure cores take values" — and here it buys
//! something specific. The decision's inputs are a [`SocketAddr`] and a `bool`, and its
//! output is a value; so the matrix is asserted in four unit tests that open no socket, read
//! no environment and consult no clock, on a machine that may have no second interface to
//! bind to at all. The alternative shape — decide while binding — makes the non-loopback
//! cells testable only on a host whose network happens to cooperate, which is the same as
//! saying they are the cells nothing checks.
//!
//! The other half of the argument is that this is a *security* decision. What
//! [`Posture::of`] answers is the whole of "may this listener serve without a token", and a
//! decision that is one branch inside a `bind` call is a decision that can be edited without
//! anything going red.
//!
//! ## The hole this matrix exists to close: an address that is loopback and does not say so
//!
//! `SocketAddr` knows whether it is loopback, and asking it the obvious way is wrong.
//! [`std::net::Ipv6Addr::is_loopback`] is `::1` and nothing else — it answers **false** for
//! `::ffff:127.0.0.1`, the IPv4-mapped form of the address every operator means by
//! "loopback", which a dual-stack listener sees whenever an IPv4 client reaches an IPv6
//! socket. A build that trusted `IpAddr::is_loopback` alone would classify that bind as
//! non-loopback. That direction is the *safe* one for the token (it would demand one), but it
//! is the wrong one for the operator: they would be warned that a live camera is exposed to
//! the network when it is reachable only from this machine, and a warning that fires on the
//! default case is a warning people learn to ignore.
//!
//! So [`Reach::of`] unwraps the mapping first, with
//! [`std::net::Ipv6Addr::to_ipv4_mapped`], and asks the IPv4 address the IPv4 question —
//! which is `127.0.0.0/8` and not `127.0.0.1`, so `127.0.0.5` is loopback too.
//!
//! **IPv4-*compatible* addresses (`::127.0.0.1`, no `ffff`) are deliberately not unwrapped.**
//! RFC 4291 deprecated that form, Linux does not route it, and `to_ipv4_mapped` declines it
//! by design. Treating it as loopback would be this module inventing a loopback address the
//! kernel does not have; declining it errs closed, which is the direction D11 chose for
//! everything else in the paragraph.
//!
//! `0.0.0.0` and `::` are **not** loopback, and neither is `::ffff:0.0.0.0`. They are the
//! wildcard binds the warning exists for: a daemon bound to one of them is reachable from
//! every interface this machine has.
//!
//! ## What this module deliberately does not decide
//!
//! - **Whether there is a listener at all.** `--http` is opt-in (D11) and that is the
//!   composition root's question; this module is asked only once the operator has already
//!   said yes.
//! - **How a request is checked.** The token gate is middleware over the routes, and it is
//!   P5b's; what lives here is the *rule* it enforces, as a value it can read.
//! - **Where the warning is printed.** [`Posture::warning`] answers a string; the shell logs
//!   it. A `tracing::warn!` inside this function would make the one thing a test wants to
//!   assert — that the words name the address and the camera — reachable only through a
//!   subscriber, and would put a policy decision inside a side effect.
//! - **Whether to refuse to start.** D11's sentence about non-loopback prescribes a token
//!   and a warning, not a refusal, and this module enforces that sentence rather than a
//!   stricter one of its own: an operator who deliberately bound to an interface would
//!   otherwise meet a daemon that will not start and no flag that would make it. What errs
//!   closed here is the *token*, which is what D11 closes.

use std::net::{IpAddr, SocketAddr};

/// The one flag D11 names, spelled once.
///
/// [`Posture::warning`] names it when it did not apply, and the composition root's clap
/// argument is the other reader. Two spellings of a flag that turns a token off is the
/// defect this constant costs nothing to prevent: a listener asked for
/// `--http-insecure-loopback` and a `main` offering `--insecure-http-loopback` would be a
/// build where the flag silently never matched, which is the safe direction and still a lie
/// in the `--help` output.
pub const INSECURE_LOOPBACK_FLAG: &str = "--http-insecure-loopback";

/// How far a bind address reaches — D11's "loopback" and its complement.
///
/// A named value rather than a `bool` because both halves are read out loud: the token rule
/// branches on it and so does the warning, and `if !bind_is_loopback` at two call sites is
/// how the two come to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// This machine and nothing else — `127.0.0.0/8`, `::1`, or the IPv4-mapped form of
    /// either.
    Loopback,
    /// Somewhere another machine can reach: a real interface address, or one of the
    /// wildcards that means every interface this host has.
    BeyondLoopback,
}

impl Reach {
    /// Where an address reaches, with the IPv4-mapped hole this module's header describes
    /// closed.
    ///
    /// The unwrapping happens **before** the question rather than after it, so there is one
    /// answer to "is this loopback" and not one per address family.
    #[must_use]
    pub fn of(ip: IpAddr) -> Reach {
        let loopback = match ip {
            IpAddr::V4(v4) => v4.is_loopback(),
            // `is_loopback` on the v6 side is `::1` alone, so the mapped form is asked as
            // the IPv4 address it carries; `to_ipv4_mapped` answers `None` for everything
            // that is not `::ffff:a.b.c.d`, including the deprecated IPv4-compatible form,
            // which leaves those addresses to the v6 question and errs closed.
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
            }
        };
        if loopback {
            Reach::Loopback
        } else {
            Reach::BeyondLoopback
        }
    }
}

/// Whether the listener may serve a request that carries no token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRule {
    /// Every request is checked against the per-run token. D11's default, and the answer in
    /// three of the four cells.
    Required,
    /// Requests are served unauthenticated. Reachable only from loopback and only when the
    /// operator typed [`INSECURE_LOOPBACK_FLAG`] — D11's "one named explicit flag".
    NotRequired,
}

/// What the listener must do, decided from what the composition root knows.
///
/// Constructed only by [`Posture::of`], and its fields are private for that reason: the four
/// cells of D11's matrix are exactly the values this type has, and a caller that could build
/// `TokenRule::NotRequired` beside `Reach::BeyondLoopback` could build the one combination
/// the paragraph forbids.
///
/// [`Reach`] is carried rather than recomputed because the warning and the token rule are
/// two readings of one fact, and a value that answered them from two calls could answer them
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posture {
    bind: SocketAddr,
    reach: Reach,
    token: TokenRule,
    insecure_loopback_requested: bool,
}

impl Posture {
    /// D11's matrix, applied to the two values `--http` and `--http-insecure-loopback`
    /// produce.
    ///
    /// `insecure_loopback_requested` is whether the operator typed
    /// [`INSECURE_LOOPBACK_FLAG`], **not** whether it had an effect: the difference is the
    /// fourth cell, and [`Posture::insecure_loopback_was_inert`] is where it comes back out.
    /// A flag that was quietly dropped is how an operator ends up believing a token-gated
    /// listener is open, or an open one gated — and neither belief is one they can check by
    /// looking.
    #[must_use]
    pub fn of(bind: SocketAddr, insecure_loopback_requested: bool) -> Posture {
        let reach = Reach::of(bind.ip());
        // The whole matrix, in one expression, so that "there is no flag that removes it"
        // off loopback is a property of the shape rather than of a branch somebody has to
        // remember not to add.
        let token = match (reach, insecure_loopback_requested) {
            (Reach::Loopback, true) => TokenRule::NotRequired,
            (Reach::Loopback, false) | (Reach::BeyondLoopback, _) => TokenRule::Required,
        };
        Posture {
            bind,
            reach,
            token,
            insecure_loopback_requested,
        }
    }

    /// The address this posture was decided about.
    #[must_use]
    pub fn bind(&self) -> SocketAddr {
        self.bind
    }

    /// How far that address reaches.
    #[must_use]
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// Whether the listener checks a token — D11's matrix, as the listener reads it.
    #[must_use]
    pub fn token(&self) -> TokenRule {
        self.token
    }

    /// The same answer as a `bool`, for the middleware that has to branch on it.
    #[must_use]
    pub fn token_required(&self) -> bool {
        matches!(self.token, TokenRule::Required)
    }

    /// Whether the operator typed [`INSECURE_LOOPBACK_FLAG`], whatever it did.
    #[must_use]
    pub fn insecure_loopback_requested(&self) -> bool {
        self.insecure_loopback_requested
    }

    /// Whether they typed it and it did nothing.
    ///
    /// The fourth cell, named. An operator who asked for a token-less listener and got a
    /// token-gated one has to be *told*: the alternative is a person pasting a URL without a
    /// token into a browser, meeting a 401, and having no way to tell a flag that was
    /// ignored from a daemon that is broken.
    #[must_use]
    pub fn insecure_loopback_was_inert(&self) -> bool {
        self.insecure_loopback_requested && matches!(self.reach, Reach::BeyondLoopback)
    }

    /// D11's warning, or `None` when the paragraph does not ask for one.
    ///
    /// **It names the address and it names the camera**, because that is what D11 requires of
    /// it — "a warning naming what it exposes (a live camera)" — and because an operator who
    /// bound a daemon to `0.0.0.0` an hour ago needs to be told which daemon this is. A
    /// warning that said only "listening on a non-loopback address" would be true of half the
    /// processes on the machine.
    ///
    /// Loopback answers `None` in both of its cells. The token-less one is not silent in the
    /// shipped daemon — the composition root says what it is serving and how — but that is an
    /// *announcement* of a posture the operator chose by name, and this method is D11's
    /// warning about a posture that reaches past the machine. Spelling both as "the warning"
    /// would leave the daemon with one line that sometimes means "as you asked" and sometimes
    /// means "a live camera is on the network".
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match self.reach {
            Reach::Loopback => None,
            Reach::BeyondLoopback => {
                // Appended rather than written twice: the exposure sentence is the same
                // sentence in both non-loopback cells, and two copies of it is two places to
                // fix the day the wording changes.
                let inert = if self.insecure_loopback_requested {
                    format!(
                        "; {INSECURE_LOOPBACK_FLAG} was given and does not apply here — \
                         there is no flag that removes the token off loopback, so this \
                         listener is token-gated regardless"
                    )
                } else {
                    String::new()
                };
                Some(format!(
                    "serving the web client on {bind}, which is not loopback: anybody who \
                     can reach that address and present the bearer token can watch and \
                     drive a live camera on this machine{inert}",
                    bind = self.bind
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An address the way an operator spells it on the command line.
    fn bind(text: &str) -> SocketAddr {
        text.parse().expect("a socket address the tests wrote")
    }

    #[test]
    fn loopback_without_the_flag_needs_the_token_and_says_nothing() {
        // D11's first cell, and the default `--http` bind: "loopback + token is the
        // default".
        let posture = Posture::of(bind("127.0.0.1:0"), false);

        assert_eq!(posture.reach(), Reach::Loopback);
        assert_eq!(posture.token(), TokenRule::Required);
        assert!(posture.token_required());
        assert!(!posture.insecure_loopback_requested());
        assert!(!posture.insecure_loopback_was_inert());
        assert_eq!(posture.warning(), None);
    }

    #[test]
    fn loopback_with_the_named_flag_is_the_one_token_less_cell() {
        // D11's second cell: "token-less loopback exists only behind one named explicit
        // flag".
        let posture = Posture::of(bind("127.0.0.1:8080"), true);

        assert_eq!(posture.reach(), Reach::Loopback);
        assert_eq!(posture.token(), TokenRule::NotRequired);
        assert!(!posture.token_required());
        assert!(posture.insecure_loopback_requested());
        assert!(
            !posture.insecure_loopback_was_inert(),
            "the flag did exactly what it says here"
        );
        assert_eq!(posture.warning(), None);
    }

    #[test]
    fn a_non_loopback_bind_without_the_flag_needs_the_token_and_is_warned_about() {
        // D11's third cell: "non-loopback always requires the token … and additionally
        // prints a warning naming what it exposes (a live camera)".
        let posture = Posture::of(bind("192.168.1.10:8080"), false);

        assert_eq!(posture.reach(), Reach::BeyondLoopback);
        assert_eq!(posture.token(), TokenRule::Required);
        assert!(!posture.insecure_loopback_was_inert());

        let warning = posture.warning().expect("D11 requires a warning here");
        assert!(
            !warning.contains(INSECURE_LOOPBACK_FLAG),
            "no flag was given; naming one would send an operator looking for it: {warning}"
        );
    }

    #[test]
    fn a_non_loopback_bind_with_the_flag_still_needs_the_token_and_the_flag_is_inert() {
        // D11's fourth cell, and the one the paragraph is emphatic about: "there is no flag
        // that removes it". The listener still serves — D11 prescribes a token and a
        // warning, not a refusal — and the operator is told the flag did nothing.
        let posture = Posture::of(bind("192.168.1.10:8080"), true);

        assert_eq!(posture.reach(), Reach::BeyondLoopback);
        assert_eq!(
            posture.token(),
            TokenRule::Required,
            "a flag removed the token off loopback"
        );
        assert!(posture.insecure_loopback_requested());
        assert!(posture.insecure_loopback_was_inert());

        let warning = posture.warning().expect("D11 requires a warning here");
        assert!(
            warning.contains(INSECURE_LOOPBACK_FLAG),
            "an operator who typed the flag is left guessing why they met a 401: {warning}"
        );
        assert!(
            warning.contains("does not apply"),
            "naming the flag without saying it was inert is not saying anything: {warning}"
        );
    }

    #[test]
    fn the_warning_names_the_bind_address_and_the_camera() {
        // D11 asks for a warning "naming what it exposes (a live camera)", and an operator
        // needs to know *where* it is exposed. Both halves, asserted as words rather than as
        // "a warning exists".
        let address = bind("10.0.0.7:34567");
        let warning = Posture::of(address, false)
            .warning()
            .expect("a non-loopback bind is warned about");

        assert!(
            warning.contains(&address.to_string()),
            "the warning does not say where: {warning}"
        );
        assert!(
            warning.contains("camera"),
            "the warning does not say what is exposed: {warning}"
        );
    }

    #[test]
    fn an_ipv4_mapped_ipv6_loopback_address_is_loopback() {
        // The hole this module's header is about: `Ipv6Addr::is_loopback` answers `false`
        // here, and a dual-stack listener sees this address for every IPv4 client.
        let mapped = bind("[::ffff:127.0.0.1]:0");
        assert!(
            !mapped.ip().is_loopback(),
            "std changed its mind about `::ffff:127.0.0.1`; this test's subject has moved"
        );

        let posture = Posture::of(mapped, false);
        assert_eq!(posture.reach(), Reach::Loopback);
        assert_eq!(
            posture.warning(),
            None,
            "warning about an address only this machine can reach trains operators to ignore \
             the warning"
        );
        assert_eq!(
            Posture::of(mapped, true).token(),
            TokenRule::NotRequired,
            "the mapped form is loopback, so the named flag applies to it"
        );
    }

    #[test]
    fn the_whole_of_the_ipv4_loopback_block_is_loopback_and_not_just_dot_one() {
        // `127.0.0.0/8`, all sixteen million of it. A build that compared against
        // `127.0.0.1` would warn about this bind and demand a token the operator had not
        // asked for.
        let posture = Posture::of(bind("127.0.0.5:0"), true);
        assert_eq!(posture.reach(), Reach::Loopback);
        assert_eq!(posture.token(), TokenRule::NotRequired);
    }

    #[test]
    fn ipv6_loopback_is_loopback() {
        assert_eq!(Reach::of(bind("[::1]:0").ip()), Reach::Loopback);
    }

    #[test]
    fn the_wildcard_binds_are_not_loopback() {
        // `0.0.0.0` and `::` are what "expose it to the network" is spelled as, and the
        // mapped wildcard is the same address wearing the same disguise as the mapped
        // loopback one — so it must not come back through the unwrapping as loopback.
        for wildcard in ["0.0.0.0:0", "[::]:0", "[::ffff:0.0.0.0]:0"] {
            let posture = Posture::of(bind(wildcard), true);
            assert_eq!(
                posture.reach(),
                Reach::BeyondLoopback,
                "{wildcard} was treated as loopback"
            );
            assert_eq!(
                posture.token(),
                TokenRule::Required,
                "{wildcard} was served token-less"
            );
            assert!(posture.insecure_loopback_was_inert());
            assert!(
                posture.warning().is_some(),
                "{wildcard} was not warned about"
            );
        }
    }

    #[test]
    fn the_token_less_cell_is_the_only_one() {
        // The matrix as a property rather than as four cases: over every address these tests
        // know and both flag values, `NotRequired` happens exactly when the address is
        // loopback and the flag was given. A fifth cell added by accident fails here as well
        // as in whichever cell test it belongs to.
        let addresses = [
            "127.0.0.1:0",
            "127.0.0.5:8080",
            "[::1]:0",
            "[::ffff:127.0.0.1]:0",
            "0.0.0.0:0",
            "[::]:0",
            "[::ffff:0.0.0.0]:0",
            "192.168.1.10:8080",
            "[2001:db8::1]:8080",
        ];

        for text in addresses {
            for flag in [false, true] {
                let posture = Posture::of(bind(text), flag);
                let token_less = posture.token() == TokenRule::NotRequired;
                let expected = flag && posture.reach() == Reach::Loopback;
                assert_eq!(token_less, expected, "{text} with flag={flag}");
                assert_eq!(
                    posture.warning().is_some(),
                    posture.reach() == Reach::BeyondLoopback,
                    "{text} with flag={flag}: the warning does not follow the reach"
                );
            }
        }
    }
}
