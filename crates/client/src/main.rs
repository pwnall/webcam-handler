//! `webcam-handler-client`'s composition root: the process's edges and nothing else.
//!
//! Everything this binary does is in `webcam-handler-client`'s library half, for the reason
//! that crate's header gives — the sweep's ordering is only assertable from inside the
//! process. What is left here is what a library must not do: read the process environment,
//! read the process arguments, write to the process's streams, and choose an exit code.
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

use std::process::ExitCode;

use cli_core::{Cli, Output};
use client::PROGRAM;

fn main() -> ExitCode {
    let cli = Cli::parse_checked(PROGRAM);
    let mut out = Output::process();

    match client::run(&cli, &schema::paths::SystemEnv, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The typed error, reported once, through the *same function*
            // `webcam-handler-cli`'s root calls (`cli_core::report_failure`, note **N127**) —
            // including the errors that arrived over the wire, which `api::codes::typed` turned
            // back into `schema::Error` values rather than into transport prose. So the
            // `--json` failure document, the line on standard error and the exit code are the
            // same three answers from both roots because they are produced by one piece of
            // code, and the parity gate compares them byte for byte rather than trusting that.
            ExitCode::from(cli_core::report_failure(
                PROGRAM, &error, cli.json, &mut out,
            ))
        }
    }
}
