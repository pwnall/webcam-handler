//! `webcam-handler-client` — the daemon CLI client. Links no backend and no engine.
//!
//! The second of the two consumers of the one command surface (T4). Everything the user sees
//! comes from `webcam-handler-cli-core`, exactly as it does for `webcam-handler-cli`; this
//! crate contributes an [`Executor`](cli_core::Executor) that speaks the T5 wire instead of
//! driving a camera, and the process's edges — argument parsing, the socket it connects to,
//! the exit code, and turning a typed error into a line on standard error.
//!
//! | File | Home of |
//! |---|---|
//! | [`transport`] | the socket and the upgrade: jsonrpsee's two transport traits over `soketto` on a `UnixStream` |
//! | [`remote`] | the T4 executor over the generated T5 client, including the sweep's state machine |
//! | this file | what is parsed, what is refused, where the socket is, and how long an answer is waited for |
//! | `src/main.rs` | the composition root: four lines, so the rest of it is reachable from a test |
//!
//! ## Why there is a library at all
//!
//! For `crates/daemon`'s reason, which its own manifest states — a lib "so integration tests
//! can drive a real server". The half of this client that cannot be asserted from outside a
//! subprocess is [`remote::Remote`]'s `calibrate_sweep`: its progress goes to an
//! [`cli_core::SweepWatcher`], and the shipped watcher is an indicatif bar that draws
//! **nothing** when standard error is not a terminal (measured: a piped `calibrate sweep`
//! prints only the two notes `cli_core` writes itself). A suite that could only see a
//! subprocess's output could therefore assert that a sweep *answered* and never that its
//! events arrived — which is the one property the ordering in that method exists to provide.
//! So the executor is reachable as a library, a test hands it a recording watcher, and the
//! binary below is left with nothing in it but the process's edges.
//!
//! ## The thin-client wall
//!
//! `webcam-handler-client` links no backend and no engine (T6), and
//! `scripts/gates/dependency-walls.sh` asserts it from the resolved graph. That is why the
//! socket's location is read from `schema::paths::runtime_dir` — moved into the schema crate
//! at P4f's opening commit precisely so a client could reach it without linking a session
//! store to find a file descriptor.
//!
//! ## What this root refuses, and why refusing is the answer
//!
//! `--backend` and `--profile` are **global** flags on the shared [`Cli`], because the surface
//! is shared and a verb exists once (T4). `webcam-handler-client` cannot honour either: the
//! daemon chose its backend at *its own* composition root — `webcam-handler-daemon --backend
//! fake --profile …` — and a client on the other end of a socket cannot change what a running
//! process is driving.
//!
//! Three answers were available and two of them are worse:
//!
//! - **Fork the surface**, so `webcam-handler-client` has its own flagless root. That is the
//!   one thing T4 forbids — the parity gate scrapes `--help` for its verb population, so a
//!   forked tree would be a gate comparing a surface with itself (`Program`'s own doc says
//!   this).
//! - **Ignore them.** `webcam-handler-client --backend fake` would then look like it did
//!   something, and a script pointed at a daemon serving real cameras would believe it was
//!   replaying a profile. Silently doing the opposite of what was typed is the worst of the
//!   three.
//! - **Refuse, naming where the decision lives.** The refusal says `webcam-handler-daemon
//!   --backend`, so the reader is sent to the process that can actually answer.
//!
//! It is a **typed** error rather than a clap usage error, and that is a deliberate reading
//! of the exit codes `cli_core::exit_code` fixes: the command line is well-formed — every
//! flag is spelled correctly and every value is in vocabulary — and what refuses it is a
//! fact about the world, which is exactly the line between exit 2 and exit 1. A script that
//! branches on "you typed it wrong" must not have to handle this one there.
#![forbid(unsafe_code)]
// The client is request-driven end to end: every path here is reachable from a command line
// somebody else wrote, and every value it renders arrived off a socket.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod remote;
pub mod transport;

use std::time::Duration;

use camino::Utf8PathBuf;
use cli_core::{Cli, Command, Output, Program};
use schema::error::{Error, Result};
use schema::limits;
use schema::paths::Env;

/// Which root this is.
///
/// The command surface is shared with `webcam-handler-cli` (T4), so the name is a parameter
/// rather than a property of the tree — see [`Program`]. One value, read by every edge of this
/// process that has to say it: the parser that renders `--help` and `--version`, the lines a
/// failing run writes to standard error, and the probe notes `controls --discover-pairs`
/// prints.
pub const PROGRAM: Program = Program::Client;

/// Connect, and run one verb.
///
/// `env` is a parameter for [`schema::paths`]'s reason — `std::env::set_var` is a data race
/// and this crate forbids `unsafe`, so the process environment is read at exactly one place
/// (`src/main.rs`) and handed down as a value.
///
/// # Errors
///
/// The refusal of the flags this client cannot honour, whatever connecting said, or whatever
/// the daemon answered — all typed, all rendered by the caller.
pub fn run(cli: &Cli, env: &dyn Env, out: &mut Output) -> Result<()> {
    refuse_composition_flags(cli)?;
    // A document verb takes files and answers a document (design §2.7, D15): it touches no
    // camera, no store and no socket, so it runs in this process and no daemon has to be
    // running for it. Asked here, before the socket is composed, for the reason the flag
    // refusal above is asked before it: a verb that needs nothing from a daemon must not
    // report that one is missing. The verbs that *do* need one are unaffected — this answers
    // `None` for every one of them.
    if let Some(answered) = cli_core::below_the_executor(cli, out) {
        return answered;
    }
    let socket = socket(env)?;
    let mut executor = remote::Remote::connect(&socket, request_timeout(&cli.command))?;
    cli_core::run(cli, &mut executor, out)
}

/// Refuse the two flags that name a *composition root's* decision, which this root is not.
///
/// The module header argues the decision; this is where it happens, and it happens **before**
/// the socket is touched, so `webcam-handler-client --backend fake list` says what is wrong
/// with it rather than reporting that no daemon is running.
///
/// # Errors
///
/// [`Error::IllegalTransition`] naming the flag and the program that owns it. That variant
/// because the command line describes a state transition this process cannot make — the same
/// reading `cli_core`'s own argument-group refusals take (`PlanArgs::spec`,
/// `SelectorArgs::selection`), and it keeps the D13 registry closed: P4f adds no nineteenth
/// variant.
fn refuse_composition_flags(cli: &Cli) -> Result<()> {
    // The *provenance*, not the value: `--backend` carries a default, so `cli.backend` always
    // holds one — see `Cli::backend_was_chosen`. Refusing on the value would let
    // `webcam-handler-client --backend v4l2` through while the daemon replayed a profile,
    // which is a client agreeing with a claim it cannot check.
    if cli.backend_was_chosen() {
        return Err(refused(
            "--backend",
            "the daemon chose its backend when it started",
        ));
    }
    if !cli.profile.is_empty() {
        return Err(refused(
            "--profile",
            "the profiles a fake backend replays are the daemon's, read at its own startup",
        ));
    }
    Ok(())
}

/// One refusal, so both flags are refused in one shape and both name the same place.
fn refused(flag: &str, because: &str) -> Error {
    Error::IllegalTransition {
        from: format!("{PROGRAM} is a client"),
        op: format!(
            "honour {flag}: {because}. Start the daemon with the backend you want \
             (`webcam-handler-daemon {flag} …`), or run the verb with `webcam-handler-cli`, \
             which drives a camera in this process"
        ),
    }
}

/// Where the daemon's socket is, per design D11.
///
/// Composed from the two homes that own its halves — [`schema::paths::runtime_dir`] for the
/// directory and [`limits::DAEMON_SOCKET_FILE`] for the name — rather than written out, so
/// `webcam-handler-daemon` and `webcam-handler-client` cannot come to disagree about a path
/// they both have to know.
///
/// The environment is a parameter for [`schema::paths`]'s reason: `std::env::set_var` is a
/// data race and this crate forbids `unsafe`, so a test that wants a different runtime
/// directory hands one over instead of mutating the process.
///
/// # Errors
///
/// [`Error::StorageIo`] naming `$XDG_RUNTIME_DIR` when it is unset, empty or relative. That
/// is a different failure from "no daemon is listening" and it reads differently on purpose:
/// one says the socket has nowhere to be, the other names the path nothing answered on.
fn socket(env: &dyn Env) -> Result<Utf8PathBuf> {
    Ok(schema::paths::runtime_dir(env)?.join(limits::DAEMON_SOCKET_FILE))
}

/// How long this invocation waits for its answer.
///
/// One verb per invocation, so the budget is a property of the verb — and there are exactly
/// two, because there is exactly one verb whose latency is unbounded by design.
/// `webcam-handler-api`'s `wch_calibrate_sweep` asks a client to raise its timeout rather
/// than leave it at jsonrpsee's sixty-second default; this is where that is honoured, and
/// both numbers are priced in [`schema::limits`] against the caps they have to outlast.
///
/// A `match` on the verb rather than a maximum applied to all of them: a
/// `webcam-handler-client list` that hung for an hour because a sweep might have needed one
/// would be this client refusing to tell an operator that their daemon has stopped answering.
fn request_timeout(command: &Command) -> Duration {
    match command {
        Command::Calibrate(cli_core::CalibrateCommand::Sweep { .. }) => {
            Duration::from_millis(limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS)
        }
        // **`record` takes the ordinary budget, and this arm exists to say that it was
        // chosen.** The invocation runs for as long as the take does — up to
        // [`limits::MAX_RECORDING_MS`] — and none of the calls it makes does: `record_start`
        // returns once the container's header is on disk, `record_status` is a read, and
        // `record_stop` waits for the driver's current turn and the close. What runs long is
        // the **loop between them** (`remote::poll_until_ended`), and jsonrpsee's timeout
        // bounds a request rather than a verb.
        //
        // The trap this arm defuses is a coincidence: `CLIENT_REQUEST_TIMEOUT_MS` and
        // `MAX_RECORDING_MS` are both 120 000 today, so a reader raising the recording cap
        // would find a number that looks like it has to move with it. It does not, and a
        // budget derived from the cap would be the sweep's mistake in miniature — a client
        // that could not tell an unresponsive daemon from a long recording, on the verb whose
        // whole point is measuring how long something took.
        Command::Record { .. } => Duration::from_millis(limits::CLIENT_REQUEST_TIMEOUT_MS),
        _ => Duration::from_millis(limits::CLIENT_REQUEST_TIMEOUT_MS),
    }
}

#[cfg(test)]
mod tests {
    use schema::paths::{MapEnv, XDG_RUNTIME_DIR};

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_checked_from(PROGRAM, args).expect("a well-formed command line")
    }

    #[test]
    fn the_socket_is_the_one_the_daemon_binds_and_says_so_when_there_is_nowhere_to_put_it() {
        // Composed from the same two homes `webcam-handler-daemon` composes it from — the
        // assertion is against those, not against a literal, so a fixture cannot pin a path
        // the daemon would not bind.
        let env = MapEnv::from_pairs(&[(XDG_RUNTIME_DIR, "/run/user/1000")]);
        assert_eq!(
            socket(&env).expect("the variable is set"),
            schema::paths::runtime_dir(&env)
                .expect("the variable is set")
                .join(limits::DAEMON_SOCKET_FILE)
        );

        // And the other direction, which is the failure an operator actually meets: no
        // login session, so no directory that is private to them. It names the variable
        // rather than guessing at `/tmp`, which is `schema::paths`' decision and this only
        // has to not swallow it.
        let nowhere = socket(&MapEnv::empty()).expect_err("no runtime directory");
        assert_eq!(nowhere.kind(), schema::ErrorKind::StorageIo);
        assert!(nowhere.to_string().contains(XDG_RUNTIME_DIR), "{nowhere}");
    }

    #[test]
    fn the_two_flags_a_client_cannot_honour_are_refused_by_provenance_and_the_rest_are_not() {
        // `--backend v4l2` is the *default* value, typed. Refusing it is the whole point of
        // reading the provenance rather than the value: a client that accepted the spelling
        // that happens to match the default would be agreeing with a claim it cannot check.
        for args in [
            ["webcam-handler-client", "--backend", "v4l2", "list"].as_slice(),
            [
                "webcam-handler-client",
                "--backend",
                "fake",
                "--profile",
                "p.json",
                "list",
            ]
            .as_slice(),
        ] {
            let error = refuse_composition_flags(&parse(args)).expect_err("--backend is refused");
            assert_eq!(error.kind(), schema::ErrorKind::IllegalTransition);
            let rendered = error.to_string();
            assert!(rendered.contains("--backend"), "{rendered}");
            // The refusal sends the reader to the process that can answer.
            assert!(rendered.contains("webcam-handler-daemon"), "{rendered}");
        }

        // `--profile` alone, which clap does not tie to `--backend` in that direction.
        let error = refuse_composition_flags(&parse(
            ["webcam-handler-client", "--profile", "p.json", "list"].as_ref(),
        ))
        .expect_err("--profile is refused");
        assert_eq!(error.kind(), schema::ErrorKind::IllegalTransition);
        assert!(error.to_string().contains("--profile"), "{error}");

        // The inverse arm, without which every command line would be refused and the two
        // assertions above would prove nothing: an ordinary invocation passes, and so does
        // one carrying every *other* global flag.
        assert!(refuse_composition_flags(&parse(&["webcam-handler-client", "list"])).is_ok());
        assert!(
            refuse_composition_flags(&parse(&["webcam-handler-client", "--json", "list"])).is_ok()
        );
        assert!(
            refuse_composition_flags(&parse(&[
                "webcam-handler-client",
                "photo",
                "cam:x",
                "--wait"
            ]))
            .is_ok(),
            "--wait is this client's flag, not the daemon's"
        );
    }

    #[test]
    fn only_the_sweep_gets_the_long_budget_and_every_other_verb_gets_the_short_one() {
        // The two constants, reached through the verb that chooses them. A build that applied
        // the sweep's hour to everything would leave `webcam-handler-client list` unable to
        // tell an operator that their daemon is gone.
        let sweep = parse(&[
            "webcam-handler-client",
            "calibrate",
            "sweep",
            "cam:x",
            "--task",
            "t",
            "focus_absolute",
            "--all",
        ]);
        assert_eq!(
            request_timeout(&sweep.command),
            Duration::from_millis(limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS)
        );

        for args in [
            ["webcam-handler-client", "list"].as_slice(),
            ["webcam-handler-client", "photo", "cam:x", "--wait"].as_slice(),
            [
                "webcam-handler-client",
                "record",
                "cam:x",
                "-o",
                "/tmp/take.avi",
                "--duration",
                "2m",
            ]
            .as_slice(),
            [
                "webcam-handler-client",
                "calibrate",
                "status",
                "cam:x",
                "--task",
                "t",
            ]
            .as_slice(),
        ] {
            assert_eq!(
                request_timeout(&parse(args).command),
                Duration::from_millis(limits::CLIENT_REQUEST_TIMEOUT_MS),
                "{args:?}"
            );
        }
        // Not vacuous: the two budgets differ, so the walk above distinguishes them.
        assert_ne!(
            limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS,
            limits::CLIENT_REQUEST_TIMEOUT_MS
        );

        // **`record` is in that list on purpose and the property is not an equality.** Its
        // *invocation* may run for `limits::MAX_RECORDING_MS` and none of its three calls does
        // — jsonrpsee's timeout bounds a request, not a verb — so the budget it needs is the
        // one that outlasts the longest single call in the sequence: a `record_start` that
        // waited for the camera, plus the frame a `record_stop` waits behind. Asserted against
        // *that* arithmetic rather than against the constant the code names, because a test
        // comparing a value to the constant that produced it agrees with itself forever.
        let record = parse(&[
            "webcam-handler-client",
            "record",
            "cam:x",
            "-o",
            "/tmp/take.avi",
        ]);
        assert!(
            request_timeout(&record.command)
                > Duration::from_millis(limits::CAMERA_ENQUEUE_WAIT_MS + limits::FRAME_DEADLINE_MS),
            "a record call could time out on a daemon that was about to answer it"
        );
        assert!(
            request_timeout(&record.command)
                < Duration::from_millis(limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS),
            "record took the sweep's hour; a client that cannot tell a dead daemon from a \
             long recording is the failure this budget exists to prevent"
        );
    }
}
