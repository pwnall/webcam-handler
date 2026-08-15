//! `webcam-handler-priv` — the privileged helper.
//!
//! A blessed copy of this binary carries `cap_sys_module`, which lets the development loop
//! load `vivid` (the R2 rung) and cycle `uvcvideo` to make a real camera vanish — neither of
//! which is otherwise possible without a human typing a sudo password.
//!
//! # The security boundary, stated plainly
//!
//! **`CAP_SYS_MODULE` is root.** A process that can load a kernel module can do anything a
//! kernel can do, so a blessed copy is still a root escalation for whoever can execute it,
//! and this is still not "a tool with some extra permissions". Three things stand between it
//! and the rest of the machine, and the first is the boundary:
//!
//! 1. **The file mode.** `just bless` chmods the blessed copy `0700` *before* it calls
//!    `setcap`, so only its owner can run it. This is the boundary, not a nicety.
//! 2. **The path.** The blessed copy lives at `.wch-bin/webcam-handler-priv`, gitignored,
//!    never inside `target/` (cargo rewrites those, and writing a file strips its
//!    capabilities).
//! 3. **Who has an account on the machine.** Nothing here defends against a second
//!    logged-in user who is also the owner.
//!
//! # What a blessed copy will and will not do for whoever runs it (note N125)
//!
//! Until P6e there was a fourth sentence here, and it said that this binary would run **any
//! program** with `CAP_SYS_MODULE` — `webcam-handler-priv exec /bin/sh` was a root shell, and
//! note N8 records the owner choosing that shape over a closed verb vocabulary on one
//! argument: only a wrapper can put a capability inside a *test process*. G6 was the recorded
//! trigger to check whether that argument had ever been spent, and the answer was no: nothing
//! in this workspace ever invoked `exec`, the hotplug rung that was supposed to need it runs
//! unprivileged \[PF:21\], and the verb is gone.
//!
//! So the blast radius is now the verb list and nothing wider: **two modules, named at
//! compile time, loaded and unloaded.** The capabilities this binary holds reach exactly one
//! other program — `/usr/sbin/modprobe`, with an argument list this crate writes — and there
//! is no path from a caller's argv to a program name. That is a claim a check can hold rather
//! than a risk somebody accepted, which is what `no_verb_hands_this_binarys_capabilities_to_a_program_the_caller_names`
//! and `scripts/gates/privileged-helper.sh`'s fifth claim are for.
//!
//! What it still cannot defend against is unchanged and is worth keeping in front of a
//! reader: `modprobe vivid` is a kernel module load, and a kernel module load is arbitrary
//! code in ring 0. Narrowing the verbs narrowed *who can ask*, never *what a yes costs*.
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

use std::process::ExitCode;

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

    /// Every command in the tree, root included, depth-first, **built**.
    ///
    /// Derived from clap rather than listed, for `phase-criteria.tsv`'s reason: a verb added
    /// to the enum joins this population without anybody remembering to register it, and a
    /// population somebody maintains by hand is the one that stops covering the verb that
    /// mattered.
    ///
    /// `build()` before the walk, and it is load-bearing rather than tidy: an unbuilt
    /// `Command` answers `None` to `get_num_args` for every argument, because the arity is
    /// derived from the action during the build. **The direction that costs is the green
    /// one**, which was measured rather than assumed (note **N125**, mutant M4): the arity
    /// assertion below spells its refusal `is_some_and`, so an unknown arity fails rather
    /// than passing, and dropping this line turns the *shipped* tree red on `doctor`'s
    /// `--setcap-argument` flag. A claim that cannot be evaluated is not a claim that holds,
    /// and that is the only reading of `None` this test will accept.
    fn every_command() -> Vec<clap::Command> {
        fn walk(command: &clap::Command, out: &mut Vec<clap::Command>) {
            out.push(command.clone());
            for sub in command.get_subcommands() {
                walk(sub, out);
            }
        }
        let mut root = Cli::command();
        root.build();
        let mut all = Vec::new();
        walk(&root, &mut all);
        all
    }

    #[test]
    fn no_verb_hands_this_binarys_capabilities_to_a_program_the_caller_names() {
        // **The claim the P6e narrowing bought, and the defect class it exists to catch: an
        // `exec` verb coming back.** Note N8 recorded the owner choosing a generic wrapper
        // over a closed verb vocabulary, so until P6e this property was *false by design* and
        // nothing could assert it. The G6 reckoning measured the argument that bought the
        // wrapper — "only a wrapper can put a capability inside a test process" — found that
        // nothing had ever spent it, and deleted the verb (note **N125**). What was an
        // accepted risk is now a checkable sentence, so it is checked.
        //
        // Structural rather than by name, because "no verb called `exec`" is a rule somebody
        // satisfies with a verb called `run`. A wrapper needs two things from its parser and
        // cannot work without either: an **unbounded** list of values, and tolerance for
        // values that begin with `-` (a test binary arrives with `--exact --nocapture`). So
        // those two are what this refuses, at every depth and in each of the spellings clap
        // has for them — the setting on a command, the same setting on an argument, and the
        // arity that survives when neither is written down.
        let commands = every_command();
        assert!(
            commands.len() >= 4,
            "the walk found {} command(s); the tree has a root, `doctor`, and two module \
             verbs with subcommands under each, so a population this small is a walk that \
             stopped rather than a tree that shrank",
            commands.len()
        );
        let mut arguments = 0;
        for command in &commands {
            let name = command.get_name();
            assert!(
                !command.is_trailing_var_arg_set(),
                "`{name}` collects trailing arguments verbatim, which is the shape a program \
                 runner needs"
            );
            for arg in command.get_arguments() {
                arguments += 1;
                let id = arg.get_id();
                assert!(
                    !arg.is_allow_hyphen_values_set(),
                    "`{name}`'s `{id}` accepts values beginning with `-`, so a caller can \
                     pass another program's flags through it"
                );
                assert!(
                    !arg.is_trailing_var_arg_set(),
                    "`{name}`'s `{id}` is a trailing var-arg — an argv, whatever it is called"
                );
                let bound = arg.get_num_args().map(|range| range.max_values());
                assert!(
                    bound.is_some_and(|max| max <= 1),
                    "`{name}`'s `{id}` takes an unbounded number of values ({bound:?}); every \
                     argument this binary accepts is a flag or one bounded value"
                );
            }
        }
        assert!(
            arguments >= 4,
            "only {arguments} argument(s) examined; the claim is about arguments, so a walk \
             that found none would be green having checked nothing"
        );
    }

    #[test]
    fn there_is_no_verb_that_takes_a_module_name_or_a_path() {
        // The other half, and the older one: a `modprobe <anything>` verb would add no
        // privilege over `vivid up` — it is the same capability — but it would add a
        // *second*, quieter way to reach it, one that reads like a safe utility in a shell
        // history. The module verbs name their module at compile time. Since P6e the same
        // sentence covers program names and paths, because the verb that took one is gone
        // and its absence is a property worth pinning by name as well as by shape.
        //
        // Every command's own long help, not just the root's: the root lists its
        // subcommands' one-line abouts and nothing of their arguments, so a `--module` flag
        // three levels down is invisible to a check that reads one page.
        for mut command in every_command() {
            let name = command.get_name().to_owned();
            let rendered = format!("{:?}", command.render_long_help());
            for forbidden in [
                "--module",
                "<MODULE>",
                "modprobe",
                "<PROGRAM>",
                "<ARGV>",
                "--exec",
            ] {
                assert!(
                    !rendered.contains(forbidden),
                    "`{name}`'s help surface exposes {forbidden}"
                );
            }
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
        // Every privileged verb: they all delegate to a subprocess, so they all need the
        // same thing, and the one that checked a weaker precondition was the one that failed
        // against a real kernel. Since P6e that is the whole list rather than three of four
        // — the fourth was `exec`, and note **N125** records that its refusal is the only
        // part of it anybody ever exercised.
        let error = vivid(&VividVerb::Up { devices: 1 }).expect_err("unblessed cannot load");
        assert!(error.contains("just bless"), "{error}");
        let error = vivid(&VividVerb::Down).expect_err("unblessed cannot unload");
        assert!(error.contains("just bless"), "{error}");
        let error = uvcvideo(&UvcVerb::Cycle { force: true }).expect_err("unblessed cannot cycle");
        assert!(error.contains("just bless"), "{error}");
    }
}
