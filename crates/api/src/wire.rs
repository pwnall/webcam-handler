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
//! So the trait and the inventory are **one declaration**. `wire_surface!` takes the
//! methods once and emits both halves: the `#[rpc(server, client)]` trait the daemon
//! implements and `wchc` consumes, and `METHODS`, the walkable population xtask reads. A
//! method cannot reach one and miss the other, because there is nowhere for it to be
//! written twice. This is the shape `webcam-handler-schema`'s `closed_vocabulary!` already
//! uses for an enum and its `ALL` — "these macros generate the enum and its `ALL` from one
//! source, so the list cannot drift from the type" — applied to a trait and its inventory.
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
        let mut out = String::new();
        for line in self.docs.lines().map(str::trim) {
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

    /// The whole doc comment as markdown, with rustdoc's leading space removed.
    ///
    /// Including the `# Errors` section, which is the part a caller most needs: it names
    /// the D13 variants this operation refuses with, and the document's error registry
    /// says what each of those becomes on the wire.
    #[must_use]
    pub fn description(&self) -> String {
        let mut out = String::with_capacity(self.docs.len());
        for line in self.docs.lines() {
            out.push_str(line.strip_prefix(' ').unwrap_or(line));
            out.push('\n');
        }
        out.trim().to_owned()
    }
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

/// Declare the T5 trait and its inventory in one breath.
///
/// The input is the trait as it would be written by hand, minus two things the macro
/// supplies because they are laws rather than choices: `#[rpc(server, client)]` and
/// `param_kind = map` on every method. Named parameters everywhere is D10's posture, not a
/// per-method decision — `param_kind` decides only what the generated *client* sends
/// (`jsonrpsee-proc-macros-0.26.0/src/render_server.rs` branches on `params.is_object()`
/// either way), and a document whose parameters have names is the whole reason to choose.
///
/// The output is the trait plus `METHODS`, in the module the macro is invoked from.
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
