# The P5 adversarial review — the web client, its transports, its credential and its gates

Doc 11 in the webcam-handler series, and the first that is a **record of a review rather than a
standard to review against**. It is filed under `docs/historical/` because it is a dated artifact:
it describes one tree, on one day, and it does not become wrong when the code changes — it becomes
*history*, which is what this directory is for.

**Run 2026-08-13/14, against `784a42d`**, closing docs/7's **P5e** requirement ("the adversarial
review; fixes; evidence entry; reconciliation") and rubric docs/8 **Part E**. `just ci` was green
at the start and the tree was clean.

**Why it exists as a document rather than as note entries.** docs/8 Part E asks for "the review's
own record as a dated evidence entry — candidate count, confirmed count, and what the review looked
for and did not find", and records that G4's went unwritten: its findings were distributed across
four notes and two amendments, "each locally well argued", and what fell between them was the
arithmetic N54 reads a sub-milestone's sizing from, and the absence list. **E14 says the fix in one
sentence: the next review that wants to settle it has to keep its candidate list.** This review kept
it, and this is where it is kept. The individual repairs still land as notes (N93–N95 and the
entries that follow); what lives *only* here is the census, the refutations, and the absence lists.

---

## 1. Method, and the one thing done differently from G4's review

Four lenses, run **concurrently** against a frozen tree, each read-only and forbidden from changing
a file. Concurrency is safe precisely because they only read — the repairs afterwards ran in
isolated git worktrees, because those write.

| lens | scope |
|---|---|
| 1 | the credential: `posture`, `token`, `gate`, `CAMERA_BEARING_PATHS`, the listener's credential half, `credential.js` |
| 2 | the transports: WS RPC, the MJPEG preview, `engine::preview`, `daemon::events`, shutdown |
| 3 | the shipped page: the ten ES modules, the embed, `web_client.rs`, the R1-web rung |
| 4 | the gates and tests: g5's rows, the five predicates, `rung-web.sh`, `counted-selections.sh`, `.config/nextest.toml` |

Each was given the same instructions from Part E: attempt the refutation *before* reporting a
Critical/High finding; make absence claims name where they looked; carry `file:line`, a category,
the red test the finding lacks, and a direction; confirm cited lines; and **keep the candidate list,
including the candidates that died.**

**Each was also told what is settled** — D1–D13, T1–T6, E1–E6, §7's rejected alternatives, §1's
non-goals, and by name the case law most likely to be re-litigated: N82 (assets served
unauthenticated, by the owner's 2026-08-12 ruling), N83, N74, N75, N78, N79, N59, N69/N70, N49.
Lens 3 and lens 4 were additionally told which residuals docs/9 already records, and asked to look
for a *third* rather than re-report the two.

---

## 2. The census

**82 candidates raised · 32 confirmed · 48 refuted · 2 folded or held as pointers.**

| lens | raised | confirmed | refuted |
|---|---|---|---|
| 1 — the credential | 26 | 6 | 20 |
| 2 — the transports | 12 | 5 | 6 (+1 folded) |
| 3 — the page | 28 | 14 | 14 |
| 4 — the gates | 16 | 7 | 8 (+1 pointer) |
| **total** | **82** | **32** | **48** |

By severity: **7 HIGH, 14 MEDIUM, 11 LOW.** No Critical.

**The false-positive rate is 48 of 82, or 59%** — and it is not comparable with N54's table, for the
reason N54 itself names: that table counts *raised candidates* against *reported findings* from a
harness that ran refutation as a separate pass, and E14 could produce no denominator at all. This is
the first review since E4/E6 that can. What it is comparable with is **E4 (31 raised, 15 confirmed,
16 refuted) and E6 (31 raised, 12 confirmed, 19 refuted)**, and against those the shape is the same
while the volume is roughly 2.6×, which is what four concurrent lenses over a phase buys.

**N54's prediction remains untested by this run**, and honesty requires saying so rather than
claiming a result: N54 predicts that *splitting* a large sub-milestone raises both the finding count
and the false-positive rate. This review split by **subject** rather than by sub-milestone, over a
whole phase rather than one piece of it, and no lens reviewed the same code as another. It is a
different experiment. What it does establish is that a review whose refutations are recorded can
report a rate at all.

---

## 3. What was confirmed

Dispositions are as of the close of the 2026-08-13/14 session. "Fixed" means a red test was watched
failing first and the repair landed with it.

### 3.1 The seven HIGH

| # | lens | what | disposition |
|---|---|---|---|
| H1 | 1 | **The header credential is written to the daemon's own log.** `rpc.rs`'s residual 4 declared the class closed because `without_the_query` strips `?token=` before jsonrpsee logs the request; jsonrpsee logs the request line **and the headers**, and the gate accepts `Authorization: Bearer` — the form the daemon's own 401 body recommends. Under systemd that is a persistent journal readable by `systemd-journal`/`adm`, for a run-long token, on the route that drives a camera | **Fixed** — note **N94**, commit `4f983b2` |
| H2 | 2 | **The web listener had no accept-time connection bound.** `rpc.rs` claimed one on the strength of jsonrpsee's `ConnectionGuard`, which is taken per in-flight *request*. `uds.rs` had already measured that ("with the cap at 32, 128 idle connections were all accepted and held") and grown a semaphore; the TCP listener never did. Unauthenticated, because the flood sends no byte and so never reaches the gate | **Fixed** — `4f983b2` |
| H3 | 2 | **The `try_send` hop drops the newest event and never consults `on_lag`.** The broadcast hop does consult it and drops the *oldest*; the compiler-asserted sizing relation (256 > 64) guarantees the private hop fails first. Together: `OnLag::EndTheStream` — the mechanism that makes the hotplug delta vocabulary honest — sits on the hop a slow client can never reach, and the hop it always reaches is silent. `SweepFinished`/`SweepInterrupted` carry no `index`/`total`, so the module's "a gap is self-healing and the next event repaints the bar" is false for exactly the event most likely to be lost | **Open — needs an owner ruling.** N57 records the drop policy and the sizing relation separately; they are jointly contradictory, and reconciling them is a doctrine change |
| H4 | 3 | **The page writes the right credential diagnosis and then destroys it**, and a browser claim pins the destroyed-by sentence verbatim while calling it "the honest one". For a page opened with no token the surviving sentence's first disjunct is false by construction | **Fixed** — note **N96**, commit `f95bee5` |
| H5 | 3 | **After the socket closes the page stays fully interactive and every call hangs for ever.** `WebSocket.send()` throws only for `CONNECTING`; on `CLOSED` it discards the frame. Clicking a camera re-enables the photo button and reopens the MJPEG preview, so the page shows live video under a "connection closed" banner with the previous camera's panel on screen | **Fixed** — N96. One half rests on the specification: Playwright's `routeWebSocket` mock *throws* where Chromium discards, so the rung cannot show the parked promise |
| H6 | 3 | **`streams.clear()` never calls each stream's `ended`**, so `#sweep-status` keeps asserting liveness about a subscription that no longer exists — the defect class `index.html` says that element exists to prevent | **Fixed** — N96 |
| H7 | 4 | **The rung's decline proof goes red, not skipped, on the host state the decline exists for.** On a fresh clone with node and no `node_modules` — the ordinary state — the test panics demanding a Chromium build when what is missing is Playwright, taking `just ci` and `just gate-g5` red with it. Reproduced by parking `node_modules` | **Fixed** — `f95bee5`. Six arms against a fabricated host, four declines and **two accepts** — the accepts closing the larger hole, a `preconditions` that had stopped returning `Ok` |

### 3.2 The MEDIUM and LOW findings

Recorded so the next session does not rediscover them. None is fixed unless marked.

**Lens 1 — the credential.**
- **M1.** The token-less cell is reachable by any website in the operator's browser, not merely by
  any local process, and `posture.rs`'s claim that "the token-less one is not silent in the shipped
  daemon" is false — the composition root prints what it serves, not what that means. *Superseded:*
  the owner ruled on 2026-08-13 that cross-origin requests are refused outright; note **N95**,
  commit `c935be2`. The startup-line half remains open.
- **M2.** `serve` cross-checks two of three caller disagreements; the unchecked one is the only one
  that opens a *token-less socket beyond loopback*. Unreachable through `open` today; the plausible
  next edits that break that are N79's reverse-proxy shape. **Open.**
- **L1.** `INSECURE_LOOPBACK_FLAG`'s doc names the composition root as its second reader; `main.rs`
  does not reference it, so two spellings exist with nothing comparing them. **Open**, one-character
  test fix.
- **L2.** A gate unit test asserts a refusal for a header value no HTTP parser can deliver, and its
  comment draws a false conclusion about the product. **Open.**
- **L3.** The page whose URL *is* the credential carries no `Cache-Control`. Retention, not access.
  **Open.**

**Lens 2 — the transports.**
- **M3.** A second preview tab is answered `200` before the feed's stream is known to exist — the
  window is a whole `engine::preview::start` round trip through the actor queue, which the module
  says is minutes when a sweep is in front of it. The existing two-tab test sequences the start out
  of the window it lives in. **Open.**
- **L4.** `Resuming`'s drop-guard resume is a no-op on its only motivating path: nothing catches the
  unwind, and the actor's `Box<dyn Camera>` drops microseconds later. The test that pins it uses a
  `ScriptedCamera` that outlives the panic. **Open** — the direction is to correct the header rather
  than the code.
- **L5.** An all-oversized-frame camera holds a preview open for ever: an oversized frame resets both
  silence budgets, so `PREVIEW_MAX_EMPTY_TURNS` never trips. Device truth; the fake cannot produce
  it. **Open.**

**Lens 3 — the page.**
- **M4.** The "works again on the next one" half of the reconnect claim is a **page reload**, so
  three of its ten assertions duplicate claims 1 and 6 and nothing about in-page reconnect can go
  red. **Open.**
- **M5.** Nothing anywhere asserts the panel renders **one card per control** — the rule
  `controls.js` states in capitals. `.control` appears once in the whole suite, inside a per-slug
  lookup, and `openClient`'s wait reads `#controls-status`, which is written from the DTO rather
  than the DOM. A page painting zero cards satisfies it. **Open.**
- **M6.** Eleven of `widgetFor`'s fifteen arms have never been rendered in a browser; the fixture
  carries four control types. Also unreached: a control with no current value, a min>max range, an
  empty menu, the `disabled` flag arm. **Open.**
- **M7.** A departed selected camera leaves the panel, the session list and the status lines stale
  and silent — `enumerate()` repaints the list and never reconciles `state.camera`. **Open.**
- **M8.** Two of the three precondition arms in the rung's decline selftest sit inside `if node`, so
  on a node-less host they silently do not run. **Folded into H7's repair.**
- **L6/L7/L8.** `"writing…"` is a status no operator can see; two places render `err.message` or raw
  JSON where the D13 discriminant helper is five lines away; a menu with no current value shows its
  lowest index as selected, and an emptied bitmask field silently writes zero. **In flight** (the
  last two), **open** (the discriminant pair).

**Lens 4 — the gates.**
- **M9.** A **third** shape residual in `token-comparison-has-one-home.sh`: `impl AsRef<str> for
  Token` plus `token.as_ref() == candidate` in one of the two credential paths leaves all seventeen
  selftest arms green while the timing leak is live on the form that rides the URL. **Repair in
  flight.**
- **M10.** `no-external-fetch-in-web.sh` has no rule for an off-origin URL assigned to an element
  property — which is how this client loads its only real subresource. Measured: `frame.src =
  "https://cdn…"` fires no rule, and `crates/web/src/lib.rs`'s reference test misses it too.
  **Repair in flight.**
- **M11.** `claims.json` is a consistency check, not a floor: a claim may carry `assertions: 0`, and
  deleting a claim from both the spec and the manifest is green everywhere. The 10/79 count is
  transcribed in three places with nothing reconciling them. **Repair in flight.**
- **M12.** `counted-selections.sh` counts per row, not per alternation branch; eleven g5 rows carry
  two to five named claims each and could lose all but one branch silently. **Repair in flight.**
- **M13.** The `.config/nextest.toml` line that makes the rung's decline visible in `just ci` has no
  gate; nothing can go red on its removal, and the effect is masked because `rung-web.sh` passes the
  flag itself. **Open.**
- **L9.** Four of `no-external-fetch-in-web.sh`'s ten rules have never been shown to go red, and
  docs/9 says they have. **Repair in flight.**

---

## 4. The absence lists — what was checked and found sound

This is the half E14 says gets lost between the notes, and the half that is worth the most to
whoever reviews this code next: it is the list of places somebody has already looked.

**The credential.** No request reaches `/rpc` or `/preview` without a verifying credential in any of
D11's three gated cells — established by enumerating the router's two routes and one fallback and
driving **twenty-two anonymous request shapes** over a real socket: method mismatch on both routes
(refuted as a bypass — `Router::route_layer` wraps the endpoint, not the per-method handlers, so
`POST /preview` is `401` and not an ungated `405`), seven path-normalisation forms, absolute-form
request targets, fragment smuggling in both directions, `;` as a legacy parameter separator, four
alternative credential headers, and a comma-list defeating `get_all`. `Token::verify` is reached
from exactly two call sites and nothing upstream branches on the secret. N74's every-credential-must-
verify rule holds at every multi-credential shape constructible. The client leaks the credential
nowhere: `location` is read in one file, both URL builders are same-origin, and there is no `fetch`,
absolute origin, external subresource, `innerHTML`, `eval` or dynamic `import()` in any of the ten
files. `Referrer-Policy: no-referrer` measured on the page, an asset, the 404 and the 401. No cookie
is read, written or accepted anywhere in the tree.

**The transports.** No unbounded queue and no blocking send between a device and a socket — every
channel walked. Three of the four bounds are read rather than transcribed on the browser's
connection; the fourth was H2. The shutdown ordering `rpc.rs` argues is what the composition
produces, and the step-3 wait is on a count **shared by both transports**, so a browser's
subscription is waited for. A client mid-frame at shutdown gets whole parts, because both `select!`s
are `biased` on the cancellation. `while_suspended`'s exits were enumerated one by one: **no path
leaves the camera in neither state, and none reaches the caller's work with the preview's stream
still up.** Two tabs on one camera are one streamer and the refusal past the bound is typed rather
than a hang. No frame reaches a log or an error on any path in scope.

**The page.** No off-origin fetch, CDN URL, dynamic import or `import.meta` URL construction in any
module. No `innerHTML`/`outerHTML`/`insertAdjacentHTML`/`eval`/`new Function`/`document.write` — so
a device string named `<script>` renders as text. No `localStorage`, `sessionStorage`, `indexedDB`
or `document.cookie`. **Zero `console.` calls in any module**, so no frame byte and no token can
reach a browser log. Two subscriptions, both page-lifetime, no leak. Only `rpc.js` builds JSON-RPC
frames. No `sleep` as synchronisation in the rung — every wait is auto-waiting or `expect.poll`. No
`test.only`/`.skip`, and `forbidOnly: true`. `web_client.rs` never asserts a DOM fact, as its own
header promises.

**The gates.** **All 57 test-name tokens** in g5's 31 `tests` rows were resolved against a real `fn`
in the package or binary the row restricts to — so the drift `counted-selections.sh` cannot see (M12)
has not happened yet. No `println!` declines a claim outside a census in P5's suites (the `smoke-hw`
shape found earlier the same day). There is no path where `rung-web.sh` reports RAN having asserted
nothing: a `.skip()` reaches the verifier as `status: "skipped"`, a rename or deletion fails set
equality, `forbidOnly` closes `test.only`, and a zero-test Playwright run exits non-zero — the one
residual is M11's. No second host state exists in which `gate-g5` passes having verified nothing
about the client.

---

## 5. DNS rebinding — the finding the review's own repair produced, and the design decided for it

**How it arose.** It is not one of the 82 candidates. It was found *while implementing* the owner's
cross-origin ruling (note **N95**): the agent building `daemon::http::provenance` reported that the
rule it had just written could not see a rebinding attack, and said so unprompted rather than
shipping a defence that looked complete. That is worth recording as a review outcome in its own
right — **the repair for one finding surfaced the next one**, which is the shape docs/8's G4
reconciliation already names ("the session that repairs a review's findings is itself a review").

**Why N95's rule cannot see it.** A rebinding attack produces a **same-origin** request. The victim
loads `http://evil.example/`, the attacker flips DNS to `127.0.0.1`, and the browser connects here
while still believing the page is `evil.example` — so it sends `Origin: http://evil.example`,
`Host: evil.example`, and `Sec-Fetch-Site: same-origin`. Nothing is tagged foreign, so N95's rule is
silent. Worse, N95 compares `Origin` against **the request's own authority**, which after a rebind
is `evil.example` on both sides: the comparison is self-referential and agrees.

### The decided design (owner's ruling, 2026-08-14)

An **allowlist of authorities' host components**, checked unconditionally:

1. **any IP literal** — v4 or v6, including the bracketed form `[::1]:8080`;
2. **`localhost`**, shipped in the list rather than requiring a flag;
3. **one `--http-allow-host` value**, if given.

Anything else is refused. **The port is deliberately not restricted**, so that an SSH tunnel
(`Host: localhost:9000` for a listener on `:8080`) works without configuration — the host component
carries the security property and the port carries none.

The owner's stated precondition for the flag is the one that makes it safe: *"`--http-allow-host`
means 'I control the DNS binding for this name on all the machines I use'."*

### Why an allowlist closes it, stated as the property rather than as a list

**A rebinding attack requires a name.** An IP literal has no DNS lookup to rebind, so an attacker
cannot make their page's authority be `127.0.0.1`. Refusing every name except ones the operator
controls therefore closes the class by construction rather than by enumerating attacks.

**And it composes with N95 as a pincer, neither rule subsuming the other:**

- rebind to a name → `Host: evil.example` → refused by *this* rule; N95 is silent because the
  browser calls it same-origin;
- skip DNS and fetch `http://127.0.0.1:8080/rpc` from a page at `http://1.2.3.4/` → `Host` is an
  allowlisted IP literal, so this rule admits it; **N95 refuses it**, because it is cross-site.

Each covers exactly what the other cannot.

**The load-bearing consequence: this check is unconditional, where N95's is not.** N95 keys on
`Sec-Fetch-Site`/`Origin` and must admit their absence, because `webcam-handler-client`, `curl` and
the unattended agent harness send neither. `Host` is mandatory in HTTP/1.1, so there is no absence
to be lenient about — **an absent `Host` is refused rather than admitted**, and the rule applies to
every caller rather than to browsers alone. A script that reaches the daemon by hostname breaks, and
that is the intended cost of the flag.

### Three residuals, recorded because they are what the rule does not buy

1. **`--http-allow-host` has one silent failure mode.** The precondition is true for a name whose
   DNS the operator hosts, and **false for an mDNS `.local` name on a network they do not control**
   — anyone on that LAN can answer for `rig.local`, and the same goes for a DHCP-provided search
   domain. If the value ever ends in `.local`, the precondition reads stronger than it is.
2. **It does not replace the token for LAN exposure.** Bound to `0.0.0.0`, an attacker on the LAN
   reaches the daemon by IP, and an IP-literal `Host` is allowlisted by design. That is D11's
   non-loopback cell being what it is; the token guards it.
3. **A local process is unaffected**, as with N95 — it sends whatever it likes.

### The parsing details that decide whether it works in practice

Each of these is a refusal an operator would experience as "the tool is broken", so they are part of
the design rather than of the implementation:

- **IPv6 literals are bracketed** and may carry a zone (`[fe80::1%eth0]`); the brackets must come
  off before the address parse.
- **Hostnames compare case-insensitively** — `LOCALHOST` must pass.
- **A trailing-dot FQDN** (`localhost.`) is a different string for the same name.
- **Allowlist entries are compared as whole authorities, case-insensitively, never by suffix.**
  Suffix matching is the classic failure: `evil-localhost.com` ends with `localhost`.

`daemon::http::posture` already owns this workspace's address-shape vocabulary (`Reach::of`,
including the IPv4-mapped case), so the parse belongs beside it rather than in a second reader.

**Status at the close of this session: designed and ruled on, not implemented.** It lands in
`daemon::http::provenance` beside N95's rule, needs a red test per direction (IP admitted, name
refused, allowlisted name admitted, bracketed IPv6 admitted, suffix near-miss refused, absent `Host`
refused), a `--help` line, and an amendment to N95's "what it does not close" list.

---

## 6. What this review cost, and what it says about the harness

**Four lenses, run concurrently, ~1.0M subagent tokens and 235 tool calls in total**, against a tree
of roughly 3,600 lines of P5 daemon code, 2,600 lines of client, and 41 gate criteria. Wall clock
was 12–20 minutes per lens; the concurrency was free because the lenses only read.

Three things are worth carrying forward to the next phase review:

1. **Telling each lens what is settled is what made the refutation counts trustworthy.** Twenty of
   lens 1's twenty-six candidates died, most of them against case law the lens was handed by name.
   A review not given that list spends its budget re-deriving decisions and reports them as findings.
2. **The best findings were where an argument and its code had come apart**, not where code was
   simply wrong. H1, H3 and H7 are each a module stating a property in its header that its code does
   not have; the mutation floor cannot express any of them, because none is a wrong expression.
3. **A type can be a blind spot.** H1's mitigation took a `Uri`, and a `Uri` cannot express a header
   — so no fixture in that module could state the failing case, however carefully written. That is
   the most transferable lesson here and it is recorded as **N94**.

---

## 6a. What the repairs themselves produced

Two findings that are *not* among the 82, because they were produced by the act of repairing rather
than by the act of reviewing. Both are recorded because docs/8's G4 reconciliation already names the
pattern — "the session that repairs a review's findings is itself a review" — and this session
produced three instances of it, counting the rebinding finding in §5.

- **N97 — the gates walk the filesystem, not the repository.** Giving the two repair agents isolated
  git worktrees put two complete checkouts under `.claude/`, which is gitignored and *inside* the
  tree, and **eight of twenty-five predicates went red at once**. Every violation was a correct file
  at a wrong path. The tree was clean throughout and no reader of the output could have told.
  `gate_find`'s prune list is a denylist over a walk; the class is that the population is defined by
  the filesystem rather than by `git ls-files`.
- **N98 — isolation separates edits, not meanings.** The two agents held **disjoint file lists** and
  both were green in their own worktree. One raised the rung's manifest to 15 claims / 117
  assertions; the other introduced the claims *floor* this review asked for (finding M11), measured
  against the 11/84 it could see. Merged, the floor sat four claims below its own manifest — the
  exact hole it existed to close. What caught it was the arm the second agent wrote to prove its
  floor could bite: one claim short of 15 is 14, which clears a floor of 11 and complains about
  nothing. AGENTS rule 2's red-on-inverse paid out in a way nobody planned.

## 7. What remains open at the close of the session

- **H3 needs an owner ruling** — the `events.rs` drop doctrine (§3.1).
- **DNS rebinding is designed and ruled on but not implemented** (§5).
- **M2, M3, M4, M5, M6, M7, M13** and the LOW findings are open, each with a direction recorded in
  §3.2.
- **G5's own close** — the evidence entry at both altitudes, docs/7's closure ledger, docs/9's
  unstruck P5b rows, and the rubric reconciliation — is P5e's remaining work.
