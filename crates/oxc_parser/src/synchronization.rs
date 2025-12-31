//! Error recovery synchronization helpers for TSC-style error recovery.
//!
//! This module implements synchronization logic that enables the parser to recover
//! from syntax errors intelligently by maintaining a context stack and making
//! skip-or-abort decisions based on the current parsing context.
//!
//! All functions in this module respect the `recover_from_errors` flag and only
//! execute when error recovery is enabled.

use crate::{ParserImpl, context::ParsingContext, lexer::Kind};

/// Decision returned by error recovery synchronization.
///
/// Determines whether the parser should skip the current token and continue
/// parsing within the current context, or abort the current context and
/// return to the parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryDecision {
    /// Skip the current token and try parsing the next element.
    ///
    /// Used when the current token is meaningless in all contexts,
    /// so we can safely skip it and continue parsing.
    Skip,

    /// Abort parsing the current context and return to the parent.
    ///
    /// Used when the current token is a terminator or is meaningful
    /// in a parent context, indicating we should exit this context.
    Abort,
}

#[expect(dead_code, reason = "M6.5: Will be used in parsing loops shortly")]
impl ParserImpl<'_> {
    /// Checks if the current token terminates the given parsing context.
    ///
    /// Returns `true` if the current token marks the end of the context,
    /// indicating that parsing should exit this context.
    ///
    /// **Note**: This function is only meaningful when `recover_from_errors` is enabled.
    /// When recovery is disabled, it returns `false`.
    #[inline]
    pub(crate) fn is_context_terminator(&self, ctx: ParsingContext) -> bool {
        // Early return if recovery is disabled
        if !self.options.recover_from_errors {
            return false;
        }

        match ctx {
            ParsingContext::TopLevel => self.at(Kind::Eof),
            ParsingContext::BlockStatements | ParsingContext::FunctionBody => {
                self.at(Kind::RCurly) || self.at(Kind::Eof)
            }
            ParsingContext::Parameters => {
                self.at(Kind::RParen)
                    || self.at(Kind::LCurly) // function body start (recovery aid)
                    || self.at(Kind::Extends) // class extends (recovery aid)
                    || self.at(Kind::Implements) // class implements (recovery aid)
                    || self.at(Kind::Eof)
            }
            ParsingContext::ArgumentExpressions => {
                self.at(Kind::RParen)
                    || self.at(Kind::Semicolon) // statement boundary (recovery aid)
                    || self.at(Kind::Eof)
            }
            ParsingContext::ClassMembers
            | ParsingContext::TypeMembers
            | ParsingContext::EnumMembers
            | ParsingContext::ObjectLiteralMembers => self.at(Kind::RCurly) || self.at(Kind::Eof),
            ParsingContext::ArrayLiteralMembers => self.at(Kind::RBrack) || self.at(Kind::Eof),
            ParsingContext::SwitchClauses => {
                self.at(Kind::RCurly)
                    || self.at(Kind::Case) // allows recovery within switch
                    || self.at(Kind::Default) // allows recovery within switch
                    || self.at(Kind::Eof)
            }
            ParsingContext::ImportSpecifiers | ParsingContext::ExportSpecifiers => {
                self.at(Kind::RCurly)
                    || self.at(Kind::From) // import/export source (recovery aid)
                    || self.at(Kind::Semicolon) // statement end (recovery aid)
                    || self.at(Kind::Eof)
            }
            ParsingContext::TypeParameters => {
                self.at(Kind::RAngle)
                    || self.at(Kind::LCurly) // body start (recovery aid)
                    || self.at(Kind::Extends) // extends clause (recovery aid)
                    || self.at(Kind::Eof)
            }
            ParsingContext::TypeArguments => {
                self.at(Kind::RAngle)
                    || self.at(Kind::RParen) // call expression end (recovery aid)
                    || self.at(Kind::LCurly) // body start (recovery aid)
                    || self.at(Kind::Eof)
            }
            ParsingContext::TypeAnnotation => {
                self.at(Kind::Eq) // assignment
                    || self.at(Kind::Semicolon) // statement end
                    || self.at(Kind::Comma) // next parameter/property
                    || self.at(Kind::RParen) // parameter list end
                    || self.at(Kind::RCurly) // block end
                    || self.at(Kind::Eof)
            }
            ParsingContext::JsxAttributes => {
                self.at(Kind::RAngle) // >
                    || self.at(Kind::Slash) // />
                    || self.at(Kind::Eof)
            }
            ParsingContext::JsxChildren => {
                self.at(Kind::LAngle) // closing tag start
                    || self.at(Kind::Eof)
            }
        }
    }

    /// Returns true if the current token can start a statement.
    #[inline]
    fn is_start_of_statement_recovery(&self) -> bool {
        matches!(
            self.cur_kind(),
            Kind::Let
                | Kind::Const
                | Kind::Var
                | Kind::Function
                | Kind::Class
                | Kind::If
                | Kind::For
                | Kind::While
                | Kind::Do
                | Kind::Switch
                | Kind::Return
                | Kind::Break
                | Kind::Continue
                | Kind::Throw
                | Kind::Try
                | Kind::LCurly
                | Kind::At
        ) || self.is_start_of_expression_recovery()
    }

    /// Returns true if the current token can start an expression.
    #[inline]
    fn is_start_of_expression_recovery(&self) -> bool {
        matches!(
            self.cur_kind(),
            Kind::This
                | Kind::Super
                | Kind::Null
                | Kind::True
                | Kind::False
                | Kind::Str
                | Kind::TemplateHead
                | Kind::TemplateTail
                | Kind::Decimal
                | Kind::Binary
                | Kind::Octal
                | Kind::Hex
                | Kind::BigInt
                | Kind::LParen
                | Kind::LBrack
                | Kind::LCurly
                | Kind::Function
                | Kind::Class
                | Kind::New
                | Kind::Slash
                | Kind::Plus
                | Kind::Minus
                | Kind::Bang
                | Kind::Tilde
                | Kind::Plus2
                | Kind::Minus2
                | Kind::Typeof
                | Kind::Void
                | Kind::Delete
                | Kind::Await
                | Kind::LAngle
        ) || self.cur_kind().is_identifier_or_keyword()
    }

    /// Returns true if current token can start a class member.
    #[inline]
    fn is_start_of_class_member(&self) -> bool {
        matches!(
            self.cur_kind(),
            Kind::Public
                | Kind::Private
                | Kind::Protected
                | Kind::Static
                | Kind::Readonly
                | Kind::Async
                | Kind::Get
                | Kind::Set
                | Kind::Star
                | Kind::LBrack
                | Kind::At
                | Kind::Accessor
        ) || self.cur_kind().is_identifier_or_keyword()
    }

    /// Checks if the current token could start an element in the given context.
    ///
    /// The `in_error_recovery` parameter changes behavior:
    /// - `false` (normal mode): More permissive, allows semicolons in some contexts
    /// - `true` (recovery mode): More strict, excludes ambiguous tokens like semicolons
    ///
    /// **Note**: Returns `false` when `recover_from_errors` is disabled.
    #[inline]
    pub(crate) fn is_context_element_start(
        &self,
        ctx: ParsingContext,
        in_error_recovery: bool,
    ) -> bool {
        // Early return if recovery is disabled
        if !self.options.recover_from_errors {
            return false;
        }

        match ctx {
            ParsingContext::BlockStatements => {
                if in_error_recovery {
                    // Recovery mode: exclude semicolons (too ambiguous)
                    self.is_start_of_statement_recovery()
                } else {
                    // Normal mode: semicolons are valid (empty statements)
                    self.is_start_of_statement_recovery() || self.at(Kind::Semicolon)
                }
            }
            ParsingContext::Parameters => {
                self.at(Kind::Dot3) // rest parameter
                    || self.at(Kind::LCurly) // object destructuring
                    || self.at(Kind::LBrack) // array destructuring
                    || self.cur_kind().is_identifier_or_keyword()
            }
            ParsingContext::ArgumentExpressions | ParsingContext::ArrayLiteralMembers => {
                self.at(Kind::Dot3) // spread
                    || self.is_start_of_expression_recovery()
            }
            ParsingContext::ClassMembers => {
                if in_error_recovery {
                    self.is_start_of_class_member()
                } else {
                    self.is_start_of_class_member() || self.at(Kind::Semicolon)
                }
            }
            ParsingContext::TypeMembers => {
                self.cur_kind().is_identifier_or_keyword()
                    || self.at(Kind::LBrack) // index signature
                    || self.at(Kind::LParen) // call signature
                    || self.at(Kind::New) // construct signature
                    || self.at(Kind::Readonly)
            }
            ParsingContext::EnumMembers => {
                self.cur_kind().is_identifier_or_keyword() || self.at(Kind::LBrack) // computed (invalid but detect for recovery)
            }
            ParsingContext::ObjectLiteralMembers => {
                self.at(Kind::LBrack) // computed property
                    || self.at(Kind::Star) // generator method
                    || self.at(Kind::Dot3) // spread
                    || self.cur_kind().is_identifier_or_keyword()
            }
            ParsingContext::SwitchClauses => self.at(Kind::Case) || self.at(Kind::Default),
            ParsingContext::ImportSpecifiers | ParsingContext::ExportSpecifiers => {
                self.cur_kind().is_identifier_or_keyword()
            }
            _ => false,
        }
    }

    /// Checks if the current token is valid in any active parsing context.
    ///
    /// This walks up the context stack from innermost to outermost context,
    /// checking if the current token is either:
    /// 1. A terminator for that context
    /// 2. An element start for that context (in recovery mode)
    ///
    /// Returns `true` if the token is meaningful in any parent context,
    /// indicating we should abort the current context and return to the parent.
    ///
    /// **Note**: Returns `false` when `recover_from_errors` is disabled.
    #[inline]
    pub(crate) fn is_in_some_parsing_context(&self) -> bool {
        // Early return if recovery is disabled
        if !self.options.recover_from_errors {
            return false;
        }

        // Check all active contexts from inner to outer
        for ctx in self.context_stack.active_contexts() {
            // Check if current token terminates this context
            if self.is_context_terminator(*ctx) {
                return true;
            }

            // Check if current token can start an element in this context (recovery mode)
            if self.is_context_element_start(*ctx, true) {
                return true;
            }
        }

        false
    }

    /// Performs error recovery synchronization by deciding whether to skip or abort.
    ///
    /// This is the main entry point for error recovery. It analyzes the current token
    /// and context to determine the best recovery strategy:
    ///
    /// - **Abort**: If the token terminates the current context or belongs to a parent context
    /// - **Skip**: If the token is meaningless in all contexts
    ///
    /// The function also advances the token stream when skipping.
    ///
    /// # Parameters
    ///
    /// - `ctx`: The current parsing context where the error occurred
    ///
    /// # Returns
    ///
    /// `RecoveryDecision` indicating whether to skip the token or abort the context.
    ///
    /// **Note**: Returns `Abort` when `recover_from_errors` is disabled (safe default).
    #[inline]
    pub(crate) fn synchronize_on_error(&mut self, ctx: ParsingContext) -> RecoveryDecision {
        // Early return if recovery is disabled - always abort to prevent cascading errors
        if !self.options.recover_from_errors {
            return RecoveryDecision::Abort;
        }

        // Decision 1: If current token terminates this context, abort
        if self.is_context_terminator(ctx) {
            return RecoveryDecision::Abort;
        }

        // Decision 2: If current token is meaningful in some parent context, abort
        // This prevents us from skipping tokens that belong to outer constructs
        if self.is_in_some_parsing_context() {
            return RecoveryDecision::Abort;
        }

        // Decision 3: Token is meaningless everywhere - skip it and continue
        // Advance the token stream before returning
        self.bump_any();
        RecoveryDecision::Skip
    }
}
