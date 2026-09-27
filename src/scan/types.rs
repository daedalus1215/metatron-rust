//! Type walking: turn a `syn::Type` into the set of paths it names.
//!
//! A type is not one reference. `Result<Vec<Total>, DomainError>` names three
//! things, and a rule about what a port signature may mention has to see all
//! of them — spec 01, `port-signature-purity` in spec 04.

use syn::{
    GenericArgument, PathArguments, ReturnType, Type, TypeParamBound,
};

pub fn path_string(p: &syn::Path) -> String {
    let mut s = String::new();
    if p.leading_colon.is_some() {
        s.push_str("::");
    }
    for (i, seg) in p.segments.iter().enumerate() {
        if i > 0 {
            s.push_str("::");
        }
        s.push_str(&seg.ident.to_string());
    }
    s
}

/// Every path named anywhere in the type, generic arguments included.
pub fn paths_in_type(ty: &Type) -> Vec<String> {
    let mut out = Vec::new();
    walk(ty, &mut out);
    out.sort();
    out.dedup();
    out
}

/// Only the paths reached through `impl Trait` / `dyn Trait`. These are the
/// port bounds — the difference between `&impl ActivityStore` and
/// `&SqliteActivityStore`, which is rule `call-through-port`.
pub fn bounds_in_type(ty: &Type) -> Vec<String> {
    let mut out = Vec::new();
    walk_bounds(ty, &mut out);
    out.sort();
    out.dedup();
    out
}

fn walk(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Path(tp) => {
            if let Some(q) = &tp.qself {
                walk(&q.ty, out);
            }
            out.push(path_string(&tp.path));
            for seg in &tp.path.segments {
                walk_args(&seg.arguments, out);
            }
        }
        Type::Reference(r) => walk(&r.elem, out),
        Type::Ptr(p) => walk(&p.elem, out),
        Type::Slice(s) => walk(&s.elem, out),
        Type::Array(a) => walk(&a.elem, out),
        Type::Paren(p) => walk(&p.elem, out),
        Type::Group(g) => walk(&g.elem, out),
        Type::Tuple(t) => t.elems.iter().for_each(|e| walk(e, out)),
        Type::TraitObject(t) => t.bounds.iter().for_each(|b| walk_bound(b, out)),
        Type::ImplTrait(t) => t.bounds.iter().for_each(|b| walk_bound(b, out)),
        Type::BareFn(f) => {
            f.inputs.iter().for_each(|a| walk(&a.ty, out));
            if let ReturnType::Type(_, t) = &f.output {
                walk(t, out);
            }
        }
        _ => {}
    }
}

fn walk_bounds(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::TraitObject(t) => t.bounds.iter().for_each(|b| walk_bound(b, out)),
        Type::ImplTrait(t) => t.bounds.iter().for_each(|b| walk_bound(b, out)),
        Type::Reference(r) => walk_bounds(&r.elem, out),
        Type::Paren(p) => walk_bounds(&p.elem, out),
        Type::Group(g) => walk_bounds(&g.elem, out),
        Type::Ptr(p) => walk_bounds(&p.elem, out),
        _ => {}
    }
}

fn walk_bound(b: &TypeParamBound, out: &mut Vec<String>) {
    if let TypeParamBound::Trait(t) = b {
        out.push(path_string(&t.path));
        for seg in &t.path.segments {
            walk_args(&seg.arguments, out);
        }
    }
}

fn walk_args(args: &PathArguments, out: &mut Vec<String>) {
    match args {
        PathArguments::AngleBracketed(ab) => {
            for a in &ab.args {
                match a {
                    GenericArgument::Type(t) => walk(t, out),
                    GenericArgument::AssocType(at) => walk(&at.ty, out),
                    GenericArgument::Constraint(c) => {
                        c.bounds.iter().for_each(|b| walk_bound(b, out))
                    }
                    _ => {}
                }
            }
        }
        PathArguments::Parenthesized(p) => {
            p.inputs.iter().for_each(|t| walk(t, out));
            if let ReturnType::Type(_, t) = &p.output {
                walk(t, out);
            }
        }
        PathArguments::None => {}
    }
}

/// Bounds declared on a function's generics and `where` clause.
pub fn generic_bounds(g: &syn::Generics) -> Vec<String> {
    let mut out = Vec::new();
    for p in &g.params {
        if let syn::GenericParam::Type(tp) = p {
            for b in &tp.bounds {
                walk_bound(b, &mut out);
            }
        }
    }
    if let Some(w) = &g.where_clause {
        for pred in &w.predicates {
            if let syn::WherePredicate::Type(pt) = pred {
                for b in &pt.bounds {
                    walk_bound(b, &mut out);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn type_to_string(ty: &Type) -> String {
    use quote::ToTokens;
    let s = ty.to_token_stream().to_string();
    // `& 'a str` -> `&'a str`; token streams space everything.
    s.replace(" ::", "::")
        .replace(":: ", "::")
        .replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
        .replace("& ", "&")
}
