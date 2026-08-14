//! The T5 surface as data, so a document can be written from it (design D10).
//!
//! ## Why the trait is declared through a macro
//!
//! A Rust trait does not reify its methods. docs/9 says exactly that where it commissions
//! P4c's method-count walk — "a Rust trait does not reify its methods, so 'exhaustive
//! match' is the wrong mechanism and this row says the real one" — and it is why xtask
//! cannot walk the `WchRpc` declaration to emit the OpenRPC document. The obvious repair,
//! a table of method names in the emitter, is precisely the defect rubric rule 6 bans: two
//! lists that agree until somebody edits one, and the one that is not the wire loses
//! silently.
//!
//! So the trait and the inventory are **one declaration**. `wire_surface!` takes the methods
//! once and emits both halves: the `#[rpc(server, client)]` trait the daemon implements and
//! `webcam-handler-client` consumes, and `METHODS`, the walkable population xtask reads. A
//! method cannot reach one and miss the other, because there is nowhere for it to be written
//! twice. This is the shape `webcam-handler-schema`'s `closed_vocabulary!` already uses for an
//! enum and its `ALL` — "these macros generate the enum and its `ALL` from one source, so the
//! list cannot drift from the type" — applied to a trait and its inventory.
//!
//! What the macro cannot own is jsonrpsee's *spelling* of a wire name: the namespace
//! separator belongs to the proc macro, not to us. That one genuinely-two-derivations fact
//! is checked as two — `lib.rs`'s registration test reads the spellings off a real
//! `RpcModule` built by `into_rpc()` and compares them with [`crate::METHODS`].
//!
//! ## What the inventory carries, and what it does not
//!
//! Names, doc comments, parameter names and the Rust types of parameters and results —
//! everything an OpenRPC document needs and nothing it does not. It carries no server
//! behaviour, no routing and no per-method error subset: which D13 variants a given method
//! can actually produce is a fact about the daemon's routing (P4b, P4c), not about the
//! trait, and inventing one here would be a claim with no producer behind it.
//!
//! ## Subscriptions: one declaration, two traits, two inventories (P4e-i)
//!
//! D10 lists `subscribe_events` and `subscribe_calibration` among the T5 trait's methods.
//! They are declared in the *same* `wire_surface!` invocation as the nineteen calls — so
//! note N28's property is intact, and a subscription still cannot reach a trait and miss
//! an inventory — but the macro emits them as a **second** `#[rpc(server, client)]` trait,
//! `WchEvents`, with its own [`crate::SUBSCRIPTIONS`]. Three measured facts force
//! that, and each one would otherwise be discovered at a compile error:
//!
//! 1. **One `#[subscription]` anywhere re-bounds the whole generated client.**
//!    `jsonrpsee-proc-macros-0.26.0/src/render_client.rs` picks the client supertrait once
//!    per trait: `SubscriptionClientT` if the trait carries any subscription, `ClientT`
//!    otherwise. `SubscriptionClientT::subscribe` answers `jsonrpsee_core::client::
//!    Subscription`, whose only constructor is private over two private types, so **no
//!    type outside `jsonrpsee-core` can implement it** — and the daemon's four integration
//!    suites drive `WchRpcClient` over a `ClientT` that is *two transports*
//!    (`crates/daemon/tests/support/wire.rs`). Folding the subscriptions in would cost
//!    those suites one of their two pipes, which is the comparison they exist to make.
//! 2. **The transports really are two capabilities.** The daemon's socket carries HTTP/1.1
//!    (`daemon::uds`), and jsonrpsee answers a subscription on an HTTP `POST` with
//!    `-32603` — not a D13 code and not `MethodNotFound` — because its HTTP path builds
//!    `RpcServiceCfg::OnlyCalls`. A subscription needs the WebSocket upgrade on that same
//!    socket. Two client traits say what one would have hidden.
//! 3. **A subscription is not method-shaped.** It registers *two* wire names (jsonrpsee
//!    inserts the `unsubscribe` callback under its own key —
//!    `rpc_module.rs::verify_and_register_unsubscribe` — which is why `method_names()`
//!    grows by four), its "result" is an opaque subscription id, and the type a consumer
//!    actually decodes is the *notification item*, which no [`Method`] field has anywhere
//!    to put. [`Subscription`] is that shape.
//!
//! What D10 is protecting — **one source** — is untouched: both traits come out of one
//! macro invocation over one declaration, and the daemon merges the two registrations into
//! the one `Methods` value it serves. Note **N57** records the cost and the decision.
//!
//! ## Are subscriptions "methods"? Two consumers, two answers
//!
//! - **The count walks say yes.** Their population is a real `RpcModule`'s
//!   `method_names()`, which is `callbacks.keys()`, so all four spellings are in it.
//!   `crates/api`'s registration test compares that against `METHODS` ∪ every name
//!   `SUBSCRIPTIONS` carries, and `crates/daemon/tests/method_surface.rs` partitions the
//!   registered set by [`Subscription::names`] rather than excluding anything by hand — so
//!   a third subscription moves both sides at once.
//! - **The OpenRPC document says no**, and pretending otherwise would publish a false
//!   document: OpenRPC 1.3.2 has no notion of a server-initiated notification stream. A
//!   subscribe call emitted as a `method` would be silent about the payload, which is the
//!   only interesting part of it, and its sibling `unsubscribe` takes **positional**
//!   params (`params.one::<RpcSubscriptionId>()`) where the document declares
//!   `"paramStructure": "by-name"` for everything in `methods`. So `xtask` emits
//!   `methods` as exactly the call surface and puts the subscriptions in a top-level
//!   `x-subscriptions` array carrying both wire names, the notification name and the
//!   **item schema** — complete about the payload, and honest about being an extension.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};

/// One method of the T5 trait.
///
/// Every field is derived from the declaration in `lib.rs` — the wire name from the
/// namespace and the `#[method]` name, the docs from the doc comment rustdoc renders, the
/// parameters from the signature. Nothing here is a second statement of anything.
#[derive(Debug, Clone, Copy)]
pub struct Method {
    /// The wire name, namespace and all: `wch_list`.
    pub name: &'static str,
    /// The method's doc comment, one line per `///`, each still carrying rustdoc's leading
    /// space and its `\[PF:n\]` bracket escapes.
    ///
    /// Raw rather than pre-trimmed because the two documents that read it want different
    /// slices of it ([`Method::summary`] and [`Method::description`]), and a field that
    /// had already chosen would force one of them to re-do the work.
    pub docs: &'static str,
    /// The parameters, in the order the signature declares them.
    ///
    /// Order is preserved for the document's sake, not for a positional caller. The
    /// emitted OpenRPC document declares `"paramStructure": "by-name"`, which in OpenRPC
    /// 1.3.2 means a conforming client sends an object — so what the order buys is a
    /// `params` array a human reads in the order the author wrote the signature, and a
    /// diff that stays small when a parameter is added at the end.
    ///
    /// The server is in fact more permissive than the document, and that asymmetry is
    /// deliberate rather than accidental: jsonrpsee's generated server takes a positional
    /// array too, but not uniformly — measured on a real `RpcModule`, `wch_info` with
    /// `["cam:x"]` is served while `wch_calibrate_list` with `[]` is refused
    /// `-32602 "Invalid params" / "No more params"`, because an exhausted sequence is not
    /// an absent optional. A document that promised "either" would be promising a shape
    /// one of its own methods rejects, so it promises the one that always works.
    pub params: &'static [Param],
    /// What the method answers with on success.
    pub result: TypeRef,
}

impl Method {
    /// The doc comment's first paragraph: what this method does, in one sentence.
    ///
    /// House style puts that sentence first (`crates/schema`, `crates/engine` and
    /// `crates/cli-core` all do), so the summary a consumer reads is the sentence the
    /// author wrote rather than a second one nobody would keep current.
    ///
    /// The paragraph, not the first *line*: a sentence that wraps is still one sentence,
    /// and taking the line would publish half of it. Rustdoc's own short description is
    /// the same slice, which is why the two agree on every method.
    #[must_use]
    pub fn summary(&self) -> String {
        summary(self.docs)
    }

    /// The whole doc comment as markdown, with rustdoc's leading space removed.
    ///
    /// Including the `# Errors` section, which is the part a caller most needs: it names
    /// the D13 variants this operation refuses with, and the document's error registry
    /// says what each of those becomes on the wire.
    #[must_use]
    pub fn description(&self) -> String {
        description(self.docs)
    }
}

/// One subscription of the T5 surface.
///
/// **Not a [`Method`]**, and this module's header says why in full: a subscription
/// registers two wire names, its "result" is an opaque subscription id, and the type a
/// consumer decodes is the notification item — which no method-shaped field has anywhere
/// to put.
#[derive(Debug, Clone, Copy)]
pub struct Subscription {
    /// The wire name a client calls to open the stream: `wch_subscribe_events`.
    pub name: &'static str,
    /// The wire name that closes it: `wch_unsubscribe_events`.
    ///
    /// **jsonrpsee's spelling, derived twice on purpose.** The proc macro builds it by
    /// `strip_prefix("subscribe")` and panics at expansion time on a name that does not
    /// start with `subscribe` (`rpc_macro.rs::build_unsubscribe_method`), and
    /// `wire_surface!` builds the same string with `concat!`. The two derivations agree
    /// *by the same precondition*, which is exactly the situation `crates/api`'s
    /// registration test exists for — it reads both off a real `RpcModule`.
    pub unsubscribe: &'static str,
    /// The method name each notification arrives under.
    ///
    /// jsonrpsee defaults it to the subscribe name, and `wire_surface!` does not override
    /// it: a consumer that has just called `wch_subscribe_events` should not have to learn
    /// a third spelling to recognise what comes back.
    pub notification: &'static str,
    /// The subscription's doc comment, in [`Method::docs`]'s raw shape and for its reason.
    pub docs: &'static str,
    /// The type of one notification's payload — the thing a subscriber actually decodes.
    pub item: TypeRef,
}

impl Subscription {
    /// The doc comment's first paragraph. [`Method::summary`]'s rule, one implementation.
    #[must_use]
    pub fn summary(&self) -> String {
        summary(self.docs)
    }

    /// The whole doc comment. [`Method::description`]'s rule, one implementation.
    #[must_use]
    pub fn description(&self) -> String {
        description(self.docs)
    }

    /// Both wire names this subscription registers, subscribe first.
    ///
    /// The population every count walk partitions by, so "a subscription registers two
    /// names" is stated once rather than at each of the three places that has to know it
    /// (`crates/api`'s registration test, `daemon::server`'s routing pin and
    /// `crates/daemon/tests/method_surface.rs`).
    #[must_use]
    pub fn names(&self) -> [&'static str; 2] {
        [self.name, self.unsubscribe]
    }
}

/// A doc comment's first paragraph, as one sentence.
///
/// One implementation for [`Method`] and [`Subscription`], because "a summary is the first
/// paragraph" is one rule and a second copy is where two documents come to disagree about
/// what a summary is.
fn summary(docs: &str) -> String {
    let mut out = String::new();
    for line in docs.lines().map(str::trim) {
        if line.is_empty() {
            if out.is_empty() {
                continue;
            }
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// A whole doc comment as markdown, with rustdoc's leading space removed. See [`summary`].
fn description(docs: &str) -> String {
    let mut out = String::with_capacity(docs.len());
    for line in docs.lines() {
        out.push_str(line.strip_prefix(' ').unwrap_or(line));
        out.push('\n');
    }
    out.trim().to_owned()
}

/// One parameter of one method.
#[derive(Debug, Clone, Copy)]
pub struct Param {
    /// The name it travels under.
    ///
    /// Every method is `param_kind = map`, so this is a JSON object key on the way out of
    /// the generated client and the name the server matches when a request arrives as an
    /// object.
    pub name: &'static str,
    /// Its type.
    pub ty: TypeRef,
}

/// A type on the wire, as the two things a document generator needs of it.
///
/// Function pointers rather than a `dyn` object or a name string: the schema has to come
/// from the Rust type itself, through the same `schemars` derive the JSON Schema bundle
/// uses, so that the document a consumer validates against and the document the daemon
/// serializes cannot describe different shapes. A `&'static str` type name here would be a
/// third spelling of a type that already has two (the Rust name and `JsonSchema`'s).
#[derive(Clone, Copy)]
pub struct TypeRef {
    name: fn() -> Cow<'static, str>,
    schema: fn(&mut SchemaGenerator) -> Schema,
}

impl TypeRef {
    /// The wire type `T`.
    #[must_use]
    pub const fn of<T: JsonSchema>() -> TypeRef {
        TypeRef {
            name: T::schema_name,
            schema: subschema_for::<T>,
        }
    }

    /// What `schemars` calls this type — the key it lands under in a schema section.
    #[must_use]
    pub fn name(self) -> Cow<'static, str> {
        (self.name)()
    }

    /// Whether a by-name request may leave a parameter of this type out entirely.
    ///
    /// One home for "which parameters are optional", because the fact has two readers that
    /// must not disagree: the OpenRPC document's `required`, and the server. It is read off
    /// the type's own `schemars` output — an `Option<T>` is the shape that admits `null`,
    /// and that is exactly the shape serde resolves through `missing_field`, which visits
    /// `None` rather than failing. So the document's answer and the daemon's answer come
    /// from the same place, and `crates/api`'s registration test is where that is
    /// *measured* against a real `RpcModule` rather than believed.
    ///
    /// Only the top level, deliberately: a nullable field nested inside a parameter's type
    /// says nothing about whether the parameter itself may be absent.
    #[must_use]
    pub fn admits_absence(self) -> bool {
        let mut generator = SchemaGenerator::default();
        let schema = self.schema(&mut generator);
        let is_null = |value: &serde_json::Value| value == "null";
        // Both spellings schemars uses for an optional: an `anyOf` alternative for a
        // referenced type, and a type union for a primitive one. Neither is our choice, so
        // both are read rather than assumed.
        schema
            .get("anyOf")
            .and_then(|alternatives| alternatives.as_array())
            .is_some_and(|alternatives| {
                alternatives
                    .iter()
                    .filter_map(|alternative| alternative.get("type"))
                    .any(is_null)
            })
            || schema
                .get("type")
                .and_then(|kinds| kinds.as_array())
                .is_some_and(|kinds| kinds.iter().any(is_null))
    }

    /// Register the type with `generator` and answer the schema that refers to it.
    ///
    /// The two halves are one call because they are one fact: a `$ref` that names a
    /// definition the generator was never asked for is a dangling reference, and this is
    /// the only way to obtain the reference at all.
    #[must_use]
    pub fn schema(self, generator: &mut SchemaGenerator) -> Schema {
        (self.schema)(generator)
    }
}

impl fmt::Debug for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The name, not the two pointers: a reader debugging an inventory wants to know
        // which type this is, and two `TypeRef`s for one type are the same fact however
        // the addresses came out.
        write!(f, "TypeRef({})", self.name())
    }
}

/// [`SchemaGenerator::subschema_for`] as a plain function, so [`TypeRef`] can point at it.
///
/// A method on a generic type cannot be turned into a `fn` pointer without a shim; this is
/// the shim, and it exists once rather than as a closure per call site.
fn subschema_for<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<T>()
}

/// Declare the T5 surface — both its traits and both its inventories — in one breath.
///
/// The input is the surface as it would be written by hand, minus three things the macro
/// supplies because they are laws rather than choices: `#[rpc(server, client)]`,
/// `param_kind = map` on every method, and `-> SubscriptionResult` on every subscription.
/// Named parameters everywhere is D10's posture, not a per-method decision — `param_kind`
/// decides only what the generated *client* sends
/// (`jsonrpsee-proc-macros-0.26.0/src/render_server.rs` branches on `params.is_object()`
/// either way), and a document whose parameters have names is the whole reason to choose.
///
/// The output is `WchRpc` plus [`crate::METHODS`], and `WchEvents` plus
/// [`crate::SUBSCRIPTIONS`], in the module the macro is invoked from. Why two traits rather
/// than one is this module's header; that is the fact with the measurements behind it.
///
/// The two halves are two repetition groups and therefore two braced blocks in the
/// invocation, in that order: `macro_rules!` cannot interleave two item shapes inside one
/// `$(...)*`, which is a limitation of the tool and not a statement about the surface.
///
/// **Subscriptions take no parameters, and the grammar below is what enforces it.** A
/// `#[subscription]` with even one parameter makes the generated server call
/// `tokio::spawn` on its params-decoding error path (`render_server.rs`'s `error_ret`),
/// which would put a task spawn in the crate whose header says "Nothing here runs; it
/// declares" — note N5's review-held half. A parameterless subscription generates no such
/// call, and `schema::progress`'s own header already settles the design question the same
/// way: the subscription is per *client*, and the session id rides on every event.
macro_rules! wire_surface {
    (
        namespace = $namespace:literal;

        $(#[$trait_meta:meta])*
        $vis:vis trait $trait_name:ident {
            $(
                $(#[doc = $doc:literal])*
                #[method(name = $wire_name:literal)]
                async fn $rust_name:ident(
                    &self
                    $(, $param:ident : $param_ty:ty)* $(,)?
                ) -> Result<$result_ty:ty, WireError>;
            )*
        }

        $(#[$events_meta:meta])*
        $events_vis:vis trait $events_name:ident {
            $(
                $(#[doc = $sub_doc:literal])*
                #[subscription(name = $sub_name:literal, item = $item_ty:ty)]
                async fn $sub_rust_name:ident(&self);
            )*
        }
    ) => {
        $(#[$trait_meta])*
        #[::jsonrpsee::proc_macros::rpc(server, client, namespace = $namespace)]
        $vis trait $trait_name {
            $(
                $(#[doc = $doc])*
                #[method(name = $wire_name, param_kind = map)]
                async fn $rust_name(
                    &self,
                    $($param: $param_ty),*
                ) -> Result<$result_ty, $crate::codes::WireError>;
            )*
        }

        /// Every method the T5 trait carries, in declaration order.
        ///
        /// Generated from the same tokens as the trait above, so it cannot describe a
        /// method that does not exist or miss one that does — the reason
        /// [`crate::wire`] declares the surface through a macro at all. xtask walks this
        /// to write the OpenRPC document; the registration test in this file walks it
        /// against a real `RpcModule` to prove the wire *spellings* agree too.
        pub const METHODS: &[$crate::wire::Method] = &[
            $(
                $crate::wire::Method {
                    // jsonrpsee's default namespace separator is `_`
                    // (`rpc_macro.rs`: `self.namespace_separator.as_deref().unwrap_or("_")`).
                    // It is the one part of a wire name this macro does not control, which
                    // is what the registration test exists to check.
                    name: ::core::concat!($namespace, "_", $wire_name),
                    docs: ::core::concat!($($doc, "\n",)*),
                    params: &[
                        $(
                            $crate::wire::Param {
                                name: ::core::stringify!($param),
                                ty: $crate::wire::TypeRef::of::<$param_ty>(),
                            },
                        )*
                    ],
                    result: $crate::wire::TypeRef::of::<$result_ty>(),
                },
            )*
        ];

        $(#[$events_meta])*
        #[::jsonrpsee::proc_macros::rpc(server, client, namespace = $namespace)]
        $events_vis trait $events_name {
            $(
                $(#[doc = $sub_doc])*
                #[subscription(name = $sub_name, item = $item_ty)]
                async fn $sub_rust_name(&self) -> ::jsonrpsee::core::SubscriptionResult;
            )*
        }

        /// Every subscription the T5 surface carries, in declaration order.
        ///
        /// [`crate::METHODS`]'s sibling, from the same invocation and for the same reason.
        pub const SUBSCRIPTIONS: &[$crate::wire::Subscription] = &[
            $(
                $crate::wire::Subscription {
                    name: ::core::concat!($namespace, "_", $sub_name),
                    // `un` + the subscribe name is jsonrpsee's own default
                    // (`rpc_macro.rs::build_unsubscribe_method` strips `subscribe` and
                    // prefixes `unsubscribe`), which is only well-defined because every
                    // name here starts with `subscribe` — the same precondition its
                    // expansion panics on. Two derivations of one string, checked as two
                    // in this file's registration test.
                    unsubscribe: ::core::concat!($namespace, "_un", $sub_name),
                    // Defaulted by jsonrpsee to the subscribe name; not overridden.
                    notification: ::core::concat!($namespace, "_", $sub_name),
                    docs: ::core::concat!($($sub_doc, "\n",)*),
                    item: $crate::wire::TypeRef::of::<$item_ty>(),
                },
            )*
        ];
    };
}

pub(crate) use wire_surface;

#[cfg(test)]
mod tests {
    use schema::report::CameraList;

    use super::*;

    #[test]
    fn a_summary_is_the_first_paragraph_and_a_description_is_the_whole_comment() {
        // The shape `///` produces: a leading space per line, and blank lines as `""`.
        // The first sentence is deliberately wrapped, because that is the case a
        // first-*line* summary would publish half of — and most of the trait wraps.
        let method = Method {
            name: "wch_example",
            docs: " What it does, at some\n length (D1).\n\n More about it.\n",
            params: &[],
            result: TypeRef::of::<CameraList>(),
        };
        assert_eq!(method.summary(), "What it does, at some length (D1).");
        assert_eq!(
            method.description(),
            "What it does, at some\nlength (D1).\n\nMore about it."
        );
    }

    #[test]
    fn an_undocumented_method_summarises_to_nothing_rather_than_to_a_panic() {
        // Not a hypothetical: `docs` is `concat!()` of an empty repetition when a method
        // carries no doc comment. An empty summary is a visible hole in the emitted
        // document; a panic in the emitter would be a build failure nobody could read.
        let method = Method {
            name: "wch_example",
            docs: "",
            params: &[],
            result: TypeRef::of::<CameraList>(),
        };
        assert_eq!(method.summary(), "");
        assert_eq!(method.description(), "");
    }

    #[test]
    fn a_subscription_carries_two_wire_names_and_the_type_a_consumer_decodes() {
        // The three things a [`Method`] has nowhere to put, asserted on the shape that
        // does. `names()` is the population every count walk partitions by, so its order
        // and its arity are the fact rather than an implementation detail: a walk that got
        // the unsubscribe spelling from somewhere else would be a second hand list.
        let subscription = Subscription {
            name: "wch_subscribe_events",
            unsubscribe: "wch_unsubscribe_events",
            notification: "wch_subscribe_events",
            docs: " Nodes appearing and\n disappearing (D10).\n\n More about it.\n",
            item: TypeRef::of::<CameraList>(),
        };
        assert_eq!(
            subscription.summary(),
            "Nodes appearing and disappearing (D10)."
        );
        assert_eq!(
            subscription.description(),
            "Nodes appearing and\ndisappearing (D10).\n\nMore about it."
        );
        assert_eq!(
            subscription.names(),
            ["wch_subscribe_events", "wch_unsubscribe_events"]
        );
        assert_eq!(subscription.item.name(), "CameraList");
    }

    #[test]
    fn a_type_reference_names_the_type_and_registers_it_when_asked_for_its_schema() {
        let mut generator = SchemaGenerator::default();
        let reference = TypeRef::of::<CameraList>();
        assert_eq!(reference.name(), "CameraList");

        // The schema is a `$ref`, and asking for it is what puts `CameraList` in the
        // generator's definitions — a document that emitted the reference without the
        // registration would carry a pointer to nothing.
        let schema = reference.schema(&mut generator);
        let rendered = serde_json::to_value(&schema).expect("a schema serializes");
        assert_eq!(
            rendered.get("$ref").and_then(serde_json::Value::as_str),
            Some("#/$defs/CameraList"),
            "{rendered}"
        );
        assert!(generator.take_definitions(false).contains_key("CameraList"));

        // And the `Debug` a reader sees is the name, not two addresses.
        assert_eq!(format!("{reference:?}"), "TypeRef(CameraList)");
    }
}
