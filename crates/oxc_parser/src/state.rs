use rustc_hash::{FxHashMap, FxHashSet};

use oxc_ast::ast::AssignmentExpression;
use oxc_span::Span;

use crate::cursor::ParserCheckpoint;

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

    /// Statements that may need reparsing when `sourceType` is `unambiguous`.
    ///
    /// In unambiguous mode, we initially parse top-level `await ...` as
    /// `await(...)` (identifier/function call). But if ESM syntax is detected
    /// later, we need to reparse these as await expressions.
    ///
    /// Each entry contains: (statement_index, checkpoint_before_statement)
    pub potential_await_reparse: Vec<(usize, ParserCheckpoint<'a>)>,

    /// Flag to track if an `await` identifier was encountered during statement parsing.
    /// Used to determine if a statement needs to be stored for potential reparsing
    /// in unambiguous mode.
    pub encountered_await_identifier: bool,
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
            potential_await_reparse: Vec::new(),
            encountered_await_identifier: false,
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
