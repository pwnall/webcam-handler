// The four element helpers every other module in this client builds its DOM with.
//
// ## Why there is a helper at all, and why it takes text rather than markup
//
// Every string this page renders came off a device: a control's `name` is the kernel's
// bytes for it, a camera's `card` is what the USB descriptor said, a D13 refusal's message
// names a device node. None of it is this project's text, none of it is escaped anywhere
// upstream — schema carries device vocabulary verbatim, which is design D2's whole point —
// and `innerHTML` on any of it is a control panel for a camera executing whatever the
// camera's descriptor happened to contain.
//
// So there is no `innerHTML` in this client, in any module, and this file is why that is
// cheap: `el()` takes children as *text nodes and elements*, never as markup, so the
// convenient spelling and the safe spelling are the same one. A device that named itself
// `<script>` renders a control called `<script>`, which is also the honest rendering
// (AGENTS rule 6: represent the unknown; never "correct" it).
//
// ## What it deliberately is not
//
// Not a framework and not a renderer. There is no diffing, no reactivity and no template
// syntax: a section that changes is rebuilt by the module that owns it, because the
// documents this page renders arrive whole (`wch_controls` answers a whole
// `ControlReport`) and rebuilding a section from a whole document is fewer moving parts
// than reconciling one. If that ever stops being true, design §2.7 already says what
// happens: a library gets vendored under this crate with its license file, and §2.8's
// inventory learns it.

/**
 * One element: a tag, its attributes, and its children.
 *
 * `attrs` is applied with `setAttribute` for anything a browser reads out of markup and by
 * assignment for the handful of properties that are not attributes at all (`checked`,
 * `value`, `disabled` on a live element, and every `on*` listener). Children are strings —
 * which become text nodes — or elements. There is no third case on purpose: a helper that
 * accepted markup would be the one place this client could grow an injection.
 */
export function el(tag, attrs = {}, children = []) {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) {
    if (value === undefined || value === null || value === false) {
      continue;
    }
    if (name.startsWith("on") && typeof value === "function") {
      node.addEventListener(name.slice(2), value);
    } else if (name in node && typeof value !== "string") {
      node[name] = value;
    } else {
      node.setAttribute(name, String(value));
    }
  }
  for (const child of [children].flat()) {
    if (child === undefined || child === null || child === false) {
      continue;
    }
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

/** Replace everything inside `node` with `children`. */
export function fill(node, children) {
  node.replaceChildren();
  for (const child of [children].flat()) {
    if (child === undefined || child === null || child === false) {
      continue;
    }
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

/**
 * The element with this id, or a failure that names it.
 *
 * A missing id means index.html and a module disagree about the page's shape, which is a
 * defect in this directory rather than anything a user did — so it throws here, at the
 * line that knows the name, instead of becoming a `null` that surfaces three frames later
 * as a property access on nothing.
 */
export function byId(id) {
  const node = document.getElementById(id);
  if (node === null) {
    throw new Error(`the page has no element with id ${id}`);
  }
  return node;
}

/**
 * A short label for a `schema::control::ControlValue`, whatever kind it is.
 *
 * The three kinds are the three shapes D2 allows, and the payload one is why this exists:
 * a compound or unrecognized control's value is opaque bytes, and rendering it as
 * `[object Object]` — or as a decoded guess — would be this page inventing a meaning the
 * device never gave it. What it renders instead is the size and the first bytes, which is
 * what `ControlValue`'s own `Display` does one language over.
 */
export function valueLabel(value) {
  if (value === undefined || value === null) {
    return "—";
  }
  switch (value.kind) {
    case "int":
      return String(value.value);
    case "text":
      return value.value;
    case "bytes":
      return `${value.value.length} bytes · ${hex(value.value, 8)}`;
    default:
      // A fourth kind would be a schema this page has not been taught; it is named rather
      // than dropped, for the same reason an unknown control type is.
      return `${value.kind}: ${JSON.stringify(value.value)}`;
  }
}

/** The first `limit` bytes of an array, as hex, with an ellipsis when there are more. */
export function hex(bytes, limit) {
  const shown = bytes
    .slice(0, limit)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join(" ");
  return bytes.length > limit ? `${shown} …` : shown;
}
