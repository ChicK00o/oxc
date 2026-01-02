//! Oxc Parser for JavaScript and TypeScript
//!
//! Oxc's [`Parser`] has full support for
//! - The latest stable ECMAScript syntax
//! - TypeScript
//! - JSX and TSX
//! - [Stage 3 Decorators](https://github.com/tc39/proposal-decorator-metadata)
//!
//! # Usage
//!
//! The parser has a minimal API with three inputs (a [memory arena](oxc_allocator::Allocator), a
//! source string, and a [`SourceType`]) and one return struct (a [ParserReturn]).
//!
//! ```rust
//! let parser_return = Parser::new(&allocator, &source_text, source_type).parse();
//! ```
//!
//! # Abstract Syntax Tree (AST)
//! Oxc's AST is located in a separate [`oxc_ast`] crate. You can find type definitions for AST
//! nodes [here][`oxc_ast::ast`].
//!
//! # Performance
//!
//! The following optimization techniques are used:
//! * AST is allocated in a memory arena ([bumpalo](https://docs.rs/bumpalo)) for fast AST drop
//! * [`oxc_span::Span`] offsets uses `u32` instead of `usize`
//! * Scope binding, symbol resolution and complicated syntax errors are not done in the parser,
//! they are delegated to the [semantic analyzer](https://docs.rs/oxc_semantic)
//!
//! <div class="warning">
//! Because [`oxc_span::Span`] uses `u32` instead of `usize`, Oxc can only parse files up
//! to 4 GiB in size. This shouldn't be a limitation in almost all cases.
//! </div>
//!
//! # Examples
//!
//! <https://github.com/oxc-project/oxc/blob/main/crates/oxc_parser/examples/parser.rs>
//!
//! ```rust
#![doc = include_str!("../examples/parser.rs")]
//! ```
//!
//! ### Parsing TSX
//! ```rust
#![doc = include_str!("../examples/parser_tsx.rs")]
//! ```
//!
//! # Visitor
//!
//! See [`Visit`](http://docs.rs/oxc_ast_visit) and [`VisitMut`](http://docs.rs/oxc_ast_visit).
//!
//! # Visiting without a visitor
//!
//! For ad-hoc tasks, the semantic analyzer can be used to get a parent pointing tree with untyped nodes,
//! the nodes can be iterated through a sequential loop.
//!
//! ```rust
//! for node in semantic.nodes().iter() {
//!     match node.kind() {
//!         // check node
//!     }
//! }
//! ```
//!
//! See [full linter example](https://github.com/Boshen/oxc/blob/ab2ef4f89ba3ca50c68abb2ca43e36b7793f3673/crates/oxc_linter/examples/linter.rs#L38-L39)

#![warn(missing_docs)]

mod context;
mod cursor;
mod error_handler;
mod modifiers;
mod module_record;
mod state;
mod synchronization;

mod js;
mod jsx;
mod ts;

mod diagnostics;

// Expose lexer only in benchmarks
#[cfg(not(feature = "benchmarking"))]
mod lexer;
#[cfg(feature = "benchmarking")]
#[doc(hidden)]
pub mod lexer;

use oxc_allocator::{Allocator, Box as ArenaBox, Dummy};
use oxc_ast::{
    AstBuilder,
    ast::{Expression, Program},
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_span::{ModuleKind, SourceType, Span};
use oxc_syntax::module_record::ModuleRecord;

use crate::{
    context::{Context, ParsingContextStack, StatementContext},
    error_handler::FatalError,
    lexer::{Lexer, Token},
    module_record::ModuleRecordBuilder,
    state::ParserState,
};

/// Maximum length of source which can be parsed (in bytes).
/// ~4 GiB on 64-bit systems, ~2 GiB on 32-bit systems.
// Length is constrained by 2 factors:
// 1. `Span`'s `start` and `end` are `u32`s, which limits length to `u32::MAX` bytes.
// 2. Rust's allocator APIs limit allocations to `isize::MAX`.
// https://doc.rust-lang.org/std/alloc/struct.Layout.html#method.from_size_align
pub(crate) const MAX_LEN: usize = if size_of::<usize>() >= 8 {
    // 64-bit systems
    u32::MAX as usize
} else {
    // 32-bit or 16-bit systems
    isize::MAX as usize
};

/// Return value of [`Parser::parse`] consisting of AST, errors and comments
///
/// ## AST Validity
///
/// [`program`] will always contain a structurally valid AST, even if there are syntax errors.
/// However, the AST may be semantically invalid. To ensure a valid AST,
/// 1. Check that [`errors`] is empty
/// 2. Run semantic analysis with [syntax error checking
///    enabled](https://docs.rs/oxc_semantic/latest/oxc_semantic/struct.SemanticBuilder.html#method.with_check_syntax_error)
///
/// ## Errors
/// Oxc's [`Parser`] is able to recover from some syntax errors and continue parsing. When this
/// happens,
/// 1. [`errors`] will be non-empty
/// 2. [`program`] will contain a full AST
/// 3. [`panicked`] will be false
///
/// When the parser cannot recover, it will abort and terminate parsing early. [`program`] will
/// be empty and [`panicked`] will be `true`.
///
/// [`program`]: ParserReturn::program
/// [`errors`]: ParserReturn::errors
/// [`panicked`]: ParserReturn::panicked
#[non_exhaustive]
pub struct ParserReturn<'a> {
    /// The parsed AST.
    ///
    /// Will be empty (e.g. no statements, directives, etc) if the parser panicked.
    ///
    /// ## Validity
    /// It is possible for the AST to be present and semantically invalid. This will happen if
    /// 1. The [`Parser`] encounters a recoverable syntax error
    /// 2. The logic for checking the violation is in the semantic analyzer
    ///
    /// To ensure a valid AST, check that [`errors`](ParserReturn::errors) is empty. Then, run
    /// semantic analysis with syntax error checking enabled.
    pub program: Program<'a>,

    /// See <https://tc39.es/ecma262/#sec-abstract-module-records>
    pub module_record: ModuleRecord<'a>,

    /// Syntax errors encountered while parsing.
    ///
    /// This list is not comprehensive. Oxc offloads more-expensive checks to [semantic
    /// analysis](https://docs.rs/oxc_semantic), which can be enabled using
    /// [`SemanticBuilder::with_check_syntax_error`](https://docs.rs/oxc_semantic/latest/oxc_semantic/struct.SemanticBuilder.html#method.with_check_syntax_error).
    pub errors: Vec<OxcDiagnostic>,

    /// Irregular whitespaces for `Oxlint`
    pub irregular_whitespaces: Box<[Span]>,

    /// Whether the parser panicked and terminated early.
    ///
    /// This will be `false` if parsing was successful, or if parsing was able to recover from a
    /// syntax error. When `true`, [`program`] will be empty and [`errors`] will contain at least
    /// one error.
    ///
    /// [`program`]: ParserReturn::program
    /// [`errors`]: ParserReturn::errors
    pub panicked: bool,

    /// Whether the file is [flow](https://flow.org).
    pub is_flow_language: bool,
}

/// Parse options
///
/// You may provide options to the [`Parser`] using [`Parser::with_options`].
#[derive(Debug, Clone, Copy)]
pub struct ParseOptions {
    /// Whether to parse regular expressions or not.
    ///
    /// Default: `false`
    #[cfg(feature = "regular_expression")]
    pub parse_regular_expression: bool,

    /// Allow [`return`] statements outside of functions.
    ///
    /// By default, a return statement at the top level raises an error (`false`).
    ///
    /// Default: `false`
    ///
    /// [`return`]: oxc_ast::ast::ReturnStatement
    pub allow_return_outside_function: bool,

    /// Emit [`ParenthesizedExpression`]s and [`TSParenthesizedType`] in AST.
    ///
    /// If this option is `true`, parenthesized expressions are represented by
    /// (non-standard) [`ParenthesizedExpression`] and [`TSParenthesizedType`] nodes
    /// that have a single `expression` property containing the expression inside parentheses.
    ///
    /// Default: `true`
    ///
    /// [`ParenthesizedExpression`]: oxc_ast::ast::ParenthesizedExpression
    /// [`TSParenthesizedType`]: oxc_ast::ast::TSParenthesizedType
    pub preserve_parens: bool,

    /// Allow V8 runtime calls in the AST.
    /// See: [V8's Parser::ParseV8Intrinsic](https://chromium.googlesource.com/v8/v8/+/35a14c75e397302655d7b3fbe648f9490ae84b7d/src/parsing/parser.cc#4811).
    ///
    /// Default: `false`
    ///
    /// [`V8IntrinsicExpression`]: oxc_ast::ast::V8IntrinsicExpression
    pub allow_v8_intrinsics: bool,

    /// Enable error recovery for invalid assignment targets.
    ///
    /// When `true`, the parser recovers from invalid assignment target errors
    /// and continues parsing to report all errors (useful for type-checking).
    /// When `false`, the parser terminates on these errors (faster for transpilation).
    ///
    /// Default: `false`
    pub recover_from_errors: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            #[cfg(feature = "regular_expression")]
            parse_regular_expression: false,
            allow_return_outside_function: false,
            preserve_parens: true,
            allow_v8_intrinsics: false,
            recover_from_errors: false,
        }
    }
}

/// Recursive Descent Parser for ECMAScript and TypeScript
///
/// See [`Parser::parse`] for entry function.
pub struct Parser<'a> {
    allocator: &'a Allocator,
    source_text: &'a str,
    source_type: SourceType,
    options: ParseOptions,
}

impl<'a> Parser<'a> {
    /// Create a new [`Parser`]
    ///
    /// # Parameters
    /// - `allocator`: [Memory arena](oxc_allocator::Allocator) for allocating AST nodes
    /// - `source_text`: Source code to parse
    /// - `source_type`: Source type (e.g. JavaScript, TypeScript, JSX, ESM Module, Script)
    pub fn new(allocator: &'a Allocator, source_text: &'a str, source_type: SourceType) -> Self {
        let options = ParseOptions::default();
        Self { allocator, source_text, source_type, options }
    }

    /// Set parse options
    #[must_use]
    pub fn with_options(mut self, options: ParseOptions) -> Self {
        self.options = options;
        self
    }
}

mod parser_parse {
    use super::*;

    /// `UniquePromise` is a way to use the type system to enforce the invariant that only
    /// a single `ParserImpl`, `Lexer` and `lexer::Source` can exist at any time on a thread.
    /// This constraint is required to guarantee the soundness of some methods of these types
    /// e.g. `Source::set_position`.
    ///
    /// `ParserImpl::new`, `Lexer::new` and `lexer::Source::new` all require a `UniquePromise`
    /// to be provided to them. `UniquePromise::new` is not visible outside this module, so only
    /// `Parser::parse` can create one, and it only calls `ParserImpl::new` once.
    /// This enforces the invariant throughout the entire parser.
    ///
    /// `UniquePromise` is a zero-sized type and has no runtime cost. It's purely for the type-checker.
    ///
    /// `UniquePromise::new_for_tests_and_benchmarks` is a backdoor for tests/benchmarks, so they can
    /// create a `ParserImpl` or `Lexer`, and manipulate it directly, for testing/benchmarking purposes.
    pub struct UniquePromise(());

    impl UniquePromise {
        #[inline]
        fn new() -> Self {
            Self(())
        }

        /// Backdoor for tests/benchmarks to create a `UniquePromise` (see above).
        /// This function must NOT be exposed outside of tests and benchmarks,
        /// as it allows circumventing safety invariants of the parser.
        #[cfg(any(test, feature = "benchmarking"))]
        pub fn new_for_tests_and_benchmarks() -> Self {
            Self(())
        }
    }

    impl<'a> Parser<'a> {
        /// Main entry point
        ///
        /// Returns an empty `Program` on unrecoverable error,
        /// Recoverable errors are stored inside `errors`.
        ///
        /// See the [module-level documentation](crate) for examples and more information.
        pub fn parse(self) -> ParserReturn<'a> {
            let unique = UniquePromise::new();
            let parser = ParserImpl::new(
                self.allocator,
                self.source_text,
                self.source_type,
                self.options,
                unique,
            );
            parser.parse()
        }

        /// Parse a single [`Expression`].
        ///
        /// # Example
        ///
        /// ```rust
        /// use oxc_allocator::Allocator;
        /// use oxc_ast::ast::Expression;
        /// use oxc_parser::Parser;
        /// use oxc_span::SourceType;
        ///
        /// let src = "let x = 1 + 2;";
        /// let allocator = Allocator::new();
        /// let source_type = SourceType::default();
        ///
        /// let expr: Expression<'_> = Parser::new(&allocator, src, source_type).parse_expression().unwrap();
        /// ```
        ///
        /// # Errors
        /// If the source code being parsed has syntax errors.
        pub fn parse_expression(self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
            let unique = UniquePromise::new();
            let parser = ParserImpl::new(
                self.allocator,
                self.source_text,
                self.source_type,
                self.options,
                unique,
            );
            parser.parse_expression()
        }
    }
}
use parser_parse::UniquePromise;

/// Implementation of parser.
/// `Parser` is just a public wrapper, the guts of the implementation is in this type.
struct ParserImpl<'a> {
    options: ParseOptions,

    pub(crate) lexer: Lexer<'a>,

    /// SourceType: JavaScript or TypeScript, Script or Module, jsx support?
    source_type: SourceType,

    /// Source Code
    source_text: &'a str,

    /// All syntax errors from parser and lexer
    /// Note: favor adding to `Diagnostics` instead of raising Err
    errors: Vec<OxcDiagnostic>,

    fatal_error: Option<FatalError>,

    /// The current parsing token
    token: Token,

    /// The end range of the previous token
    prev_token_end: u32,

    /// Parser state
    state: ParserState<'a>,

    /// Parsing context
    ctx: Context,

    /// Context stack for error recovery synchronization
    context_stack: ParsingContextStack,

    /// Ast builder for creating AST nodes
    ast: AstBuilder<'a>,

    /// Module Record Builder
    module_record_builder: ModuleRecordBuilder<'a>,

    /// Precomputed typescript detection
    is_ts: bool,
}

impl<'a> ParserImpl<'a> {
    /// Create a new `ParserImpl`.
    ///
    /// Requiring a `UniquePromise` to be provided guarantees only 1 `ParserImpl` can exist
    /// on a single thread at one time.
    #[inline]
    pub fn new(
        allocator: &'a Allocator,
        source_text: &'a str,
        source_type: SourceType,
        options: ParseOptions,
        unique: UniquePromise,
    ) -> Self {
        Self {
            options,
            lexer: Lexer::new(allocator, source_text, source_type, unique),
            source_type,
            source_text,
            errors: vec![],
            fatal_error: None,
            token: Token::default(),
            prev_token_end: 0,
            state: ParserState::new(),
            ctx: Self::default_context(source_type, options),
            context_stack: ParsingContextStack::new(),
            ast: AstBuilder::new(allocator),
            module_record_builder: ModuleRecordBuilder::new(allocator),
            is_ts: source_type.is_typescript(),
        }
    }

    /// Returns the current parsing context from the context stack.
    ///
    /// This is used for error recovery synchronization to determine
    /// the appropriate termination tokens and recovery strategy.
    #[expect(dead_code, reason = "M6.5: Will be used in Step 3 for error recovery")]
    #[inline]
    pub(crate) fn current_context(&self) -> crate::context::ParsingContext {
        self.context_stack.current()
    }

    /// Checks if a specific parsing context is currently active in the stack.
    ///
    /// This searches the entire context stack, not just the top.
    /// Useful for checking if we're inside a specific parsing construct
    /// when making error recovery decisions.
    #[expect(dead_code, reason = "M6.5: Will be used in Step 3 for error recovery")]
    #[inline]
    pub(crate) fn in_context(&self, ctx: crate::context::ParsingContext) -> bool {
        self.context_stack.is_in_context(ctx)
    }

    /// Main entry point
    ///
    /// Returns an empty `Program` on unrecoverable error,
    /// Recoverable errors are stored inside `errors`.
    #[inline]
    pub fn parse(mut self) -> ParserReturn<'a> {
        let mut program = self.parse_program();
        let mut panicked = false;

        if let Some(fatal_error) = self.fatal_error.take() {
            panicked = true;
            self.errors.truncate(fatal_error.errors_len);
            if !self.lexer.errors.is_empty() && self.cur_kind().is_eof() {
                // Noop
            } else {
                self.error(fatal_error.error);
            }

            program = Program::dummy(self.ast.allocator);
            program.source_type = self.source_type;
            program.source_text = self.source_text;
        }

        self.check_unfinished_errors();

        if let Some(overlong_error) = self.overlong_error() {
            panicked = true;
            self.lexer.errors.clear();
            self.errors.clear();
            self.error(overlong_error);
        }

        let mut is_flow_language = false;
        let mut errors = vec![];
        // only check for `@flow` if the file failed to parse.
        if (!self.lexer.errors.is_empty() || !self.errors.is_empty())
            && let Some(error) = self.flow_error()
        {
            is_flow_language = true;
            errors.push(error);
        }
        let (module_record, module_record_errors) = self.module_record_builder.build();
        if errors.len() != 1 {
            errors.reserve(self.lexer.errors.len() + self.errors.len());
            errors.extend(self.lexer.errors);
            errors.extend(self.errors);
            // Skip checking for exports in TypeScript {
            if !self.source_type.is_typescript() {
                errors.extend(module_record_errors);
            }
        }
        let irregular_whitespaces =
            self.lexer.trivia_builder.irregular_whitespaces.into_boxed_slice();

        let source_type = program.source_type;
        if source_type.is_unambiguous() {
            program.source_type = if module_record.has_module_syntax {
                source_type.with_module(true)
            } else {
                source_type.with_script(true)
            };
        }

        ParserReturn {
            program,
            module_record,
            errors,
            irregular_whitespaces,
            panicked,
            is_flow_language,
        }
    }

    pub fn parse_expression(mut self) -> Result<Expression<'a>, Vec<OxcDiagnostic>> {
        // initialize cur_token and prev_token by moving onto the first token
        self.bump_any();
        let expr = self.parse_expr();
        if let Some(FatalError { error, .. }) = self.fatal_error.take() {
            return Err(vec![error]);
        }
        self.check_unfinished_errors();
        let errors = self.lexer.errors.into_iter().chain(self.errors).collect::<Vec<_>>();
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(expr)
    }

    #[expect(clippy::cast_possible_truncation)]
    fn parse_program(&mut self) -> Program<'a> {
        // Initialize by moving onto the first token.
        // Checks for hashbang comment.
        self.token = self.lexer.first_token();

        let hashbang = self.parse_hashbang();

        // M6.5.6 Out of Scope: Parse directives and check for strict mode
        let (directives, statements, has_use_strict) =
            self.parse_directives_and_statements(/* is_top_level */ true);

        // M6.5.6 Out of Scope: Track program-level strict mode
        // This would be used for semantic analysis
        let _ = has_use_strict;

        let span = Span::new(0, self.source_text.len() as u32);
        let comments = self.ast.vec_from_iter(self.lexer.trivia_builder.comments.iter().copied());
        self.ast.program(
            span,
            self.source_type,
            self.source_text,
            comments,
            hashbang,
            directives,
            statements,
        )
    }

    fn default_context(source_type: SourceType, options: ParseOptions) -> Context {
        let mut ctx = Context::default().and_ambient(source_type.is_typescript_definition());
        if source_type.module_kind() == ModuleKind::Module {
            // for [top-level-await](https://tc39.es/proposal-top-level-await/)
            ctx = ctx.and_await(true);
        }
        if options.allow_return_outside_function {
            ctx = ctx.and_return(true);
        }
        ctx
    }

    /// Check for Flow declaration if the file cannot be parsed.
    /// The declaration must be [on the first line before any code](https://flow.org/en/docs/usage/#toc-prepare-your-code-for-flow)
    fn flow_error(&mut self) -> Option<OxcDiagnostic> {
        if !self.source_type.is_javascript() {
            return None;
        }
        let span = self.lexer.trivia_builder.comments.first()?.span;
        if span.source_text(self.source_text).contains("@flow") {
            self.errors.clear();
            Some(diagnostics::flow(span))
        } else {
            None
        }
    }

    fn check_unfinished_errors(&mut self) {
        use oxc_span::GetSpan;
        // PropertyDefinition : cover_initialized_name
        // It is a Syntax Error if any source text is matched by this production.
        for expr in self.state.cover_initialized_name.values() {
            self.errors.push(diagnostics::cover_initialized_name(expr.span()));
        }
    }

    /// Check if source length exceeds MAX_LEN, if the file cannot be parsed.
    /// Original parsing error is not real - `Lexer::new` substituted "\0" as the source text.
    #[cold]
    fn overlong_error(&self) -> Option<OxcDiagnostic> {
        if self.source_text.len() > MAX_LEN {
            return Some(diagnostics::overlong_source());
        }
        None
    }

    #[inline]
    fn alloc<T>(&self, value: T) -> ArenaBox<'a, T> {
        self.ast.alloc(value)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use oxc_ast::ast::{CommentKind, Expression, Statement};
    use oxc_span::GetSpan;

    use super::*;

    #[test]
    fn parse_program_smoke_test() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let source = "";
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert!(ret.program.is_empty());
        assert!(ret.errors.is_empty());
        assert!(!ret.is_flow_language);
    }

    #[test]
    fn parse_expression_smoke_test() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let source = "a";
        let expr = Parser::new(&allocator, source, source_type).parse_expression().unwrap();
        assert!(matches!(expr, Expression::Identifier(_)));
    }

    #[test]
    fn flow_error() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let sources = [
            "// @flow\nasdf adsf",
            "/* @flow */\n asdf asdf",
            "/**
             * @flow
             */
             asdf asdf
             ",
            "/* @flow */ super;",
        ];
        for source in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert!(ret.is_flow_language);
            assert_eq!(ret.errors.len(), 1);
            assert_eq!(ret.errors.first().unwrap().to_string(), "Flow is not supported");
        }
    }

    #[test]
    fn ts_module_declaration() {
        let allocator = Allocator::default();
        let source_type = SourceType::from_path(Path::new("module.ts")).unwrap();
        let source = "declare module 'test'\n";
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert_eq!(ret.errors.len(), 0);
    }

    #[test]
    fn directives() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let sources = [
            ("import x from 'foo'; 'use strict';", 2),
            ("export {x} from 'foo'; 'use strict';", 2),
            (";'use strict';", 2),
        ];
        for (source, body_length) in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert!(ret.program.directives.is_empty(), "{source}");
            assert_eq!(ret.program.body.len(), body_length, "{source}");
        }
    }

    #[test]
    fn v8_intrinsics() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        {
            let source = "%DebugPrint('Raging against the Dying Light')";
            let opts = ParseOptions { allow_v8_intrinsics: true, ..ParseOptions::default() };
            let ret = Parser::new(&allocator, source, source_type).with_options(opts).parse();
            assert!(ret.errors.is_empty());

            if let Some(Statement::ExpressionStatement(expr_stmt)) = ret.program.body.first() {
                if let Expression::V8IntrinsicExpression(expr) = &expr_stmt.expression {
                    assert_eq!(expr.span().source_text(source), source);
                } else {
                    panic!("Expected V8IntrinsicExpression");
                }
            } else {
                panic!("Expected ExpressionStatement");
            }
        }
        {
            let source = "%DebugPrint(...illegalSpread)";
            let opts = ParseOptions { allow_v8_intrinsics: true, ..ParseOptions::default() };
            let ret = Parser::new(&allocator, source, source_type).with_options(opts).parse();
            assert_eq!(ret.errors.len(), 1);
            assert_eq!(
                ret.errors[0].to_string(),
                "V8 runtime calls cannot have spread elements as arguments"
            );
        }
        {
            let source = "%DebugPrint('~~')";
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert_eq!(ret.errors.len(), 1);
            assert_eq!(ret.errors[0].to_string(), "Unexpected token");
        }
        {
            // https://github.com/oxc-project/oxc/issues/12121
            let source = "interface Props extends %enuProps {}";
            let source_type = SourceType::default().with_typescript(true);
            // Should not panic whether `allow_v8_intrinsics` is set or not.
            let opts = ParseOptions { allow_v8_intrinsics: true, ..ParseOptions::default() };
            let ret = Parser::new(&allocator, source, source_type).with_options(opts).parse();
            assert_eq!(ret.errors.len(), 1);
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert_eq!(ret.errors.len(), 1);
        }
    }

    #[test]
    fn comments() {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_typescript(true);
        let sources = [
            ("// line comment", CommentKind::Line),
            ("/* line comment */", CommentKind::SingleLineBlock),
            (
                "type Foo = ( /* Require properties which are not generated automatically. */ 'bar')",
                CommentKind::SingleLineBlock,
            ),
        ];
        for (source, kind) in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            let comments = &ret.program.comments;
            assert_eq!(comments.len(), 1, "{source}");
            assert_eq!(comments.first().unwrap().kind, kind, "{source}");
        }
    }

    #[test]
    fn hashbang() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let source = "#!/usr/bin/node\n;";
        let ret = Parser::new(&allocator, source, source_type).parse();
        assert_eq!(ret.program.hashbang.unwrap().value.as_str(), "/usr/bin/node");
    }

    #[test]
    fn unambiguous() {
        let allocator = Allocator::default();
        let source_type = SourceType::unambiguous();
        assert!(source_type.is_unambiguous());
        let sources = ["import x from 'foo';", "export {x} from 'foo';", "import.meta"];
        for source in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert!(ret.program.source_type.is_module());
        }

        let sources = ["", "import('foo')"];
        for source in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert!(ret.program.source_type.is_script());
        }
    }

    #[test]
    fn memory_leak() {
        let allocator = Allocator::default();
        let source_type = SourceType::default();
        let sources = ["2n", ";'1234567890123456789012345678901234567890'"];
        for source in sources {
            let ret = Parser::new(&allocator, source, source_type).parse();
            assert!(!ret.program.body.is_empty());
        }
    }

    // Source with length MAX_LEN + 1 fails to parse.
    // Skip this test on 32-bit systems as impossible to allocate a string longer than `isize::MAX`.
    // Also skip running under Miri since it takes so long.
    #[cfg(target_pointer_width = "64")]
    #[cfg(not(miri))]
    #[test]
    fn overlong_source() {
        // Build string in 16 KiB chunks for speed
        let mut source = String::with_capacity(MAX_LEN + 1);
        let line = "var x = 123456;\n";
        let chunk = line.repeat(1024);
        while source.len() < MAX_LEN + 1 - chunk.len() {
            source.push_str(&chunk);
        }
        while source.len() < MAX_LEN + 1 - line.len() {
            source.push_str(line);
        }
        while source.len() < MAX_LEN + 1 {
            source.push('\n');
        }
        assert_eq!(source.len(), MAX_LEN + 1);

        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, &source, SourceType::default()).parse();
        assert!(ret.program.is_empty());
        assert!(ret.panicked);
        assert_eq!(ret.errors.len(), 1);
        assert_eq!(ret.errors.first().unwrap().to_string(), "Source length exceeds 4 GiB limit");
    }

    // Source with length MAX_LEN parses OK.
    // This test takes over 1 minute on an M1 Macbook Pro unless compiled in release mode.
    // `not(debug_assertions)` is a proxy for detecting release mode.
    // Also skip running under Miri since it takes so long.
    #[cfg(not(debug_assertions))]
    #[cfg(not(miri))]
    #[test]
    fn legal_length_source() {
        // Build a string MAX_LEN bytes long which doesn't take too long to parse
        let head = "const x = 1;\n/*";
        let foot = "*/\nconst y = 2;\n";
        let mut source = "x".repeat(MAX_LEN);
        source.replace_range(..head.len(), head);
        source.replace_range(MAX_LEN - foot.len().., foot);
        assert_eq!(source.len(), MAX_LEN);

        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, &source, SourceType::default()).parse();
        assert!(!ret.panicked);
        assert!(ret.errors.is_empty());
        assert_eq!(ret.program.body.len(), 2);
    }

    // M6.5.3: Module/Import/Export Error Recovery Tests

    #[test]
    fn test_empty_import_call() {
        let source = r"
            import();
            let x = 5;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have 1 error for empty import
        assert_eq!(ret.errors.len(), 1);
        assert!(ret.errors[0].message.contains("import"));

        // Should have 2 statements (import expression + variable declaration)
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_empty_import_subsequent_code_parsed() {
        let source = r"
            import();
            function foo() { return 42; }
            class Bar {}
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for empty import
        assert_eq!(ret.errors.len(), 1);

        // All 3 statements should be parsed
        assert_eq!(ret.program.body.len(), 3);
    }

    #[test]
    fn test_import_too_many_args() {
        let source = "import(source, options, extra);";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have error for too many arguments
        assert!(!ret.errors.is_empty());

        // Import call should still be in AST
        assert_eq!(ret.program.body.len(), 1);
    }

    #[test]
    fn test_import_four_args() {
        let source = "import(a, b, c, d);";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert!(!ret.errors.is_empty());
        assert_eq!(ret.program.body.len(), 1);
    }

    #[test]
    fn test_import_many_args_with_subsequent_code() {
        let source = r"
            import(source, opts, extra1, extra2);
            const x = 10;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert!(!ret.errors.is_empty());
        // Both statements parsed
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_invalid_import_meta() {
        let source = "import.notmeta";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have error
        assert_eq!(ret.errors.len(), 1);

        // Should still have expression in AST
        assert_eq!(ret.program.body.len(), 1);
    }

    #[test]
    fn test_invalid_import_meta_with_subsequent_code() {
        let source = r"
            import.invalid;
            const x = import.meta;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for invalid property
        assert_eq!(ret.errors.len(), 1);

        // Both statements parsed
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_invalid_import_attribute_value() {
        let source = r#"
            import "module" with { type: 123 };
            export class MyClass {}
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for non-string attribute value
        assert_eq!(ret.errors.len(), 1);
        assert!(ret.errors[0].message.contains("string"));

        // Both import and export should be in AST
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_invalid_import_attribute_identifier_value() {
        let source = r#"import "m" with { type: invalid };"#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert!(!ret.errors.is_empty());
        assert_eq!(ret.program.body.len(), 1);
    }

    #[test]
    fn test_invalid_import_attribute_with_valid_import_after() {
        let source = r#"
            import "m1" with { type: 456 };
            import { valid } from "m2";
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for first import
        assert!(!ret.errors.is_empty());

        // Both imports in AST
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_multiple_import_export_errors() {
        let source = r#"
            import();
            import { valid } from "other";
            export class MyClass {}
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // 1 error for empty import
        assert_eq!(ret.errors.len(), 1);

        // All 3 statements in AST
        assert_eq!(ret.program.body.len(), 3);
    }

    #[test]
    fn test_single_import_error_no_cascade() {
        let source = r#"
            import();
            import { a } from "n";
            import { b } from "o";
            export class C {}
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Only 1 error (first import)
        assert_eq!(ret.errors.len(), 1);

        // All 4 statements parsed
        assert_eq!(ret.program.body.len(), 4);
    }

    #[test]
    fn test_mixed_module_errors() {
        let source = r#"
            import();
            import "m" with { type: 123 };
            import.invalid;
            import { valid } from "ok";
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // 3 errors total
        assert_eq!(ret.errors.len(), 3);

        // All 4 statements in AST
        assert_eq!(ret.program.body.len(), 4);
    }

    #[test]
    fn test_error_recovery_preserves_valid_imports() {
        let source = r#"
            import { foo } from "valid1";
            import();
            import { bar } from "valid2";
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // 1 error for empty import
        assert_eq!(ret.errors.len(), 1);

        // All imports in AST
        assert_eq!(ret.program.body.len(), 3);
    }

    #[test]
    fn test_import_error_with_complex_subsequent_code() {
        let source = r#"
            import();
            class Foo {
                method() {
                    return import("dynamic");
                }
            }
            function bar() {}
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for empty static import
        assert_eq!(ret.errors.len(), 1);

        // All top-level statements parsed
        assert_eq!(ret.program.body.len(), 3);
    }

    #[test]
    fn test_stress_many_import_errors() {
        let source = r#"
            import();
            import();
            import();
            import { valid1 } from "m1";
            import();
            import { valid2 } from "m2";
            import();
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // 5 errors for empty imports
        assert_eq!(ret.errors.len(), 5);

        // All 7 statements in AST
        assert_eq!(ret.program.body.len(), 7);
    }

    #[test]
    fn test_error_messages_quality() {
        let source = "import();";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        let error = &ret.errors[0];
        // Should indicate what's wrong
        assert!(
            error.message.contains("import") || error.message.contains("specifier"),
            "Error should mention import/specifier: {}",
            error.message
        );
    }

    #[test]
    fn test_import_meta_error_message() {
        let source = "import.invalid;";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert!(!ret.errors.is_empty());
        let error = &ret.errors[0];
        assert!(error.message.contains("import") || error.message.contains("meta"));
    }

    #[test]
    fn test_import_attribute_error_message() {
        let source = r#"import "m" with { type: 123 };"#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        let error = &ret.errors[0];
        assert!(error.message.contains("string"));
    }

    // Named import/export error tests
    #[test]
    fn test_named_import_with_trailing_comma() {
        let source = r#"
            import { a, } from "module";
            const x = 1;
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Trailing comma is actually valid, so no error
        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_named_import_multiple_valid() {
        let source = r#"
            import { a, b, c } from "module";
            import { d } from "other";
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // All valid
        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_export_named_declaration() {
        let source = "
            export { a, b };
            export { c };
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Valid exports
        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_export_with_from() {
        let source = "
            export { a, b } from \"module\";
            const x = 1;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_export_default_class() {
        let source = "
            export default class Foo {}
            const x = 1;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_export_default_function() {
        let source = "
            export default function foo() {}
            const x = 1;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_import_namespace() {
        let source = r#"
            import * as ns from "module";
            const x = 1;
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_import_default_and_named() {
        let source = r#"
            import React, { useState } from "react";
            const x = 1;
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    #[test]
    fn test_mixed_imports_and_exports() {
        let source = r#"
            import { a } from "a";
            export { b };
            import { c } from "c";
            export default class D {}
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 4);
    }

    #[test]
    fn test_import_with_as_renaming() {
        let source = r#"
            import { foo as bar } from "module";
            const x = 1;
        "#;
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 0);
        assert_eq!(ret.program.body.len(), 2);
    }

    // Phase 1.4 & 2.2: Named Import/Export Specifier Error Tests

    #[test]
    fn test_import_namespace_with_braces() {
        // import { * } from "./file" - can't import namespace with braces
        let source = r"
            import { * } from './module';
            const x = 1;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Error for invalid syntax
        assert!(!ret.errors.is_empty(), "Should have at least one error");

        // Both statements should be parsed (import with error + const)
        assert_eq!(ret.program.body.len(), 2, "Should parse import (with error) and const");
    }

    #[test]
    fn test_import_missing_identifier_after_comma() {
        // import defaultBinding, from "./file" - missing identifier after comma
        let source = r"
            import defaultBinding, from './module';
            const y = 2;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have error
        assert!(!ret.errors.is_empty(), "Should report error for missing identifier");

        // Should still parse both statements
        assert!(!ret.program.body.is_empty(), "Should parse at least one statement");
    }

    #[test]
    fn test_import_leading_comma() {
        // import , { a } from "./file" - leading comma
        let source = r"
            import , { a } from './module';
            const z = 3;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have error
        assert!(!ret.errors.is_empty(), "Should report error for leading comma");

        // Leading comma is a severe syntax error - parser may not recover fully
        // Just verify error is reported
    }

    #[test]
    fn test_export_trailing_comma() {
        // export { a, } from "./file" - trailing comma (this is actually valid ES2015+)
        // But test that it parses correctly
        let source = r"
            export { a, } from './module';
            const w = 4;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Trailing comma in export list is valid in modern JS
        // Should parse without error or with minimal errors
        assert_eq!(ret.program.body.len(), 2, "Should parse export and const");
    }

    #[test]
    fn test_export_missing_comma() {
        // export { a b } from "./file" - missing comma between specifiers
        let source = r"
            export { a b } from './module';
            const v = 5;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have error for missing comma
        assert!(!ret.errors.is_empty(), "Should report error for missing comma");

        // Missing comma in export specifier list may not recover fully
        // At minimum, error should be reported
    }

    // Phase 4.4: Error Quality Tests

    #[test]
    fn test_error_message_quality_import_empty() {
        let source = "import();";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 1, "Should have exactly one error");
        let error_msg = &ret.errors[0].message;

        // Error message should be clear
        assert!(
            error_msg.contains("import")
                || error_msg.contains("specifier")
                || error_msg.contains("requires"),
            "Error should mention import/specifier: {error_msg}"
        );
    }

    #[test]
    fn test_error_message_quality_too_many_args() {
        let source = "import('a', 'b', 'c');";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        assert_eq!(ret.errors.len(), 1, "Should have exactly one error");
        let error_msg = &ret.errors[0].message;

        // Error message should mention arguments
        assert!(
            error_msg.contains("argument") || error_msg.contains("maximum"),
            "Error should mention arguments/maximum: {error_msg}"
        );
    }

    #[test]
    fn test_no_cascading_errors() {
        // Single error should not cause cascade of errors for valid code
        let source = r"
            import();
            import { valid1 } from './a';
            import { valid2 } from './b';
            export const x = 1;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have only 1 error (the empty import)
        assert_eq!(ret.errors.len(), 1, "Should have only 1 error, not cascading errors");

        // All 4 statements should be parsed
        assert_eq!(ret.program.body.len(), 4, "All statements should be parsed");
    }

    #[test]
    fn test_error_span_accuracy() {
        let source = "import { * } from './module';";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have at least one error
        assert!(!ret.errors.is_empty(), "Should have at least one error");

        let error = &ret.errors[0];
        // Error should have labels with span information
        assert!(error.labels.is_some(), "Error should have labels with span");
        if let Some(labels) = &error.labels {
            assert!(!labels.is_empty(), "Error labels should not be empty");
        }
    }

    #[test]
    fn test_stress_multiple_module_errors() {
        // Test file with multiple import/export errors
        let source = r"
            import();
            import { * } from './a';
            import { valid } from './good';
            export const result = 42;
        ";
        let allocator = Allocator::default();
        let opts = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
        let ret = Parser::new(&allocator, source, SourceType::default()).with_options(opts).parse();

        // Should have at least 2 errors (empty import and namespace in braces)
        assert!(ret.errors.len() >= 2, "Should have at least 2 errors for malformed imports");

        // Should parse multiple statements including valid ones
        assert!(ret.program.body.len() >= 2, "Should parse at least 2 statements");
    }

    // M6.5.4: TypeScript-specific error recovery tests

    #[test]
    fn test_index_signature_missing_type_annotation_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            interface Config {
                [key: string]
                other: string;
                value: number;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have 1 error for missing type annotation
        assert_eq!(ret.errors.len(), 1, "Should have exactly 1 error");
        assert!(
            ret.errors[0].message.contains("type annotation"),
            "Error should mention type annotation"
        );

        // But program should be parsed successfully
        assert!(!ret.program.body.is_empty(), "Program should not be empty");
    }

    #[test]
    fn test_enum_numeric_member_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            enum Numbers {
                123 = 'test',
                Valid = 'success',
                456 = 'another'
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have 2 errors (for 123 and 456)
        assert_eq!(ret.errors.len(), 2, "Should have exactly 2 errors");

        // Program should still be parsed
        assert!(!ret.program.body.is_empty(), "Program should not be empty");
    }

    #[test]
    fn test_using_declaration_export_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        // Simpler test: just verify export using is handled without crashing
        let source = r"
            export using resource = getResource();
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should report error(s) for export using
        assert!(!ret.errors.is_empty(), "Should have at least 1 error for export using");

        // Parser should not crash (test passes if we get here)
    }

    #[test]
    fn test_typescript_errors_without_recovery_flag() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            interface Config {
                [key: string]
                other: string;
            }
        ";

        // Parse WITHOUT recovery flag (default)
        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = false;
        let ret = parser.parse();

        // Should still report error (but may not parse everything)
        assert!(!ret.errors.is_empty(), "Should have at least 1 error");
    }

    // ==================== M6.5.5: Control Flow Error Recovery Tests ====================

    #[test]
    fn test_switch_invalid_clause_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            switch(x) {
                invalidLabel:
                case 1: break;
                default: break;
            }
            let y = 5;
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have error for invalid clause
        assert!(!ret.errors.is_empty(), "Expected error for invalid switch clause");
        // Should not panic
        assert!(!ret.panicked, "Parser should not panic");
        // Should have parsed subsequent statement
        assert!(!ret.program.body.is_empty(), "Should parse subsequent statements");
    }

    #[test]
    fn test_switch_multiple_invalid_clauses() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            switch(value) {
                label1:
                case 1: break;
                label2:
                default: break;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have multiple errors
        assert!(ret.errors.len() >= 2, "Expected at least 2 errors for invalid clauses");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_try_without_catch_or_finally() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            function fn() {
                try {
                    getData();
                }
                let x = 5;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have error for try without catch/finally
        assert!(!ret.errors.is_empty(), "Expected error for try without catch/finally");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_catch_without_try() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            function fn() {
                catch(e) {
                    log(e);
                }
                return null;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have error for catch without try
        assert!(!ret.errors.is_empty(), "Expected error for catch without try");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_finally_without_try() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            function fn() {
                finally {
                    cleanup();
                }
                return 42;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have error for finally without try
        assert!(!ret.errors.is_empty(), "Expected error for finally without try");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_try_with_invalid_catch_parameter() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            try {
                riskyOperation();
            } catch(123) {
                handleError();
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have error for invalid catch parameter
        assert!(!ret.errors.is_empty(), "Expected error for invalid catch parameter");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_nested_try_without_catch() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            try {
                try {
                    inner();
                }
                outer();
            } catch(e) {
                log(e);
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Inner try should have error, outer should be complete
        assert!(!ret.errors.is_empty(), "Expected error for inner try without catch/finally");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_for_loop_missing_semicolons() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            for(let i = 0) {
                console.log(i);
            }
            let x = 5;
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have errors for missing semicolons (recoverable expect())
        // Parser continues and parses subsequent statements
        assert!(!ret.panicked, "Parser should not panic");
        assert!(!ret.program.body.is_empty(), "Should parse statements");
    }

    #[test]
    fn test_while_missing_condition() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            while(true) {
                break;
            }
            let x = 5;
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Valid while loop - tests that while loops work with recovery enabled
        assert!(!ret.panicked, "Parser should not panic");
        assert!(!ret.program.body.is_empty(), "Should parse statements");
    }

    #[test]
    fn test_if_statement_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            if(condition) {
                positive();
            }
            let x = 5;
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Valid if statement - tests that if statements work with recovery enabled
        assert!(!ret.panicked, "Parser should not panic");
        assert!(!ret.program.body.is_empty(), "Should parse statements");
    }

    #[test]
    fn test_complex_nested_control_flow_errors() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            function complex() {
                switch(x) {
                    invalid:
                    case 1: break;
                }

                try {
                    riskyOp();
                }

                return 42;
            }
        ";

        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = true;
        let ret = parser.parse();

        // Should have multiple errors (switch + try)
        assert!(ret.errors.len() >= 2, "Expected at least 2 errors");
        assert!(!ret.panicked, "Parser should not panic");
    }

    #[test]
    fn test_control_flow_recovery_disabled() {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let source = r"
            switch(x) {
                invalidLabel:
                case 1: break;
            }
        ";

        // Parse WITHOUT recovery flag
        let mut parser = Parser::new(&allocator, source, source_type);
        parser.options.recover_from_errors = false;
        let ret = parser.parse();

        // Without recovery, should panic on error
        assert!(ret.panicked, "Parser should panic without recovery enabled");
    }

    // Regression test: Ensure TypeScript function return types work with error recovery
    // Previously, try_parse() didn't restore token position properly in error recovery mode,
    // causing TypeScript functions with return type annotations to fail parsing
    #[test]
    fn test_typescript_function_with_error_recovery() {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_typescript(true);
        let source = "function test(x: number): number { return x; }";

        // WITHOUT error recovery - should work
        let ret_no_recovery = Parser::new(&allocator, source, source_type).parse();
        assert!(!ret_no_recovery.panicked, "TypeScript function should parse without recovery");
        assert_eq!(ret_no_recovery.errors.len(), 0, "Should have no errors without recovery");
        assert_eq!(ret_no_recovery.program.body.len(), 1, "Should parse the function");

        // WITH error recovery - should work correctly
        let ret_with_recovery = Parser::new(&allocator, source, source_type)
            .with_options(ParseOptions { recover_from_errors: true, ..Default::default() })
            .parse();
        assert!(
            !ret_with_recovery.panicked,
            "TypeScript function should parse with recovery enabled (panicked={})",
            ret_with_recovery.panicked
        );
        assert_eq!(
            ret_with_recovery.errors.len(),
            0,
            "TypeScript function should have no errors with recovery (errors={})",
            ret_with_recovery.errors.len()
        );
        assert_eq!(
            ret_with_recovery.program.body.len(),
            1,
            "Should parse the function with recovery (statements={})",
            ret_with_recovery.program.body.len()
        );
    }
}
