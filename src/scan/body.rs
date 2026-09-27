//! Function-body walking: `self.<field>` accesses, `self.<method>()` calls,
//! and free-function calls.
//!
//! Method-call resolution is deliberately shallow (spec 01). A call on a
//! receiver whose type we would have to infer is not recorded at all — a
//! hand-rolled resolver that guesses trait dispatch is where this kind of tool
//! goes quietly wrong.

use super::types::path_string;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprField, ExprMethodCall, Member};

#[derive(Default)]
pub struct BodyScan {
    pub self_fields: Vec<String>,
    /// `self.<field>` in a position that consumes the value. A field that
    /// is written and never read is state nobody consults — a different
    /// finding from a field nobody mentions, and a harder one to spot.
    pub self_reads: Vec<String>,
    /// `self.<field> = ...`. Compound assignment reads as well, so it is
    /// left to the ordinary read path.
    pub self_writes: Vec<String>,
    pub self_calls: Vec<String>,
    /// Free-function / associated calls, as written. Resolved later.
    pub calls: Vec<(String, u32)>,
    /// Enum paths named in `match` arm patterns — `Mode::Normal` yields
    /// `Mode`. Two heuristics need to know what a function branches on.
    pub matches_on: Vec<String>,
    /// Methods that abort the thread on failure, as `(method, line)`.
    ///
    /// Recorded by name rather than resolved, because the receiver's type is
    /// not inferred: `x.unwrap()` could be `Result::unwrap`, `Option::unwrap`,
    /// or a domain type's own `unwrap`. The name is enough for the rule that
    /// wants it — a domain that panics is wrong whichever of the three it is —
    /// and a guess about which would be the kind of quiet wrong this scanner
    /// is written to avoid.
    pub panics: Vec<(String, u32)>,
    /// Dotted access chains two fields deep or more, e.g.
    /// `app.registry.entries`: reaching through one object to get at
    /// another's state.
    pub field_chains: Vec<String>,
    /// Named field access on a base that is not `self` — `app.mode`. Not an
    /// LCOM input; it is what stops spec 02 calling a `pub` field dead
    /// because its own type never touches it through `self`.
    pub foreign_fields: Vec<String>,
}

impl BodyScan {
    pub fn of(block: &syn::Block) -> Self {
        let mut s = Self::default();
        s.visit_block(block);
        s.self_fields.sort();
        s.self_fields.dedup();
        s.self_calls.sort();
        s.self_calls.dedup();
        s.self_reads.sort();
        s.self_reads.dedup();
        s.self_writes.sort();
        s.self_writes.dedup();
        s.foreign_fields.sort();
        s.foreign_fields.dedup();
        s.matches_on.sort();
        s.matches_on.dedup();
        s.field_chains.sort();
        s.field_chains.dedup();
        // The token walker restarts at each ident, so `a.b.c` also yields
        // `b.c`. Keep only the longest form of each chain.
        let all = s.field_chains.clone();
        s.field_chains.retain(|c| {
            !all.iter()
                .any(|o| o.len() > c.len() && o.ends_with(&format!(".{c}")))
        });
        s
    }
}

impl BodyScan {
    /// `syn` models a macro invocation as an opaque token stream and does
    /// not descend into it, so every access inside `format!`, `write!` or
    /// `vec!` is invisible to the visitor. In a TUI crate that is most of
    /// the reads: `arioch`'s only use of `Annotation::text` is inside a
    /// `format!`, and without this the field reports as dead.
    ///
    /// Rather than guess at each macro's grammar, match the token shape
    /// `ident . ident`, and treat a following parenthesised group as the
    /// difference between a field and a call.
    fn tokens(&mut self, ts: TokenStream) {
        let t: Vec<TokenTree> = ts.into_iter().collect();
        for (i, tt) in t.iter().enumerate() {
            if let TokenTree::Group(g) = tt {
                self.tokens(g.stream());
                continue;
            }
            let (TokenTree::Ident(base), Some(TokenTree::Punct(dot)), Some(TokenTree::Ident(m))) =
                (tt, t.get(i + 1), t.get(i + 2))
            else {
                continue;
            };
            if dot.as_char() != '.' {
                continue;
            }
            let is_call = matches!(
                t.get(i + 3),
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis
            );
            match (base.to_string().as_str(), is_call) {
                ("self", true) => self.self_calls.push(m.to_string()),
                ("self", false) => {
                    self.self_fields.push(m.to_string());
                    self.self_reads.push(m.to_string());
                }
                (_, false) => self.foreign_fields.push(m.to_string()),
                _ => {}
            }

            // Keep walking `. ident` to recover the whole chain. A render
            // fn's reach through the object graph is almost always inside
            // a `format!`, where the AST visitor cannot see it.
            let mut chain = vec![base.to_string(), m.to_string()];
            let mut j = i + 3;
            while let (Some(TokenTree::Punct(d)), Some(TokenTree::Ident(n))) =
                (t.get(j), t.get(j + 1))
            {
                if d.as_char() != '.' {
                    break;
                }
                chain.push(n.to_string());
                j += 2;
            }
            // A trailing call is not part of the chain: `entry.tags.join(..)`
            // reaches one field deep and then does something with it,
            // which is not reaching through the object graph.
            if is_call {
                chain.pop();
            } else if matches!(
                t.get(j),
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis
            ) {
                chain.pop();
            }
            if chain.len() >= 3 {
                self.field_chains.push(chain.join("."));
            }
        }
    }
}

/// `a.b.c` as a string, or `None` if the base is not a plain path.
fn chain_of(e: &ExprField) -> Option<String> {
    let mut parts = Vec::new();
    let Member::Named(id) = &e.member else {
        return None;
    };
    parts.push(id.to_string());
    let mut cur = &*e.base;
    loop {
        match cur {
            Expr::Field(f) => {
                let Member::Named(id) = &f.member else {
                    return None;
                };
                parts.push(id.to_string());
                cur = &f.base;
            }
            Expr::Path(p) => {
                parts.push(path_string(&p.path));
                break;
            }
            _ => return None,
        }
    }
    parts.reverse();
    Some(parts.join("."))
}

fn is_self(e: &Expr) -> bool {
    matches!(e, Expr::Path(p) if p.path.is_ident("self"))
}

impl<'ast> Visit<'ast> for BodyScan {
    fn visit_expr_field(&mut self, node: &'ast ExprField) {
        if let Some(c) = chain_of(node) {
            // Two dots or more: `app.registry.entries`, not `app.mode`.
            if c.matches('.').count() >= 2 {
                self.field_chains.push(c);
            }
        }
        if let Member::Named(id) = &node.member {
            if is_self(&node.base) {
                self.self_fields.push(id.to_string());
                self.self_reads.push(id.to_string());
            } else {
                self.foreign_fields.push(id.to_string());
            }
        }
        visit::visit_expr_field(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if is_panic(&node.method.to_string()) {
            self.panics
                .push((node.method.to_string(), node.span().start().line as u32));
        }
        if is_self(&node.receiver) {
            self.self_calls.push(node.method.to_string());
        } else if let Expr::Field(f) = &*node.receiver {
            // `self.store.get()` reads the field, but the call belongs to
            // whatever `store` is — not recorded.
            if let Member::Named(id) = &f.member {
                if is_self(&f.base) {
                    self.self_fields.push(id.to_string());
                    self.self_reads.push(id.to_string());
                } else {
                    self.foreign_fields.push(id.to_string());
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        for arm in &node.arms {
            if let syn::Pat::TupleStruct(t) = &arm.pat {
                self.matches_on.push(path_string(&t.path));
            } else if let syn::Pat::Path(p) = &arm.pat {
                self.matches_on.push(path_string(&p.path));
            } else if let syn::Pat::Struct(p) = &arm.pat {
                self.matches_on.push(path_string(&p.path));
            }
        }
        visit::visit_expr_match(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        // Descend into the right side only: the left is the write itself,
        // and counting it as a read would make every assigned field look
        // consulted.
        if let Expr::Field(f) = &*node.left {
            if is_self(&f.base) {
                if let Member::Named(id) = &f.member {
                    self.self_fields.push(id.to_string());
                    self.self_writes.push(id.to_string());
                    self.visit_expr(&node.right);
                    return;
                }
            }
        }
        visit::visit_expr_assign(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.tokens(node.tokens.clone());
        visit::visit_macro(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(p) = &*node.func {
            use syn::spanned::Spanned;
            let line = p.span().start().line as u32;
            self.calls.push((path_string(&p.path), line));
        }
        visit::visit_expr_call(self, node);
    }
}

/// Methods whose failure mode is to abort rather than return. The rule is not
/// "is this `Result::unwrap`" — the receiver is untyped here — but "does this
/// call end the process if it is wrong".
fn is_panic(method: &str) -> bool {
    matches!(method, "unwrap" | "expect" | "unwrap_err" | "expect_err")
}
