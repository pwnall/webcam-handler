//! `webcam-handler-priv` — the privileged helper.
//!
//! A blessed copy of this binary carries `cap_sys_module` and `cap_net_admin`, which lets
//! the development loop load `vivid` (the R2 rung), cycle `uvcvideo` to make a real camera
//! vanish, and run a test process that can bind the uevent netlink socket — none of which
//! is otherwise possible without a human typing a sudo password.
//!
//! # The security boundary, stated plainly
//!
//! **`webcam-handler-priv exec` runs any program with `CAP_SYS_MODULE`, and `CAP_SYS_MODULE`
//! is root.** A process that can load a kernel module can do anything a kernel can do. So this
//! binary is not "a tool with some extra permissions" — a blessed copy is a root escalation
//! for whoever can execute it, and the *only* things standing between it and the rest of the
//! machine are:
//!
//! 1. **The file mode.** `just bless` chmods the blessed copy `0700` *before* it calls
//!    `setcap`, so only its owner can run it. This is the boundary, not a nicety.
//! 2. **The path.** The blessed copy lives at `.wch-bin/webcam-handler-priv`, gitignored,
//!    never inside `target/` (cargo rewrites those, and writing a file strips its
//!    capabilities).
//! 3. **Who has an account on the machine.** Nothing here defends against a second
//!    logged-in user who is also the owner.
//!
//! That is a smaller boundary than a closed verb vocabulary would have given, and it was
//! chosen deliberately (note N8): the exec wrapper is what lets a *test process* hold
//! `CAP_NET_ADMIN`, which no amount of verb design can do from outside.
//!
//! # Getting a blessed copy
//!
//! `just bless`, once. It needs sudo, and again only when this binary's own source
//! changes — the stamp is keyed on its sha256. There is no way to bootstrap that from
//! inside; a program cannot grant itself capabilities, and one that could would be a bug
//! in the kernel.
#![forbid(unsafe_code)]
// Every path here is request-driven and runs with root-equivalent capabilities. A panic
// in a blessed binary is a worse outcome than a refusal.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod caps;
mod modules;

use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};

/// Privileged helper for webcam-handler's development loop.
#[derive(Debug, Parser)]
#[command(name = "webcam-handler-priv", version, about, long_about = None)]
struct Cli {
    /// What to do.
    #[command(subcommand)]
    command: Verb,
}

#[derive(Debug, Subcommand)]
enum Verb {
    /// Report what this copy is blessed with and what it can therefore do.
    Doctor {
        /// Print only the `setcap` argument, so the justfile has one home for it.
        #[arg(long)]
        setcap_argument: bool,
    },

    /// Run a program with the capabilities, via the ambient set.
    ///
    /// Shaped for `cargo nextest`'s target-runner: point it at
    /// `[".wch-bin/webcam-handler-priv", "exec"]` and the test binary arrives as the first
    /// argument.
    Exec {
        /// The program, and everything to pass it.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
    },

    /// The `vivid` virtual capture driver (design §3.1's R2 rung).
    #[command(subcommand)]
    Vivid(VividVerb),

    /// The `uvcvideo` driver behind the real cameras.
    #[command(subcommand)]
    Uvcvideo(UvcVerb),
}

#[derive(Debug, Subcommand)]
enum VividVerb {
    /// Load it.
    Up {
        /// How many driver instances to create.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=8))]
        devices: u8,
    },
    /// Unload it.
    Down,
    /// Report whether it is loaded, and which nodes are present.
    Status {
        /// Emit JSON instead of a sentence.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum UvcVerb {
    /// Unload and reload it, so every real camera vanishes and returns.
    ///
    /// The only way to exercise `Error::DeviceGone` and the P4 hotplug path against
    /// hardware that is soldered to the motherboard.
    Cycle {
        /// Proceed even though a process holds a camera open.
        ///
        /// The refusal exists because pulling the driver out from under a video call is
        /// the kind of thing a dev tool does exactly once.
        #[arg(long)]
        force: bool,
    },
    /// Report whether it is loaded, and who is holding a camera.
    Status {
        /// Emit JSON instead of a sentence.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("webcam-handler-priv: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    match &cli.command {
        Verb::Doctor { setcap_argument } => doctor(*setcap_argument),
        Verb::Exec { argv } => exec(argv),
        Verb::Vivid(verb) => vivid(verb),
        Verb::Uvcvideo(verb) => uvcvideo(verb),
    }
}

/// `doctor` — the one verb that works unblessed, because its whole job is to say so.
fn doctor(setcap_argument: bool) -> Result<(), String> {
    if setcap_argument {
        println!("{}", caps::BLESSING);
        return Ok(());
    }

    let held = caps::Held::read().map_err(|error| error.to_string())?;
    println!("webcam-handler-priv {}", env!("CARGO_PKG_VERSION"));
    println!("  blessing:    {}", caps::BLESSING);
    println!(
        "  permitted:   {}",
        render_set(
            &held
                .permitted
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        )
    );
    println!(
        "  inheritable: {}",
        render_set(
            &held
                .inheritable
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        )
    );
    println!(
        "  ambient:     {}",
        held.ambient.as_ref().map_or_else(
            || "(this kernel has none — pre-4.3)".to_owned(),
            |set| render_set(&set.iter().map(String::as_str).collect::<Vec<_>>()),
        )
    );
    println!();

    // Delegation is *performed*, not predicted, and it is the only question worth asking:
    // nothing here acts on a module itself, so "can act" was a line about a capability we
    // never spend. `modprobe` is a subprocess, and a subprocess receives the ambient set
    // or nothing.
    //
    // Raising here costs nothing (the process exits straight after) and is the same call
    // every privileged verb makes, so a green line means the chain has actually run.
    if !held.can_act() {
        println!("  can delegate to a child:   no (not blessed)");
    } else {
        match caps::raise_ambient() {
            Ok(()) => {
                let after = caps::Held::read().map_err(|error| error.to_string())?;
                println!("  can delegate to a child:   yes (ambient raise verified)");
                println!(
                    "  ambient after raise:       {}",
                    after.ambient.as_ref().map_or_else(
                        || "(none)".to_owned(),
                        |set| render_set(&set.iter().map(String::as_str).collect::<Vec<_>>()),
                    )
                );
            }
            Err(error) => {
                println!("  can delegate to a child:   NO — {error}");
                return Ok(());
            }
        }
    }

    if !held.can_act() {
        println!();
        // Not an error exit: `doctor` answering "you are not blessed" is a successful
        // diagnosis, and a non-zero exit would make `just bless` unable to use it.
        println!("  Run `just bless` to fix this. It needs sudo once.");
        for purpose in held.missing() {
            println!("    missing: {purpose}");
        }
    }
    Ok(())
}

fn render_set(items: &[&str]) -> String {
    if items.is_empty() {
        "(none)".to_owned()
    } else {
        items.join(", ")
    }
}

/// `exec` — raise the capabilities into the ambient set, then become the target.
///
/// `exec` rather than spawn-and-wait: the child replaces us, so it keeps our pid, our
/// stdio, and our signal disposition. A wrapper that forked would have to forward signals
/// correctly to avoid leaving a test binary running after a Ctrl-C, and the simplest way
/// to get that right is not to fork.
fn exec(argv: &[String]) -> Result<(), String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| "exec needs a program to run".to_owned())?;

    caps::raise_ambient().map_err(|error| error.to_string())?;

    // `Command::exec` returns only on failure — on success this process is gone.
    let error = Command::new(program).args(args).exec();
    Err(format!("could not exec {program}: {error}"))
}

fn vivid(verb: &VividVerb) -> Result<(), String> {
    match verb {
        VividVerb::Up { devices } => {
            caps::raise_ambient().map_err(|error| error.to_string())?;
            let created = modules::load_vivid(*devices).map_err(|error| error.to_string())?;
            println!(
                "vivid: loaded ({devices} instance(s)); {} node(s) appeared: {}",
                created.len(),
                render_set(&created.iter().map(String::as_str).collect::<Vec<_>>())
            );
            Ok(())
        }
        VividVerb::Down => {
            caps::raise_ambient().map_err(|error| error.to_string())?;
            let removed = modules::unload_vivid().map_err(|error| error.to_string())?;
            println!(
                "vivid: unloaded; {} node(s) went away: {}",
                removed.len(),
                render_set(&removed.iter().map(String::as_str).collect::<Vec<_>>())
            );
            Ok(())
        }
        VividVerb::Status { json } => {
            // Deliberately needs no capability: asking is not doing, and `just` recipes
            // want to branch on this without a bless.
            let loaded = modules::is_loaded("vivid").map_err(|error| error.to_string())?;
            let nodes: Vec<String> = modules::video_nodes().into_iter().collect();
            if *json {
                println!(
                    r#"{{"module":"vivid","loaded":{loaded},"video_nodes":[{}]}}"#,
                    nodes
                        .iter()
                        .map(|n| format!("\"{n}\""))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            } else {
                println!(
                    "vivid: {}; {} video node(s) present: {}",
                    if loaded { "loaded" } else { "not loaded" },
                    nodes.len(),
                    render_set(&nodes.iter().map(String::as_str).collect::<Vec<_>>())
                );
            }
            Ok(())
        }
    }
}

fn uvcvideo(verb: &UvcVerb) -> Result<(), String> {
    match verb {
        UvcVerb::Cycle { force } => {
            caps::raise_ambient().map_err(|error| error.to_string())?;
            let cycled = modules::cycle_uvcvideo(*force).map_err(|error| error.to_string())?;

            println!(
                "uvcvideo: cycled; {} node(s) before, {} after",
                cycled.nodes_before.len(),
                cycled.nodes_after.len()
            );
            // Both halves reported, because a cycle where nothing vanished proved nothing
            // about `DeviceGone` and a caller must be able to tell.
            if !cycled.vanished {
                println!(
                    "  warning: the nodes never went away, so nothing was proved about a \
                     device disappearing"
                );
            }
            if !cycled.returned {
                println!(
                    "  warning: the nodes had not all returned within the settle deadline; \
                     `webcam-handler-priv uvcvideo status` will say whether they have since"
                );
            }
            if !cycled.holders_seen.is_empty() {
                println!(
                    "  note: forced past {} holder(s); they will have seen their camera \
                     disappear",
                    cycled.holders_seen.len()
                );
            }
            Ok(())
        }
        UvcVerb::Status { json } => {
            let loaded = modules::is_loaded("uvcvideo").map_err(|error| error.to_string())?;
            let holders = modules::video_holders();
            if *json {
                println!(
                    r#"{{"module":"uvcvideo","loaded":{loaded},"holders":[{}]}}"#,
                    holders
                        .iter()
                        .map(|h| format!(
                            r#"{{"pid":{},"comm":{},"node":"{}"}}"#,
                            h.pid,
                            h.comm
                                .as_ref()
                                .map_or_else(|| "null".to_owned(), |c| format!("\"{c}\"")),
                            h.node
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            } else {
                println!(
                    "uvcvideo: {}; {} camera holder(s)",
                    if loaded { "loaded" } else { "not loaded" },
                    holders.len()
                );
                for holder in &holders {
                    println!(
                        "  {} held by {} (pid {})",
                        holder.node,
                        holder.comm.as_deref().unwrap_or("an unreadable process"),
                        holder.pid
                    );
                }
                if holders.is_empty() {
                    // The distinction the scan cannot make, said out loud.
                    println!(
                        "  (no holder seen — /proc/<pid>/fd is unreadable for other users' \
                         processes, so this is not proof that nobody has one open)"
                    );
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn the_command_tree_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn exec_takes_everything_after_it_verbatim_including_flags() {
        // The target-runner contract: nextest appends the test binary and its arguments,
        // and those routinely start with `-`. A parser that ate them would silently run
        // the test with the wrong arguments.
        let cli = Cli::try_parse_from([
            "webcam-handler-priv",
            "exec",
            "/path/to/test-binary",
            "--exact",
            "--nocapture",
        ])
        .expect("parses");
        let Verb::Exec { argv } = cli.command else {
            panic!("expected exec");
        };
        assert_eq!(argv, vec!["/path/to/test-binary", "--exact", "--nocapture"]);
    }

    #[test]
    fn exec_with_no_program_is_refused_by_the_parser() {
        assert!(Cli::try_parse_from(["webcam-handler-priv", "exec"]).is_err());
    }

    #[test]
    fn the_device_count_is_range_checked_at_the_parser() {
        // A device count is device-facing input even here: `n_devs=0` loads a driver with
        // no nodes, and a large one floods /dev.
        assert!(
            Cli::try_parse_from(["webcam-handler-priv", "vivid", "up", "--devices", "0"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["webcam-handler-priv", "vivid", "up", "--devices", "9"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["webcam-handler-priv", "vivid", "up", "--devices", "8"]).is_ok()
        );

        let cli = Cli::try_parse_from(["webcam-handler-priv", "vivid", "up"]).expect("parses");
        let Verb::Vivid(VividVerb::Up { devices }) = cli.command else {
            panic!("expected vivid up");
        };
        assert_eq!(devices, 1, "the default must be the least disruptive one");
    }

    #[test]
    fn cycling_requires_an_explicit_flag_to_override_the_interlock() {
        let cli =
            Cli::try_parse_from(["webcam-handler-priv", "uvcvideo", "cycle"]).expect("parses");
        let Verb::Uvcvideo(UvcVerb::Cycle { force }) = cli.command else {
            panic!("expected cycle");
        };
        assert!(!force, "forcing past a live camera must never be a default");
    }

    #[test]
    fn there_is_no_verb_that_takes_a_module_name() {
        // The one property that survives choosing the exec wrapper: `exec` already grants
        // root, so a `modprobe <anything>` verb would add no privilege — but it would add
        // a *second*, quieter way to reach it, and one that reads like a safe utility in
        // a shell history. The module verbs name their module at compile time.
        let rendered = format!("{:?}", Cli::command().render_long_help());
        for forbidden in ["--module", "<MODULE>", "modprobe"] {
            assert!(
                !rendered.contains(forbidden),
                "the help surface exposes {forbidden}"
            );
        }
    }

    #[test]
    fn status_verbs_work_without_a_blessing() {
        // Asking is not doing. `just` recipes branch on `vivid status` before deciding
        // whether a bless is even needed, so requiring one here would be circular.
        assert!(vivid(&VividVerb::Status { json: true }).is_ok());
        assert!(uvcvideo(&UvcVerb::Status { json: true }).is_ok());
    }

    #[test]
    fn the_privileged_verbs_refuse_when_unblessed_and_say_how_to_fix_it() {
        // The test binary is never blessed, so this is the real unblessed path.
        if caps::Held::read().is_ok_and(|h| h.can_act()) {
            return;
        }
        // Every privileged verb, not just `exec`: they all delegate to a subprocess, so
        // they all need the same thing, and the one that checked a weaker precondition
        // was the one that failed against a real kernel.
        let error = vivid(&VividVerb::Up { devices: 1 }).expect_err("unblessed cannot load");
        assert!(error.contains("just bless"), "{error}");
        let error = vivid(&VividVerb::Down).expect_err("unblessed cannot unload");
        assert!(error.contains("just bless"), "{error}");
        let error = uvcvideo(&UvcVerb::Cycle { force: true }).expect_err("unblessed cannot cycle");
        assert!(error.contains("just bless"), "{error}");
        let error = exec(&["/bin/true".to_owned()]).expect_err("unblessed cannot delegate");
        assert!(error.contains("just bless"), "{error}");
    }
}
