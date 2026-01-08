use rustc_hash::{FxHashMap, FxHashSet};

use oxc_ast::ast::AssignmentExpression;
use oxc_span::Span;

pub struct ParserState<'a> {
    pub not_parenthesized_arrow: FxHashSet<u32>,

    /// Temporary storage for `CoverInitializedName` `({ foo = bar })`.
    /// Keyed by `ObjectProperty`'s span.start.
    pub cover_initialized_name: FxHashMap<u32, AssignmentExpression<'a>>,

    /// Trailing comma spans for `ArrayExpression` and `ObjectExpression`.
    /// Used for error reporting.
    /// Keyed by start span of `ArrayExpression` / `ObjectExpression`.
    /// Valued by position of the trailing_comma.
    pub trailing_commas: FxHashMap<u32, Span>,

    /// M6.5.6 Phase 2.1: Track unclosed parentheses for error recovery
    /// Stack of opening paren spans. When we see '(', push its span.
    /// When we see ')', pop from the stack.
    /// If stack is non-empty at synchronization points, we have unclosed parens.
    pub paren_stack: Vec<Span>,
}

impl ParserState<'_> {
    pub fn new() -> Self {
        Self {
            not_parenthesized_arrow: FxHashSet::default(),
            cover_initialized_name: FxHashMap::default(),
            trailing_commas: FxHashMap::default(),
            paren_stack: Vec::new(),
        }
    }

    /// M6.5.6 Phase 2.1: Push opening paren span for tracking
    pub fn push_paren(&mut self, span: Span) {
        self.paren_stack.push(span);
    }

    /// M6.5.6 Phase 2.1: Pop closing paren, returns true if matched
    pub fn pop_paren(&mut self) -> bool {
        self.paren_stack.pop().is_some()
    }

    /// M6.5.6 Phase 2.1: Check if there are unclosed parens
    #[allow(dead_code)]
    pub fn has_unclosed_parens(&self) -> bool {
        !self.paren_stack.is_empty()
    }

    /// M6.5.6 Phase 2.1: Get the span of the oldest unclosed paren
    pub fn first_unclosed_paren(&self) -> Option<Span> {
        self.paren_stack.first().copied()
    }
}
