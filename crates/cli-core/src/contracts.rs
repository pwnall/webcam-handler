//! What each verb answers with, and what the surface's verbs are called.
//!
//! Two questions, one module, because they are two halves of one claim: the population of
//! verbs is the clap tree's, and the `--json` contract is a fact about each member of that
//! population. Splitting them would let a table of contracts quantify over a set nothing
//! compares it against, which is the shape the mapping had until docs/7 P6e — written down
//! in `scripts/gates/json-validates.sh` and nowhere else, kept honest only by that gate's
//! own `--help` walk.
//!
//! ## Why the table is a file rather than a `const` array
//!
//! Three readers, and only one of them is Rust. `webcam-handler-xtask` prints the mapping
//! into the generated agent guide (docs/7 P6e), `json-validates.sh` validates each verb's
//! real answer against the `$defs` entry named in it, and this crate's own test proves the
//! rows and the clap tree name the same verbs. A `const` array here would make the shell
//! reader parse Rust; a table in the gate makes the Rust readers parse shell. A tab-separated
//! file is the one shape all three read without either of those, which is the same argument
//! `scripts/gates/phase-criteria.tsv` already carries for its two readers.
//!
//! ## The verb spelling
//!
//! Dash-joined, so `calibrate sweep` is `calibrate-sweep`. That is the gate's spelling and
//! it is kept rather than improved: the gate derives its population by scraping `--help` at
//! two levels and keys its rows this way, so a second spelling here would mean a lookup that
//! silently missed — and a missing row reads as "this verb has no contract", which is the
//! defect rather than the report of one.

use crate::Program;

/// Which schema document one verb's `--json` emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonContract {
    /// The verb, dash-joined (`calibrate-sweep`).
    pub verb: &'static str,
    /// The type it answers with, as the committed bundle keys it under `$defs`.
    pub document: &'static str,
}

/// The contract table, verbatim.
///
/// Public because `json-validates.sh` reads the same *file* and the agent guide is generated
/// from the same *bytes*: a consumer that wanted to re-parse it differently should be reading
/// what the other two read rather than a copy of it.
pub const JSON_CONTRACTS_TSV: &str = include_str!("../json-contracts.tsv");

/// The `--json` contract of every verb, in the table's order.
///
/// Comment lines (`#`) and blank lines are skipped; anything else is a row and is expected to
/// carry a tab. A line that does not is *dropped* rather than refused, because this function
/// has no way to report — and `every_row_of_the_contract_table_is_a_row` is what makes the
/// drop impossible to miss: it counts the lines that look like rows and the rows that parsed,
/// and a difference is a red test rather than a verb that quietly lost its contract.
pub fn json_contracts() -> impl Iterator<Item = JsonContract> {
    contract_rows().filter_map(|line| {
        line.split_once('\t').map(|(verb, document)| JsonContract {
            verb: verb.trim(),
            document: document.trim(),
        })
    })
}

/// Every line of the table that is meant to be a row.
///
/// Separate from the parse so the test can count both sides of it.
fn contract_rows() -> impl Iterator<Item = &'static str> {
    JSON_CONTRACTS_TSV
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// The document `verb` answers with, or `None` for a verb with no contract.
#[must_use]
pub fn json_contract(verb: &str) -> Option<&'static str> {
    json_contracts()
        .find(|contract| contract.verb == verb)
        .map(|contract| contract.document)
}

/// Every verb the command surface offers, dash-joined, in the tree's declaration order.
///
/// Walked from the clap tree rather than listed, which is the whole point: the generated
/// agent guide documents this population, the contract table above is compared against it,
/// and a verb that existed in neither of those while being reachable from a command line is
/// the drift docs/7 P6e commissions a gate against.
///
/// The tree is [`Program::Cli`]'s because the two roots share one (T4) — `each_root_announces
/// _its_own_name_over_the_one_shared_tree` is the assertion that keeps that true, so which
/// one is asked here is arbitrary and stated rather than left to look like a choice.
///
/// clap's own `help` subcommand is excluded: it is generated machinery rather than a verb of
/// this surface, it answers with no document, and the gate's `--help` scrape drops it for the
/// same reason.
#[must_use]
pub fn verbs() -> Vec<String> {
    let mut found = Vec::new();
    collect_verbs(&Program::Cli.command(), "", &mut found);
    found
}

/// [`verbs`]'s recursion: a node a caller can run is a verb, and a node with children is also
/// a prefix.
///
/// **The two are not exclusive, and `photo` is why** (D17). `photo <CAMERA>` takes a photo and
/// `photo diff <A> <B>` compares two of them, so that node is a verb *and* a subtree — clap's
/// own `subcommand_required` is the question this asks, because it is exactly clap's answer to
/// "may this be run without naming a child": `calibrate` and `profile` require one and are
/// prefixes only, `photo` does not and is both. A walk that had kept the old "childless means
/// leaf" rule would have dropped `photo` from this population the day it grew a subcommand,
/// and `photo` is a verb with a `--json` contract, a parity bucket and a row in the agent
/// guide — the drop would have taken all three with it, silently.
fn collect_verbs(command: &clap::Command, prefix: &str, found: &mut Vec<String>) {
    for sub in command.get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue;
        }
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}-{name}")
        };
        if !sub.is_subcommand_required_set() {
            found.push(path.clone());
        }
        collect_verbs(sub, &path, found);
    }
}

/// The clap command one dash-joined verb names, walked from `root`.
///
/// Returns the parent chain with it, because a synopsis needs the words a caller types —
/// `calibrate sweep`, not `sweep` — and the caller of this has the leaf's own arguments and
/// nothing else.
#[must_use]
pub fn find_verb<'a>(root: &'a clap::Command, verb: &str) -> Option<&'a clap::Command> {
    let mut current = root;
    for step in verb.split('-') {
        current = current
            .get_subcommands()
            .find(|sub| sub.get_name() == step)?;
    }
    Some(current)
}

/// The shared command tree, for a consumer that documents rather than parses.
///
/// One line, and it exists so `webcam-handler-xtask` does not have to know that the tree is
/// [`crate::Cli`]'s and that [`Program`] renames it: the guide generator asks the surface
/// what it offers, and how that is assembled stays this crate's business (design §2.10).
#[must_use]
pub fn command_tree(program: Program) -> clap::Command {
    program.command()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_of_the_contract_table_is_a_row() {
        // `json_contracts` drops a line it cannot split, because an iterator has nowhere to
        // report to. That is safe only while the two counts agree: a row written with spaces
        // instead of a tab would otherwise vanish, and the verb it names would look exactly
        // like a verb nobody wrote a contract for.
        let candidates = contract_rows().count();
        let parsed = json_contracts().count();
        assert_eq!(
            candidates, parsed,
            "{} line(s) of json-contracts.tsv look like rows and {parsed} parsed; a row is \
             VERB<TAB>DOCUMENT",
            candidates
        );
        assert!(parsed > 1, "the contract table has {parsed} row(s)");

        // A document name is a `$defs` key, so it is a Rust type name and never a path or a
        // pointer. `#/$defs/CameraList` in this column would validate nothing and read as a
        // type nobody defined.
        for contract in json_contracts() {
            assert!(
                !contract.document.is_empty()
                    && contract.document.chars().all(|c| c.is_ascii_alphanumeric()),
                "{} names {:?}, which is not a bundle type name",
                contract.verb,
                contract.document
            );
        }
    }

    #[test]
    fn every_verb_the_command_surface_offers_has_a_json_row() {
        // Direction one: the tree is the authority on what exists. A verb added to
        // `Command` without a row here reaches a caller with no documented answer — the
        // agent guide would not name it, and `json-validates.sh` would never validate it.
        let verbs = verbs();
        assert!(verbs.len() > 10, "{verbs:?}");
        for verb in &verbs {
            assert!(
                json_contract(verb).is_some(),
                "the command surface offers `{verb}` and json-contracts.tsv gives it no \
                 document; add the row in the same commit as the verb"
            );
        }
    }

    #[test]
    fn no_row_of_the_contract_table_names_a_verb_the_surface_does_not_offer() {
        // Direction two, and the one a count alone would miss: a renamed verb leaves its old
        // row behind, and a table that documents a verb nobody can type teaches a caller to
        // invoke something that exits 2.
        let verbs = verbs();
        for contract in json_contracts() {
            assert!(
                verbs.iter().any(|verb| verb == contract.verb),
                "json-contracts.tsv names `{}` and the command surface offers no such verb \
                 ({verbs:?})",
                contract.verb
            );
        }
    }

    #[test]
    fn the_verb_walk_reaches_a_subtrees_leaves_and_never_its_root() {
        // `calibrate` and `profile` are subtrees; the verb an operator types is the leaf.
        // A walk that stopped at the root would document `calibrate` — a name that answers
        // with nothing, because clap refuses a subcommand with no subcommand of its own.
        let verbs = verbs();
        assert!(
            verbs.iter().any(|verb| verb == "calibrate-sweep"),
            "{verbs:?}"
        );
        assert!(
            verbs.iter().any(|verb| verb == "profile-capture"),
            "{verbs:?}"
        );
        assert!(!verbs.iter().any(|verb| verb == "calibrate"), "{verbs:?}");
        assert!(!verbs.iter().any(|verb| verb == "profile"), "{verbs:?}");
        // And clap's generated help subcommand is not one of this surface's verbs.
        assert!(!verbs.iter().any(|verb| verb == "help"), "{verbs:?}");

        // **A node can be both, and `photo` is** (D17): `photo <CAMERA>` takes a picture,
        // `photo diff <A> <B>` compares two, and each answers a document of its own. The two
        // assertions are one claim in both directions — the parent kept and the child reached
        // — because the walk that would drop the parent is the same one that adds the child,
        // so a test naming only the second would go green on exactly the regression this
        // arrangement risks.
        assert!(verbs.iter().any(|verb| verb == "photo"), "{verbs:?}");
        assert!(verbs.iter().any(|verb| verb == "photo-diff"), "{verbs:?}");
        // Once each, which is what makes the count this population reports mean something.
        assert_eq!(verbs.iter().filter(|verb| *verb == "photo").count(), 1);
        assert_eq!(
            verbs.len(),
            verbs
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "{verbs:?}"
        );
    }

    #[test]
    fn no_text_this_surface_prints_carries_a_rustdoc_link() {
        // **Found by writing the agent guide** (docs/7 P6e, note **N123**), and it was in
        // the shipped binary the whole time: `webcam-handler-cli photo --help` printed
        //
        //     standard output when omitted, which `--json` therefore does not allow — see
        //     [`Cli::try_parse_checked_from`], which enforces it
        //
        // to a *user*. Rustdoc resolves that bracket form into a link; clap does not, and
        // neither does a person or an agent reading `--help` — what reaches them is an
        // identifier that exists in this crate's source and nowhere on their machine.
        //
        // The class is narrow and exact on purpose: the ban is on the *link syntax*, not on
        // naming a Rust item. `record`'s help may say `record_status` in backticks, because
        // that is a wire method a client author calls. It may not say ``[`Executor`]``,
        // because that is an instruction to a documentation tool that is not running.
        //
        // Both strings per argument, because clap prints the first paragraph for `-h` and
        // the whole comment for `--help`; a rule that checked only one would leave the other
        // half of every doc comment free to carry links, which is where three of the four
        // original findings were.
        fn walk(command: &clap::Command, path: &str, found: &mut Vec<String>, seen: &mut usize) {
            let mut note = |what: &str, text: Option<&clap::builder::StyledStr>| {
                if let Some(text) = text {
                    *seen += 1;
                    let rendered = text.to_string();
                    // Two spellings of the same defect. `[`Foo`]` is rustdoc's link, which
                    // clap prints verbatim; `\[PF:3\]` is rustdoc's *escape*, which this
                    // repository writes because `-D warnings` reads a bare `[PF:3]` as a link
                    // to an item of that name — and which clap also prints verbatim, so four
                    // flags shipped backslashes to a terminal until `Program::command` learned
                    // to undo them. A ban on the first alone could not see the second, which
                    // is how the second lasted (note **N249**).
                    if rendered.contains("[`") || rendered.contains("\\[") {
                        found.push(format!("{path} {what}: {rendered}"));
                    }
                }
            };
            note("(about)", command.get_about());
            note("(long about)", command.get_long_about());
            for arg in command.get_arguments() {
                note(&format!("--{}", arg.get_id()), arg.get_help());
                note(&format!("--{} (long)", arg.get_id()), arg.get_long_help());
            }
            for sub in command.get_subcommands() {
                walk(sub, &format!("{path} {}", sub.get_name()), found, seen);
            }
        }

        let mut found = Vec::new();
        let mut seen = 0;
        walk(&Program::Cli.command(), "", &mut found, &mut seen);
        // Not vacuous: the surface is mostly help text, so a walk that inspected nothing
        // would mean the walker is broken rather than the prose clean.
        assert!(seen > 50, "the walk inspected {seen} string(s)");
        assert!(
            found.is_empty(),
            "{} help string(s) this surface prints carry a rustdoc link, which reaches the \
             reader as brackets around an identifier they cannot open:\n{}",
            found.len(),
            found.join("\n")
        );
    }

    #[test]
    fn a_refusal_names_the_guard_and_never_a_flag_that_does_not_exist() {
        // **The second thing writing the agent guide found** (note **N123**).
        // `Error::ControlInactive` rendered as *"disable white_balance_automatic first, or
        // use `--guarded`"* — design D3's spelling of a flag the shipped surface never had.
        // The verb guards by default and `--no-guard` opts out, so a caller doing what the
        // refusal said got clap's exit 2, from a registry whose entire purpose is telling an
        // unattended caller what to do next.
        //
        // This is the home for the check even though the message lives in
        // `webcam-handler-schema`: the schema crate cannot see a command line, and the
        // question is whether a *flag* exists. Both populations are walked — every D13 sample
        // rendered, every long option in the tree collected — so neither side is a hand list,
        // and a flag renamed in the surface turns this red just as a flag invented in a
        // message does.
        fn collect_flags(command: &clap::Command, into: &mut Vec<String>) {
            for arg in command.get_arguments() {
                if let Some(long) = arg.get_long() {
                    into.push(long.to_owned());
                }
            }
            for sub in command.get_subcommands() {
                collect_flags(sub, into);
            }
        }

        let mut flags = Vec::new();
        collect_flags(&Program::Cli.command(), &mut flags);
        assert!(flags.len() > 20, "{flags:?}");

        let mut inspected = 0;
        for &kind in schema::error::ErrorKind::ALL {
            let message = schema::error::Error::sample(kind).to_string();
            inspected += 1;
            let mut rest = message.as_str();
            while let Some(at) = rest.find("--") {
                rest = rest.split_at(at + 2).1;
                let named: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                if named.is_empty() {
                    continue;
                }
                assert!(
                    flags.iter().any(|flag| flag == &named),
                    "the {kind:?} refusal tells the reader to use `--{named}`, and no verb of \
                     this surface has that flag: {message}"
                );
            }
        }
        assert_eq!(inspected, schema::error::ErrorKind::ALL.len());
    }

    #[test]
    fn a_verb_resolves_to_the_command_that_parses_it() {
        let root = command_tree(Program::Cli);
        for verb in verbs() {
            let command = find_verb(&root, &verb)
                .unwrap_or_else(|| panic!("{verb} is offered and does not resolve"));
            // The leaf's own name is the last step, which is what a synopsis prints after
            // the parent chain.
            let last = verb.rsplit('-').next().expect("a verb has a last step");
            assert_eq!(command.get_name(), last);
        }
        assert!(find_verb(&root, "list-nothing").is_none());
    }
}
