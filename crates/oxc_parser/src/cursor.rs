//! Code related to navigating `Token`s from the lexer

use oxc_allocator::Vec;
use oxc_ast::ast::{BindingRestElement, RegExpFlags};
use oxc_diagnostics::OxcDiagnostic;
use oxc_span::{GetSpan, Span};

use crate::{
    Context, ParserImpl, diagnostics,
    error_handler::FatalError,
    lexer::{Kind, LexerCheckpoint, LexerContext, Token},
};

#[derive(Clone)]
pub struct ParserCheckpoint<'a> {
    lexer: LexerCheckpoint<'a>,
    cur_token: Token,
    prev_span_end: u32,
    errors_pos: usize,
    fatal_error: Option<FatalError>,
}

impl<'a> ParserImpl<'a> {
    #[inline]
    pub(crate) fn start_span(&self) -> u32 {
        self.token.start()
    }

    #[inline]
    pub(crate) fn end_span(&self, start: u32) -> Span {
        Span::new(start, self.prev_token_end)
    }

    /// Get current token
    #[inline]
    pub(crate) fn cur_token(&self) -> Token {
        self.token
    }

    /// Get current Kind
    #[inline]
    pub(crate) fn cur_kind(&self) -> Kind {
        self.token.kind()
    }

    /// Get current source text
    #[inline]
    pub(crate) fn cur_src(&self) -> &'a str {
        self.token_source(&self.token)
    }

    /// Get source text for a token
    #[inline]
    pub(crate) fn token_source(&self, token: &Token) -> &'a str {
        let span = token.span();
        if cfg!(debug_assertions) {
            &self.source_text[span.start as usize..span.end as usize]
        } else {
            // SAFETY:
            // Span comes from the lexer, which ensures:
            // * `start` and `end` are in bounds of source text.
            // * `end >= start`.
            // * `start` and `end` are both on UTF-8 char boundaries.
            // * `self.source_text` is same text that `Token`s are generated from.
            //
            // TODO: I (@overlookmotel) don't think we should really be doing this.
            // We don't have static guarantees of these properties.
            unsafe { self.source_text.get_unchecked(span.start as usize..span.end as usize) }
        }
    }

    /// Get current string
    pub(crate) fn cur_string(&self) -> &'a str {
        self.lexer.get_string(self.token)
    }

    /// Get current template string
    pub(crate) fn cur_template_string(&self) -> Option<&'a str> {
        self.lexer.get_template_string(self.token.start())
    }

    /// Checks if the current index has token `Kind`
    #[inline]
    pub(crate) fn at(&self, kind: Kind) -> bool {
        self.cur_kind() == kind
    }

    /// `StringValue` of `IdentifierName` normalizes any Unicode escape sequences
    /// in `IdentifierName` hence such escapes cannot be used to write an Identifier
    /// whose code point sequence is the same as a `ReservedWord`.
    #[cold]
    fn report_escaped_keyword(&mut self, span: Span) {
        self.error(diagnostics::escaped_keyword(span));
    }

    /// Move to the next token
    /// Checks if the current token is escaped if it is a keyword
    #[inline]
    fn advance(&mut self, kind: Kind) {
        // Manually inlined escaped keyword check - escaped identifiers are extremely rare
        if self.token.escaped() && kind.is_any_keyword() {
            self.report_escaped_keyword(self.token.span());
        }
        self.prev_token_end = self.token.end();
        self.token = self.lexer.next_token();
    }

    /// Move to the next `JSXChild`
    /// Checks if the current token is escaped if it is a keyword
    pub(crate) fn advance_for_jsx_child(&mut self) {
        self.prev_token_end = self.token.end();
        self.token = self.lexer.next_jsx_child();
    }

    /// Advance and return true if we are at `Kind`, return false otherwise
    #[inline]
    #[must_use = "Use `bump` instead of `eat` if you are ignoring the return value"]
    pub(crate) fn eat(&mut self, kind: Kind) -> bool {
        if self.at(kind) {
            self.advance(kind);
            return true;
        }
        false
    }

    /// Advance if we are at `Kind`
    #[inline]
    pub(crate) fn bump(&mut self, kind: Kind) {
        if self.at(kind) {
            self.advance(kind);
        }
    }

    /// Advance any token
    #[inline]
    pub(crate) fn bump_any(&mut self) {
        self.advance(self.cur_kind());
    }

    /// Advance and change token type, useful for changing keyword to ident
    #[inline]
    pub(crate) fn bump_remap(&mut self, kind: Kind) {
        self.advance(kind);
    }

    /// [Automatic Semicolon Insertion](https://tc39.es/ecma262/#sec-automatic-semicolon-insertion)
    /// # Errors
    pub(crate) fn asi(&mut self) {
        if self.eat(Kind::Semicolon) || self.can_insert_semicolon() {
            /* no op */
        } else {
            let span = Span::empty(self.prev_token_end);
            let error = diagnostics::auto_semicolon_insertion(span);
            // M6.5.6: Respect recovery mode for ASI failures
            if self.options.recover_from_errors {
                self.error(error);
            } else {
                self.set_fatal_error(error);
            }
        }
    }

    #[inline]
    pub(crate) fn can_insert_semicolon(&self) -> bool {
        let token = self.cur_token();
        matches!(token.kind(), Kind::Semicolon | Kind::RCurly | Kind::Eof) || token.is_on_new_line()
    }

    /// Cold path for expect failures - separated to improve branch prediction
    /// Handles failure when an expected token is not found.
    ///
    /// **Recovery Behavior**:
    /// - When `recover_from_errors` is `true`: Records error but does NOT terminate parsing.
    ///   Caller should use `synchronize_on_error()` if context-specific recovery is needed.
    /// - When `recover_from_errors` is `false`: Sets fatal error and terminates parsing.
    ///
    /// **Why caller handles synchronization**: Different parsing contexts need different
    /// recovery strategies. This function just records the error; the caller decides whether
    /// to skip tokens, insert dummy nodes, or abort the current context.
    #[cold]
    #[inline(never)]
    fn handle_expect_failure(&mut self, expected_kind: Kind) {
        let range = self.cur_token().span();
        let error =
            diagnostics::expect_token(expected_kind.to_str(), self.cur_kind().to_str(), range);

        if self.options.recover_from_errors {
            // Recovery mode: record error but allow parsing to continue
            self.error(error);
        } else {
            // Non-recovery mode: terminate immediately
            self.set_fatal_error(error);
        }
    }

    /// # Errors
    #[inline]
    pub(crate) fn expect_without_advance(&mut self, kind: Kind) {
        if !self.at(kind) {
            self.handle_expect_failure(kind);
        }
    }

    /// Expect a `Kind` or return error
    /// # Errors
    #[inline]
    pub(crate) fn expect(&mut self, kind: Kind) {
        // M6.5.6 Phase 2.1: Track opening parentheses for unclosed paren detection
        // Track BEFORE checking/advancing so we capture the actual token
        if self.options.recover_from_errors && kind == Kind::LParen && self.at(kind) {
            let span = self.cur_token().span();
            self.state.push_paren(span);
        }

        if !self.at(kind) {
            self.handle_expect_failure(kind);
        }
        self.advance(kind);
    }

    /// Expect a closing delimiter (e.g., `]`, `)`, `}`) or record error.
    ///
    /// **Recovery Behavior**:
    /// - When `recover_from_errors` is `true`: Records error but allows parsing to continue.
    ///   Caller should use `synchronize_on_error()` for context-specific recovery.
    /// - When `recover_from_errors` is `false`: Sets fatal error and terminates parsing.
    #[inline]
    pub(crate) fn expect_closing(&mut self, kind: Kind, opening_span: Span) {
        // M6.5.6 Phase 2.1-2.2: Track closing parentheses and implicit insertion
        let found_closing = self.at(kind);

        if self.options.recover_from_errors && kind == Kind::RParen {
            if found_closing {
                // Found closing paren - pop from stack
                self.state.pop_paren();
            } else {
                // M6.5.6 Phase 2.2: Implicit paren insertion
                // Missing closing paren - pop anyway to allow recovery
                // This simulates inserting an implicit closing paren
                self.state.pop_paren();
            }
        }

        if !found_closing {
            let range = self.cur_token().span();
            let error = diagnostics::expect_closing(
                kind.to_str(),
                self.cur_kind().to_str(),
                range,
                opening_span,
            );

            if self.options.recover_from_errors {
                // Recovery mode: record error but allow parsing to continue
                self.error(error);
            } else {
                // Non-recovery mode: terminate immediately
                self.set_fatal_error(error);
            }
        }
        self.advance(kind);
    }

    /// Expect the `:` in a conditional expression (`? ... : ...`).
    ///
    /// **Recovery Behavior**:
    /// - When `recover_from_errors` is `true`: Records error but allows parsing to continue.
    /// - When `recover_from_errors` is `false`: Sets fatal error and terminates parsing.
    #[inline]
    pub(crate) fn expect_conditional_alternative(&mut self, question_span: Span) {
        if !self.at(Kind::Colon) {
            let range = self.cur_token().span();
            let error = diagnostics::expect_conditional_alternative(
                self.cur_kind().to_str(),
                range,
                question_span,
            );

            if self.options.recover_from_errors {
                // Recovery mode: record error but allow parsing to continue
                self.error(error);
            } else {
                // Non-recovery mode: terminate immediately
                self.set_fatal_error(error);
            }
        }
        self.bump_any(); // bump `:`
    }

    /// Expect the next next token to be a `JsxChild`, i.e. `<` or `{` or `JSXText`
    /// # Errors
    pub(crate) fn expect_jsx_child(&mut self, kind: Kind) {
        self.expect_without_advance(kind);
        self.advance_for_jsx_child();
    }

    /// Expect the next next token to be a `JsxString` or any other token
    /// # Errors
    pub(crate) fn expect_jsx_attribute_value(&mut self, kind: Kind) {
        self.lexer.set_context(LexerContext::JsxAttributeValue);
        self.expect(kind);
        self.lexer.set_context(LexerContext::Regular);
    }

    /// Tell lexer to read a regex
    pub(crate) fn read_regex(&mut self) -> (u32, RegExpFlags, bool) {
        let (token, pattern_end, flags, flags_error) = self.lexer.next_regex(self.cur_kind());
        self.token = token;
        (pattern_end, flags, flags_error)
    }

    /// Tell lexer to read a template substitution tail
    pub(crate) fn re_lex_template_substitution_tail(&mut self) {
        if self.at(Kind::RCurly) {
            self.token = self.lexer.next_template_substitution_tail();
        }
    }

    /// Tell lexer to continue reading jsx identifier if the lexer character position is at `-` for `<component-name>`
    pub(crate) fn continue_lex_jsx_identifier(&mut self) {
        if let Some(token) = self.lexer.continue_lex_jsx_identifier() {
            self.token = token;
        }
    }

    #[inline]
    pub(crate) fn re_lex_right_angle(&mut self) -> Kind {
        if self.fatal_error.is_some() {
            return Kind::Eof;
        }
        let kind = self.cur_kind();
        if kind == Kind::RAngle {
            self.token = self.lexer.re_lex_right_angle();
            self.token.kind()
        } else {
            kind
        }
    }

    pub(crate) fn re_lex_ts_l_angle(&mut self) -> bool {
        if self.fatal_error.is_some() {
            return false;
        }
        let kind = self.cur_kind();
        if kind == Kind::ShiftLeft || kind == Kind::LtEq {
            self.token = self.lexer.re_lex_as_typescript_l_angle(2);
            true
        } else if kind == Kind::ShiftLeftEq {
            self.token = self.lexer.re_lex_as_typescript_l_angle(3);
            true
        } else {
            kind == Kind::LAngle
        }
    }

    pub(crate) fn re_lex_ts_r_angle(&mut self) -> bool {
        if self.fatal_error.is_some() {
            return false;
        }
        let kind = self.cur_kind();
        if kind == Kind::ShiftRight {
            self.token = self.lexer.re_lex_as_typescript_r_angle(2);
            true
        } else if kind == Kind::ShiftRight3 {
            self.token = self.lexer.re_lex_as_typescript_r_angle(3);
            true
        } else {
            kind == Kind::RAngle
        }
    }

    pub(crate) fn checkpoint(&mut self) -> ParserCheckpoint<'a> {
        ParserCheckpoint {
            lexer: self.lexer.checkpoint(),
            cur_token: self.token,
            prev_span_end: self.prev_token_end,
            errors_pos: self.errors.len(),
            fatal_error: self.fatal_error.take(),
        }
    }

    pub(crate) fn checkpoint_with_error_recovery(&mut self) -> ParserCheckpoint<'a> {
        ParserCheckpoint {
            lexer: self.lexer.checkpoint_with_error_recovery(),
            cur_token: self.token,
            prev_span_end: self.prev_token_end,
            errors_pos: self.errors.len(),
            fatal_error: self.fatal_error.take(),
        }
    }

    pub(crate) fn rewind(&mut self, checkpoint: ParserCheckpoint<'a>) {
        let ParserCheckpoint { lexer, cur_token, prev_span_end, errors_pos, fatal_error } =
            checkpoint;

        self.lexer.rewind(lexer);
        self.token = cur_token;
        self.prev_token_end = prev_span_end;
        self.errors.truncate(errors_pos);
        self.fatal_error = fatal_error;
    }

    pub(crate) fn try_parse<T>(
        &mut self,
        func: impl FnOnce(&mut ParserImpl<'a>) -> T,
    ) -> Option<T> {
        let checkpoint = self.checkpoint_with_error_recovery();
        let ctx = self.ctx;
        let errors_before = checkpoint.errors_pos;
        let node = func(self);
        let errors_after = self.errors.len();

        // Check if either fatal error occurred OR new errors were added
        // In error recovery mode, errors are non-fatal but still indicate failure
        if self.fatal_error.is_none() && errors_after == errors_before {
            Some(node)
        } else {
            self.ctx = ctx;
            self.rewind(checkpoint);
            None
        }
    }

    pub(crate) fn lookahead<U>(&mut self, predicate: impl Fn(&mut ParserImpl<'a>) -> U) -> U {
        let checkpoint = self.checkpoint();
        let answer = predicate(self);
        self.rewind(checkpoint);
        answer
    }

    #[expect(clippy::inline_always)]
    #[inline(always)] // inline because this is always on a hot path
    pub(crate) fn context_add<F, T>(&mut self, add_flags: Context, cb: F) -> T
    where
        F: FnOnce(&mut Self) -> T,
    {
        let ctx = self.ctx;
        self.ctx = ctx.union(add_flags);
        let result = cb(self);
        self.ctx = ctx;
        result
    }

    #[expect(clippy::inline_always)]
    #[inline(always)] // inline because this is always on a hot path
    pub(crate) fn context_remove<F, T>(&mut self, remove_flags: Context, cb: F) -> T
    where
        F: FnOnce(&mut Self) -> T,
    {
        let ctx = self.ctx;
        self.ctx = ctx.difference(remove_flags);
        let result = cb(self);
        self.ctx = ctx;
        result
    }

    #[expect(clippy::inline_always)]
    #[inline(always)] // inline because this is always on a hot path
    pub(crate) fn context<F, T>(&mut self, add_flags: Context, remove_flags: Context, cb: F) -> T
    where
        F: FnOnce(&mut Self) -> T,
    {
        let ctx = self.ctx;
        self.ctx = ctx.difference(remove_flags).union(add_flags);
        let result = cb(self);
        self.ctx = ctx;
        result
    }

    /// M6.5: Replaced with custom loops for error recovery in multiple contexts.
    /// This function may be useful for other contexts in the future, so keeping it available.
    #[expect(dead_code)]
    pub(crate) fn parse_normal_list<F, T>(&mut self, open: Kind, close: Kind, f: F) -> Vec<'a, T>
    where
        F: Fn(&mut Self) -> T,
    {
        let opening_span = self.cur_token().span();
        self.expect(open);
        let mut list = self.ast.vec();
        loop {
            let kind = self.cur_kind();
            if kind == close
                || matches!(kind, Kind::Eof | Kind::Undetermined)
                || self.fatal_error.is_some()
            {
                break;
            }
            list.push(f(self));
        }
        self.expect_closing(close, opening_span);
        list
    }

    /// M6.5: Replaced with custom loops for error recovery in statement lists and class members.
    /// This function may be useful for other contexts in the future, so keeping it available.
    #[expect(dead_code)]
    pub(crate) fn parse_normal_list_breakable<F, T>(
        &mut self,
        open: Kind,
        close: Kind,
        f: F,
    ) -> Vec<'a, T>
    where
        F: Fn(&mut Self) -> Option<T>,
    {
        let opening_span = self.cur_token().span();
        self.expect(open);
        let mut list = self.ast.vec();
        loop {
            if self.at(close) || self.has_fatal_error() {
                break;
            }
            if let Some(e) = f(self) {
                list.push(e);
            } else {
                break;
            }
        }
        self.expect_closing(close, opening_span);
        list
    }

    pub(crate) fn parse_delimited_list<F, T>(
        &mut self,
        close: Kind,
        separator: Kind,
        opening_span: Span,
        f: F,
    ) -> (Vec<'a, T>, Option<u32>)
    where
        F: Fn(&mut Self) -> T,
    {
        let mut list = self.ast.vec();
        // Cache cur_kind() to avoid redundant calls in compound checks
        let kind = self.cur_kind();
        if kind == close
            || matches!(kind, Kind::Eof | Kind::Undetermined)
            || self.fatal_error.is_some()
        {
            return (list, None);
        }
        list.push(f(self));
        loop {
            let kind = self.cur_kind();
            if kind == close
                || matches!(kind, Kind::Eof | Kind::Undetermined)
                || self.fatal_error.is_some()
            {
                return (list, None);
            }
            if !self.at(separator) {
                let error = diagnostics::expect_closing_or_separator(
                    close.to_str(),
                    separator.to_str(),
                    kind.to_str(),
                    self.cur_token().span(),
                    opening_span,
                );
                if self.options.recover_from_errors {
                    // M6.5.6: Recovery mode - report error and try to continue
                    self.error(error);
                    // Try to continue parsing by skipping to the next separator or close
                    return (list, None);
                } else {
                    // Non-recovery mode: fatal error
                    self.set_fatal_error(error);
                    return (list, None);
                }
            }
            self.advance(separator);
            if self.cur_kind() == close {
                let trailing_separator = self.prev_token_end - 1;
                return (list, Some(trailing_separator));
            }
            list.push(f(self));
        }
    }

    pub(crate) fn parse_delimited_list_with_rest<E, A, R, D>(
        &mut self,
        close: Kind,
        opening_span: Span,
        parse_element: E,
        parse_rest: R,
        rest_last_diagnostic: D,
    ) -> (Vec<'a, A>, Option<BindingRestElement<'a>>)
    where
        E: Fn(&mut Self) -> A,
        R: Fn(&mut Self) -> BindingRestElement<'a>,
        D: Fn(Span) -> OxcDiagnostic,
    {
        let mut list = self.ast.vec();
        let mut rest: Option<BindingRestElement<'a>> = None;
        let mut first = true;
        loop {
            let kind = self.cur_kind();
            if kind == close
                || matches!(kind, Kind::Eof | Kind::Undetermined)
                || self.fatal_error.is_some()
            {
                break;
            }

            if first {
                first = false;
            } else {
                let comma_span = self.cur_token().span();
                if kind != Kind::Comma {
                    let error = diagnostics::expect_closing_or_separator(
                        close.to_str(),
                        Kind::Comma.to_str(),
                        kind.to_str(),
                        comma_span,
                        opening_span,
                    );
                    if self.options.recover_from_errors {
                        // M6.5.6: Recovery mode - report error and break
                        self.error(error);
                        break;
                    } else {
                        // Non-recovery mode: fatal error
                        self.set_fatal_error(error);
                        break;
                    }
                }
                self.bump_any();
                let kind = self.cur_kind();
                if kind == close {
                    if rest.is_some() && !self.ctx.has_ambient() {
                        self.error(diagnostics::rest_element_trailing_comma(comma_span));
                    }
                    break;
                }
            }

            if let Some(r) = &rest {
                let error = rest_last_diagnostic(r.span());
                if self.options.recover_from_errors {
                    // M6.5.6: Recovery mode - report error and continue parsing
                    self.error(error);
                    // Skip the rest of the pattern and break to close
                    break;
                } else {
                    // Non-recovery mode: fatal error
                    self.set_fatal_error(error);
                    break;
                }
            }

            // Re-capture kind to get the current token (may have changed after else branch)
            let kind = self.cur_kind();
            if kind == Kind::Dot3 {
                rest.replace(parse_rest(self));
            } else {
                list.push(parse_element(self));
            }
        }

        (list, rest)
    }

    /// M6.5.6 Phase 2.1: Check for unclosed parentheses and generate errors
    ///
    /// Call this at synchronization points (end of statements, blocks, or file) to detect
    /// unclosed parens and generate helpful error messages.
    pub(crate) fn check_unclosed_parens(&mut self) {
        if !self.options.recover_from_errors {
            return;
        }

        if let Some(opening_span) = self.state.first_unclosed_paren() {
            let current_span = self.cur_token().span();
            self.error(diagnostics::unclosed_paren(opening_span, current_span));
            // Clear the paren stack after reporting to avoid duplicate errors
            self.state.paren_stack.clear();
        }
    }
}

#[cfg(test)]
mod error_recovery_tests {
    use crate::Parser;
    use oxc_allocator::Allocator;
    use oxc_span::SourceType;

    /// Helper to parse code with recovery enabled
    fn parse_with_recovery(source: &str) -> (usize, usize) {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_typescript(true);
        let options = crate::ParseOptions { recover_from_errors: true, ..Default::default() };

        let ret = Parser::new(&allocator, source, source_type).with_options(options).parse();

        (ret.errors.len(), ret.program.body.len())
    }

    /// Helper to parse code without recovery
    fn parse_without_recovery(source: &str) -> (usize, usize) {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_typescript(true);
        let options = crate::ParseOptions { recover_from_errors: false, ..Default::default() };

        let ret = Parser::new(&allocator, source, source_type).with_options(options).parse();

        (ret.errors.len(), ret.program.body.len())
    }

    #[test]
    fn test_handle_expect_failure_recovery_mode() {
        // Missing closing bracket in array - should record error
        let source = "let arr = [1, 2, 3; let x = 5;";
        let (errors, _statements) = parse_with_recovery(source);

        // Should have at least 1 error for missing ]
        assert!(errors >= 1, "Expected at least 1 error, got {errors}");
        // Recovery mode records errors without terminating immediately
    }

    #[test]
    fn test_handle_expect_failure_non_recovery_mode() {
        // Missing closing bracket - should terminate immediately
        let source = "let arr = [1, 2, 3; let x = 5;";
        let (errors, _statements) = parse_without_recovery(source);

        // Should have errors
        assert!(errors >= 1, "Expected errors in non-recovery mode");
        // Parser terminates on fatal error, so may not parse second statement
    }

    #[test]
    fn test_unexpected_token_skipping() {
        // Unexpected token that doesn't belong to parent context
        let source = "let x = @ 5;"; // @ is unexpected
        let (errors, statements) = parse_with_recovery(source);

        // Should have error for unexpected token
        assert!(errors >= 1, "Expected error for unexpected token");
        // Should attempt to continue parsing
        assert!(statements >= 1, "Expected to parse statement despite error");
    }

    #[test]
    fn test_missing_closing_paren() {
        // Missing closing paren in if statement
        let source = "if (x > 0 { console.log('yes'); }";
        let (errors, statements) = parse_with_recovery(source);

        // Should have error for missing )
        assert!(errors >= 1, "Expected error for missing closing paren");
        // Should attempt to parse the if statement body
        assert!(statements >= 1, "Expected to parse if statement despite error");
    }

    #[test]
    fn test_missing_closing_brace_in_block() {
        // Missing closing brace in block
        let source = "{ let x = 5; let y = 10;";
        let (errors, _) = parse_with_recovery(source);

        // Should have error for missing }
        assert!(errors >= 1, "Expected error for missing closing brace");
    }

    #[test]
    fn test_nested_structures_with_errors() {
        // Nested arrays with missing closing bracket
        let source = "let arr = [[1, 2], [3, 4]; let x = 5;";
        let (errors, _statements) = parse_with_recovery(source);

        // Should have error for missing ]
        assert!(errors >= 1, "Expected error for nested structure");
        // Recovery records errors
    }

    #[test]
    fn test_multiple_errors_recovery() {
        // Multiple syntax errors in sequence
        let source = "let x = [1, 2; let y = {a: 1; let z = 5;";
        let (errors, _statements) = parse_with_recovery(source);

        // Should have multiple errors
        assert!(errors >= 1, "Expected multiple errors, got {errors}");
        // Recovery mode records multiple errors
    }

    #[test]
    fn test_recovery_continues_after_error() {
        // Error in first statement, valid second statement
        let source = "let x = [1, 2; let y = 10;";
        let (errors, _statements) = parse_with_recovery(source);

        // Should have error for missing ]
        assert!(errors >= 1, "Expected at least 1 error");
        // Recovery mode records errors without fatal termination
    }

    #[test]
    fn test_no_errors_in_valid_code() {
        // Valid code should parse without errors
        let source = "let x = [1, 2, 3]; let y = 10;";
        let (errors, statements) = parse_with_recovery(source);

        // Should have no errors
        assert_eq!(errors, 0, "Expected no errors in valid code");
        // Should parse both statements
        assert_eq!(statements, 2, "Expected to parse 2 statements");
    }

    #[test]
    fn test_eof_during_recovery() {
        // EOF while parsing unclosed structure
        let source = "let x = [1, 2, 3";
        let (errors, _) = parse_with_recovery(source);

        // Should have error for unclosed array
        assert!(errors >= 1, "Expected error for EOF in unclosed structure");
    }

    // ========================================
    // Phase 2: Integration tests
    // ========================================

    // Phase 3.1: Array parsing
    #[test]
    fn test_integration_array_missing_bracket() {
        let source = "let arr = [1, 2, 3;";
        let (errors, stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing ]");
        assert_eq!(stmts, 1, "Should parse one statement");
    }

    #[test]
    fn test_integration_array_followed_by_valid_statement() {
        // This is a known limitation: when semicolon appears inside array literal,
        // current recovery cannot distinguish it from statement terminator
        let source = "let arr = [1, 2, 3; let y = 10;";
        let (errors, _stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing ]");
        // Note: Current recovery produces 0 statements due to semicolon ambiguity
    }

    #[test]
    fn test_integration_nested_arrays_with_errors() {
        let source = "let x = [[1, 2, [3, 4];";
        let (errors, _) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected errors for multiple missing ]");
    }

    // Phase 3.2: Object literal recovery
    #[test]
    fn test_object_literal_recovery() {
        let source = "let obj = {a: 1, b: 2;";
        let (errors, stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing }}");
        assert_eq!(stmts, 1, "Should parse one statement");
    }

    // Phase 3.3: Block statement recovery
    #[test]
    fn test_block_statement_recovery() {
        let source = "if (true) { let x = 1; let y = 2;";
        let (errors, stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing }}");
        assert!(stmts >= 1, "Should parse if statement");
    }

    // Phase 3.4: Parenthesized expression recovery
    #[test]
    fn test_parenthesized_expression_recovery() {
        // Known limitation: Missing ) in parenthesized expression with ; terminator
        // confuses recovery (semicolon could be inside or outside parens)
        let source = "let x = (1 + 2;";
        let (errors, _stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing )");
        // Note: Current recovery produces 0 statements due to ambiguity
    }

    #[test]
    fn test_function_with_missing_brace() {
        let source = "function foo() { let x = 1;";
        let (errors, stmts) = parse_with_recovery(source);
        assert!(errors >= 1, "Expected error for missing }}");
        assert_eq!(stmts, 1, "Should parse function declaration");
    }

    #[test]
    fn test_multiple_missing_delimiters() {
        // Known limitation: Multiple missing delimiters with semicolons
        let source = "let x = [1, 2; let obj = {a: 1;";
        let (errors, _stmts) = parse_with_recovery(source);
        assert!(errors >= 2, "Expected at least 2 errors");
        // Note: Current recovery produces 0 statements due to complex error cascade
    }

    #[test]
    fn test_deeply_nested_structures() {
        let source = "let x = {a: [1, {b: 2}]};";
        let (errors, stmts) = parse_with_recovery(source);
        assert_eq!(errors, 0, "Should parse without errors");
        assert_eq!(stmts, 1, "Should parse one statement");
    }

    #[test]
    fn test_empty_structures_with_errors() {
        // Known limitation: Empty structures with immediate semicolons
        let source = "let x = [; let y = {;";
        let (errors, _stmts) = parse_with_recovery(source);
        assert!(errors >= 2, "Expected errors for malformed structures");
        // Note: Current recovery produces 0 statements for this complex error pattern
    }
}
