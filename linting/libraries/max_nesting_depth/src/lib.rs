#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{Body, Expr, ExprKind, FnDecl};
use rustc_lint::{LateContext, LateLintPass};
use rustc_span::Span;

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Reports functions whose control-flow nesting exceeds the house
    /// maximum of two levels. Counts the constructs a reader indents
    /// for: `if`/`else`, `match`, `loop`/`for`/`while`, explicit blocks
    /// and closures.
    ///
    /// ### Why is this bad?
    ///
    /// The project's house rule: max nesting level = 2.
    ///
    /// ### Example
    ///
    /// ```rust
    /// // example code where a warning is issued
    /// fn deep() {
    ///     if a { // 1
    ///         if b { // 2
    ///             if c {} // 3 -- too deep
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// // example code that does not raise a warning
    /// fn deep() {
    ///     if a && b && c {} // early returns and small helpers, not stairs
    /// }
    /// ```
    pub MAX_NESTING_DEPTH,
    Deny,
    "nesting deeper than the house maximum of two levels"
}

struct DepthVisitor {
    offending: Span,
    depth: usize,
    max_depth: usize,
}

impl<'tcx> Visitor<'tcx> for DepthVisitor {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        // Macro expansions (tokio::select!, tauri handlers) generate
        // their own staircases; only operator-written code counts.
        if expr.span.from_expansion() {
            return;
        }
        // Only the constructs rustfmt indents into stairs count:
        // if/else, match, and loops. Bare blocks and closures do not
        // (their braces sit inline with the expression they belong to).
        let counted = matches!(
            expr.kind,
            ExprKind::If(..) | ExprKind::Match(..) | ExprKind::Loop(..)
        );
        if counted {
            self.depth += 1;
            if self.depth > self.max_depth {
                self.max_depth = self.depth;
            }
            if self.depth == 3 {
                self.offending = expr.span;
            }
        }
        intravisit::walk_expr(self, expr);
        if counted {
            self.depth -= 1;
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for MaxNestingDepth {
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        _kind: intravisit::FnKind<'tcx>,
        _decl: &'tcx FnDecl<'tcx>,
        body: &'tcx Body<'tcx>,
        _span: Span,
        _def_id: rustc_hir::def_id::LocalDefId,
    ) {
        // Macro-generated code (tauri's handler expansion included) is
        // not ours to lint.
        if body.value.span.from_expansion() {
            return;
        }
        let mut visitor = DepthVisitor {
            offending: body.value.span,
            depth: 0,
            max_depth: 0,
        };
        intravisit::walk_expr(&mut visitor, body.value);
        if visitor.max_depth > 2 {
            clippy_utils::diagnostics::span_lint(
                cx,
                MAX_NESTING_DEPTH,
                visitor.offending,
                format!(
                    "nesting deeper than the house maximum of 2 levels (found depth {})",
                    visitor.max_depth
                ),
            );
        }
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
