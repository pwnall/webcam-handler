// The control panel, generated from the `controls` DTO and from nothing else.
//
// The input is one `schema::report::ControlReport` — every control the device reported,
// plus the automation pairs in effect for it — and the output is a widget per control.
// There is no table of known controls in this file and no `if (slug === "brightness")`:
// design D2 says the device is the authority on itself, so a panel keyed on names this
// project happens to have seen would be a panel that renders a camera nobody has met yet as
// nothing at all.
//
// ## The four things a real control set does that a naive panel gets wrong
//
// All four are in this build's own fixture, because all four were measured on the seed
// hardware (design §1.2's registry):
//
// 1. **Menus are sparse \[PF:2\].** The Chicony's `Auto Exposure` has items at indices 1 and
//    3; index 2 does not exist. A `<select>` built by walking `0..n`, or by using an
//    option's *position* as its value, writes 2 to a device that has no such item — so the
//    options below carry the DTO's own keys, and the map is a map rather than a list all the
//    way from `VIDIOC_QUERYMENU` to this line. The hole is *shown*, not hidden: the index is
//    part of every option's label, so a reader can see that the menu skips one.
// 2. **A current value may sit outside the declared range \[PF:4\].** The OBSBOT reports
//    `Zoom, Continuous` as `245` in a range of `-100..=100`. A slider cannot represent that
//    — the element clamps, silently — so the number beside it is the authority, the slider
//    is parked at the closest position it *can* take, and a note says both numbers. Hiding
//    it would be this page correcting a device, which D2 forbids in as many words.
// 3. **A control may be INACTIVE because an automation partner owns it \[PF:3\].** It is not
//    disabled: D3's guarded write switches the automation off first, which is exactly what
//    the checkbox above the panel selects, so the widget stays usable and the note names
//    the owner and how it goes off. Rendering it as "unsupported" would be the
//    availability-as-capability collapse AGENTS rule 7 refuses.
// 4. **A control type may be one this build cannot name.** `ControlType::Unknown { raw }` is
//    a type the kernel emitted and `schema` carried through rather than rejecting \[PF:1\];
//    it renders as a row naming the raw discriminant and whatever payload came with it.
//    **Nothing vanishes from this panel** — AGENTS rule 6 — because a control that is
//    missing from a UI is indistinguishable from a control the camera does not have.
//
// ## Requested is not applied, and the panel must show the difference
//
// Every write answers a `WriteReport` whose `Applied` carries `{requested, applied}` and the
// warnings that explain a gap (D3, E4, \[PF:6\]: drivers clamp and report success). So a
// write here is followed by a re-read of the whole report, and **a slider that snaps back is
// the correct rendering of a clamp** rather than a glitch to smooth over. The write's own
// outcome is kept beside the control across that repaint — see [`outcomes`] — because the
// repainted widget shows what the device now holds and the outcome shows what was asked
// for, and only the two together are the fact E4 is about.
//
// ## What is asserted about this file, and what is not
//
// Nothing here is verified by a Rust test, deliberately. docs/7 P5c: "a browser behavior
// verified only through the JSON the page consumes is not verified" — so the suite that
// landed with this client asserts that the *DTO* carries all four edges above over the real
// transports, and that a sparse menu really does arrive with the holes in it, and stops
// there. That a `<select>` ends up carrying those indices is P5d's, in a real Chromium.

import { el, fill, valueLabel } from "./dom.js";

/**
 * The last write attempted per control slug, kept across repaints.
 *
 * Module state rather than a parameter, and it is the smallest thing that makes the E4
 * doctrine visible: a write is followed by a fresh `wch_controls`, which rebuilds every
 * widget from what the device now holds — so without this, the requested value would be
 * gone by the time the applied one arrived, and the clamp would look like a slider that
 * moved by itself.
 */
const outcomes = new Map();

/**
 * Paint `report` into `panel`.
 *
 * `write(control, value)` sends one `wch_set` and answers its `WriteReport`; `refresh()`
 * re-reads the report and calls back in here. Both are supplied by app.js, so this module
 * builds DOM and decides nothing about transports.
 */
export function paint(panel, report, { write, refresh }) {
  // **Every list on this wire may be absent rather than empty.** `ControlReport::pairs`,
  // `Applied::warnings`, `WriteReport::disabled_automation`, `ControlDesc::menu`,
  // `CameraList::hints` and `ControlSession::samples` all carry
  // `#[serde(skip_serializing_if = …)]`, so the healthy case is a key that is not there. Two
  // documents on the same surface say the opposite — `SessionStatus::log` and
  // `Session::controls` are always emitted, and `log`'s own doc argues that a consumer
  // "should meet a list with nothing in it rather than a missing key" — so this client
  // defaults every list at the point it reads one rather than trusting either rule.
  const pairs = new Map((report.pairs ?? []).map((pair) => [pair.manual, pair]));
  fill(
    panel,
    report.controls.map((desc) => control(desc, pairs.get(desc.slug), { write, refresh })),
  );
  if (report.controls.length === 0) {
    // An empty control set is a fact about the device rather than an empty panel: D1's
    // "an empty enumeration is diagnosed, not shrugged at", one level down.
    panel.append(el("p", { class: "note" }, "this camera reported no controls at all"));
  }
}

/** Forget every recorded write outcome — used when the panel moves to another camera. */
export function forgetOutcomes() {
  outcomes.clear();
}

/** One control: its identity, its flags, its widget, and whatever is odd about it. */
function control(desc, pair, io) {
  const notes = [];
  const node = el("div", { class: "control" }, [
    el("div", { class: "name" }, [
      desc.name,
      " ",
      el("span", { class: "slug mono" }, `${desc.slug} · ${controlId(desc.id)} · ${typeName(desc)}`),
    ]),
    badges(desc),
  ]);

  const widget = el("div", { class: "widget" }, widgetFor(desc, io, notes));
  node.append(widget);

  const refuses = readOnlyReason(desc);
  if (refuses !== null) {
    for (const input of widget.querySelectorAll("input, select, button")) {
      input.disabled = true;
    }
    notes.push(note(refuses, "surprising"));
  }
  if (desc.flags.known.includes("inactive")) {
    notes.push(note(inactiveSentence(desc, pair), "owned"));
  }
  if (desc.current === undefined || desc.current === null) {
    // `ControlDesc::current` is an `Option` and its absence is a fact rather than a zero: a
    // `WRITE_ONLY` control's value is the device's to keep, and an enumeration that did not
    // fetch values has nothing to show. A widget drawn from a default would be this page
    // asserting a device state nobody told it.
    notes.push(note("the device reported no current value for this control", "surprising"));
  }
  if (outsideRange(desc.range, currentInt(desc))) {
    notes.push(
      note(
        `the device reports ${currentInt(desc)}, outside the range it declares ` +
          `(${desc.range.min}…${desc.range.max}) — carried as the device reported it [PF:4]`,
        "surprising",
      ),
    );
  }
  if (outsideRange(desc.range, desc.default)) {
    notes.push(
      note(
        `its declared default ${desc.default} is outside its own declared range [PF:5]`,
        "surprising",
      ),
    );
  }
  const outcome = outcomes.get(desc.slug);
  if (outcome !== undefined) {
    notes.push(note(outcome.text, outcome.tone));
  }
  node.append(...notes);
  return node;
}

/** The widget for a control's type — one arm per `schema::control::ControlType`. */
function widgetFor(desc, io, notes) {
  switch (desc.type.kind) {
    case "integer":
    case "integer64":
      return scalar(desc, io, notes);
    case "boolean":
      return toggle(desc, io);
    case "menu":
    case "integer_menu":
      return menu(desc, io);
    case "bitmask":
      return bitmask(desc, io);
    case "string":
      return text(desc, io);
    case "button":
      return trigger(desc, io);
    case "control_class":
      return el("span", { class: "note" }, "a class header rather than a control");
    case "u8":
    case "u16":
    case "u32":
    case "area":
    case "rect":
      return payload(desc, "a compound value this panel shows and does not write");
    case "unknown":
      // The arm AGENTS rule 6 is about, and the one that must never be a `return null`.
      // The discriminant is the kernel's, preserved exactly by `ControlType::Unknown`, so
      // an operator can look it up even though this build cannot name it.
      return payload(desc, `a control type this build does not name (raw ${controlId(desc.type.raw)}) [PF:1]`);
    default:
      // Not reachable from a daemon of this version, and here anyway: `ControlType` is a
      // Rust enum that grows, and a page that met a new arm with a blank row would drop a
      // control silently. A `default` that renders something is the JavaScript spelling of
      // the payload-carrying fallback arm AGENTS rule 6 asks a `match` for.
      return payload(desc, `a control kind this page does not know: ${desc.type.kind}`);
  }
}

/**
 * An integer control: a slider where one is meaningful, and a number that is always the
 * authority.
 *
 * The number field carries **no `min`/`max` attributes**, and that is the doctrine rather
 * than laziness: the range is what the device *declares*, and what it *accepts* is a
 * different question that only a write can answer \[PF:6\]. A field that refused 245 here
 * would be this page enforcing a bound the driver does not, and the answer to a write past
 * the maximum is a clamp the panel can show — which is more informative than a refusal it
 * invented.
 *
 * Writes fire on `change` and never on `input`. A control that moves motors is one drag
 * away from a sweep nobody planned, and design §5 puts motion behind an explicit decision;
 * `change` is one write per gesture.
 */
function scalar(desc, io, notes) {
  const current = currentInt(desc);
  const number = el("input", {
    type: "number",
    class: "mono",
    value: current ?? "",
    step: usableStep(desc.range),
    onchange: () => apply(desc, { kind: "int", value: Number(number.value) }, io),
  });
  if (!orderedRange(desc.range)) {
    notes.push(
      note(
        `the device declares a minimum (${desc.range.min}) above its maximum ` +
          `(${desc.range.max}), so there is no slider to draw`,
        "surprising",
      ),
    );
    return [number];
  }
  const slider = el("input", {
    type: "range",
    min: desc.range.min,
    max: desc.range.max,
    step: usableStep(desc.range),
    // Clamped, because the element cannot hold anything else — the note beside it is what
    // keeps that from being a quiet correction (this module's header, point 2).
    value: clamp(desc.range, current ?? desc.default),
    oninput: () => {
      number.value = slider.value;
    },
    onchange: () => apply(desc, { kind: "int", value: Number(slider.value) }, io),
  });
  return [slider, number];
}

/**
 * A boolean: the two values V4L2 gives it, as a checkbox.
 *
 * `=== null` is checked explicitly rather than folded into a truthiness test, because a
 * control whose value was **not read** — `ControlDesc::current` is an `Option` and a
 * write-only control has none — must not render as "on". A box drawn from a value nobody
 * has is a UI asserting a device state it was never told; the note in [`control`] says so
 * beside it.
 */
function toggle(desc, io) {
  const current = currentInt(desc);
  const box = el("input", {
    type: "checkbox",
    checked: current !== null && current !== 0,
    onchange: () => apply(desc, { kind: "int", value: box.checked ? 1 : 0 }, io),
  });
  return [el("label", {}, [box, " on"])];
}

/**
 * A menu: one option per item the device declared, carrying **its** index.
 *
 * `Object.entries` over the DTO's map, sorted numerically, because the keys crossed JSON as
 * strings and `"10" < "3"` lexically. The index is in every label so the holes are visible
 * \[PF:2\]; the value written is `Number(key)` and never the option's position.
 *
 * A current value with no item is its own option, marked. That is not a defensive branch: a
 * menu control whose default sits outside its declared range is in this build's fixture
 * \[PF:5\], and a device holding an index it did not enumerate is the same class of fact. A
 * `<select>` with no matching option silently shows its first item, which would make this
 * page report a value the camera does not hold.
 */
function menu(desc, io) {
  // `?? {}` because a menu with no items at all is a *missing key* on this wire (see
  // [`paint`]), and a menu control with nothing in it is a thing a device can report.
  const items = Object.entries(desc.menu ?? {})
    .map(([index, item]) => [Number(index), item])
    .sort(([left], [right]) => left - right);
  const current = currentInt(desc);
  const options = items.map(([index, item]) =>
    el("option", { value: String(index), selected: index === current }, `${index} · ${itemLabel(item)}`),
  );
  if (current !== null && !items.some(([index]) => index === current)) {
    options.unshift(
      el(
        "option",
        { value: String(current), selected: true },
        `${current} · the device holds an index this menu does not declare`,
      ),
    );
  }
  const select = el(
    "select",
    { onchange: () => apply(desc, { kind: "int", value: Number(select.value) }, io) },
    options,
  );
  return [select];
}

/** A bitmask: the raw word, in the base the kernel documents it in. */
function bitmask(desc, io) {
  const current = currentInt(desc) ?? 0;
  const field = el("input", {
    type: "text",
    class: "mono",
    value: `0x${(current >>> 0).toString(16)}`,
    onchange: () => {
      const parsed = Number(field.value);
      if (Number.isFinite(parsed)) {
        apply(desc, { kind: "int", value: parsed }, io);
      }
    },
  });
  // No per-bit checkboxes: V4L2 gives a bitmask no names for its bits, so a row of boxes
  // labelled "bit 3" would be a UI that knows less than the hex it replaced.
  return [field, el("span", { class: "note" }, "hexadecimal or decimal; the device names no bits")];
}

/** A string control. */
function text(desc, io) {
  const field = el("input", {
    type: "text",
    value: desc.current?.kind === "text" ? desc.current.value : "",
    onchange: () => apply(desc, { kind: "text", value: field.value }, io),
  });
  return [field];
}

/**
 * A button: a trigger with no value.
 *
 * Writing it *is* the effect, so the answer carries `Unverified { TypeHasNoValue }` rather
 * than a read-back — `WriteWarning::classify` says so — and the note this page shows after
 * the write is that warning rather than a green tick. A panel that reported "applied" here
 * would be claiming a verification that did not happen (D3).
 */
function trigger(desc, io) {
  return [
    el("button", { type: "button", onclick: () => apply(desc, { kind: "int", value: 1 }, io) }, "Trigger"),
  ];
}

/** A value this panel shows and does not write, with the reason it does not. */
function payload(desc, why) {
  return [
    el("span", { class: "mono" }, valueLabel(desc.current)),
    el("span", { class: "note" }, `${why} · ${desc.elems} × ${desc.elem_size} bytes`),
  ];
}

/**
 * Send one write and record what the device did with it.
 *
 * The whole `WriteReport` is read rather than just this control's row, because a *guarded*
 * write is a plan: D3 switches the automation partner off first, so the answer may carry
 * writes to controls the operator never touched, and `disabled_automation` names them. A
 * panel that showed only the row it asked for would leave an operator wondering why
 * `auto_exposure` had turned itself off.
 */
async function apply(desc, value, io) {
  outcomes.set(desc.slug, { text: "writing…", tone: null });
  try {
    const report = await io.write(desc.slug, value);
    outcomes.set(desc.slug, describeWrite(desc, report));
  } catch (err) {
    // The refusal keeps its D13 name: `control_inactive` and `control_read_only` are
    // different from `busy`, which is different again from `device_gone` (AGENTS rule 7).
    outcomes.set(desc.slug, {
      text: err.kind === null || err.kind === undefined ? err.message : `${err.kind}: ${err.message}`,
      tone: "surprising",
    });
  }
  // Repaint from the device rather than from the answer: another control's INACTIVE flag
  // may have moved with this write \[PF:3\], and `V4L2_CTRL_FLAG_UPDATE` says a write can
  // change other controls' *properties* outright. Re-reading is how this panel finds out.
  await io.refresh();
}

/** What one `WriteReport` says about the control that was written. */
function describeWrite(desc, report) {
  const mine = report.writes.find((applied) => applied.slug === desc.slug);
  if (mine === undefined) {
    return { text: "the daemon answered without a row for this control", tone: "surprising" };
  }
  const requested = valueLabel(mine.requested);
  const applied = valueLabel(mine.applied);
  const parts = [
    requested === applied
      ? `wrote ${applied}`
      : `asked for ${requested}, the device holds ${applied}`,
  ];
  for (const warning of mine.warnings ?? []) {
    parts.push(warningSentence(warning));
  }
  if ((report.disabled_automation ?? []).length > 0) {
    parts.push(`automation switched off first: ${report.disabled_automation.join(", ")}`);
  }
  return { text: parts.join(" · "), tone: requested === applied ? null : "surprising" };
}

/** One `schema::control::WriteWarning`, as a sentence. */
function warningSentence(warning) {
  switch (warning.kind) {
    case "clamped":
      return `clamped into ${warning.range.min}…${warning.range.max} [PF:6]`;
    case "step_aligned":
      return `aligned down to a step of ${warning.step}`;
    case "adjusted":
      return "the device took something else and the range does not explain it";
    case "unverified":
      return `accepted, and not read back (${warning.because})`;
    default:
      return `${warning.kind}`;
  }
}

/** Every flag the device set: the ones this build names, and the ones it does not. */
function badges(desc) {
  const chips = desc.flags.known.map((flag) =>
    el("span", { class: `badge${flag === "inactive" ? " owned" : ""}` }, flag),
  );
  if (desc.flags.unknown_bits !== 0) {
    // Next year's flag is data, not an error (design D2). It is shown as the raw bits
    // because that is exactly what is known about it.
    chips.push(
      el("span", { class: "badge unknown" }, `unknown bits ${controlId(desc.flags.unknown_bits)}`),
    );
  }
  return el("div", { class: "badges" }, chips);
}

/** Why this control cannot be written at all, or `null` when it can. */
function readOnlyReason(desc) {
  if (desc.flags.known.includes("disabled")) {
    return "the device reports this control permanently unsupported";
  }
  if (desc.flags.known.includes("read_only")) {
    // The Chicony's `Privacy` is the seed case \[PF:12\], and AGENTS is explicit that the
    // hardware privacy control is honored and never worked around.
    return "the device reports this control read-only";
  }
  if (desc.type.kind === "control_class") {
    return "a class header, which is not a value";
  }
  return null;
}

/** What owns an INACTIVE control right now, and how it lets go. */
function inactiveSentence(desc, pair) {
  if (pair === undefined) {
    return "an automation control owns this right now and this camera's pair table does not say which [PF:3]";
  }
  const how =
    pair.off.kind === "value"
      ? `by writing ${pair.off.value}`
      : `by selecting the menu item named like ${pair.off.patterns.join(" / ")}`;
  return `${pair.automation} owns this right now; a guarded write switches it off ${how} (D3, ${pair.provenance})`;
}

/** One note line. */
function note(text, tone) {
  return el("p", { class: tone === null ? "note" : `note ${tone}` }, text);
}

/**
 * The control's type, as a short label beside its slug.
 *
 * An unnameable type shows its raw discriminant rather than the word "unknown" alone: the
 * number is what an operator can look up in the kernel's headers, and it is the only thing
 * this build knows about that control's type at all \[PF:1\].
 */
function typeName(desc) {
  return desc.type.kind === "unknown" ? `unknown ${controlId(desc.type.raw)}` : desc.type.kind;
}

/** A menu item's label — a name for a `MENU`, the integer for an `INTEGER_MENU`. */
function itemLabel(item) {
  return item.kind === "name" ? item.name : String(item.value);
}

/** The control's current value when it is a scalar, else `null`. */
function currentInt(desc) {
  return desc.current?.kind === "int" ? desc.current.value : null;
}

/** Whether `value` is a number the declared range does not contain. */
function outsideRange(range, value) {
  return value !== null && orderedRange(range) && (value < range.min || value > range.max);
}

/** Whether the declared range has an inside at all. */
function orderedRange(range) {
  return range.min <= range.max;
}

/** `ControlRange::effective_step`, in JavaScript: a step of zero or less is a step of one. */
function usableStep(range) {
  return range.step > 0 ? range.step : 1;
}

/** `ControlRange::clamp_value`, for the one element that cannot hold anything else. */
function clamp(range, value) {
  return Math.min(Math.max(value, range.min), range.max);
}

/** A kernel number, in the base the kernel writes it in. */
function controlId(raw) {
  return `0x${(raw >>> 0).toString(16).padStart(8, "0")}`;
}
