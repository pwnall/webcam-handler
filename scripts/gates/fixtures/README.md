# Gate fixtures

Inputs for the failing arms of `scripts/gates/selftest.sh`. Each directory is its own
Cargo workspace, deliberately: the root `Cargo.toml` lists its members explicitly, so
nothing here is built, linted, formatted or tested by `just ci`. They exist to be *read*
by `cargo metadata`, never compiled.

**Why path dependencies.** `just ci` runs offline, and so does its selftest. A failing
arm for the license and ban policy therefore cannot download a banned crate — it has to
be manufactured locally. `cargo-deny` reads licenses and crate names out of manifests, so
a path dependency on a crate whose manifest says `license = "GPL-3.0-only"`, or whose
`name` is one deny.toml bans, produces exactly the violation the real thing would, with
no registry involved.

**Why they cannot drift from the policy.** `license-allowlist.sh` always passes
`--config <repo>/deny.toml`. The fixtures supply the crate graph; the repository supplies
the policy. A ban deleted from `deny.toml` turns the failing arm green, and the selftest
reports it.

| Fixture | Violates | Proven by |
|---|---|---|
| `offlicense/` | the license allowlist (`GPL-3.0-only`) | `cases/license-allowlist.cases.sh` |
| `banned-crate/` | a named ban (a crate literally named `colored`) | `cases/license-allowlist.cases.sh` |

Each fixture commits its `Cargo.lock`. The predicate runs `cargo deny` with `--locked`,
so a gate run writes nothing into the tree.
