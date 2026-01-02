use oxc_allocator::Box;
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::FunctionKind;
use crate::{
    Context, ParserImpl, StatementContext,
    context::ParsingContext,
    diagnostics,
    lexer::Kind,
    modifiers::{ModifierFlags, ModifierKind, Modifiers},
};

impl FunctionKind {
    pub(crate) fn is_id_required(self) -> bool {
        matches!(self, Self::Declaration)
    }

    pub(crate) fn is_expression(self) -> bool {
        self == Self::Expression
    }
}

impl<'a> ParserImpl<'a> {
    pub(crate) fn at_function_with_async(&mut self) -> bool {
        self.at(Kind::Function)
            || self.at(Kind::Async) && {
                let token = self.lexer.peek_token();
                token.kind() == Kind::Function && !token.is_on_new_line()
            }
    }

    pub(crate) fn parse_function_body(&mut self) -> Box<'a, FunctionBody<'a>> {
        let span = self.start_span();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);

        if self.options.recover_from_errors {
            self.context_stack.push(ParsingContext::FunctionBody);
        }

        // M6.5.6 Out of Scope: Parse directives and check for strict mode
        let (directives, statements, has_use_strict) = self.context_add(Context::Return, |p| {
            p.parse_directives_and_statements(/* is_top_level */ false)
        });

        // M6.5.6 Out of Scope: If "use strict" found, re-parse with strict mode context
        // Note: This is a simplified implementation. A full implementation would need
        // to validate the entire function body in strict mode context.
        if has_use_strict {
            // TODO: Re-validate function body in strict mode
            // For now, we just track that strict mode was detected
        }

        if self.options.recover_from_errors {
            self.context_stack.pop();
        }

        self.expect_closing(Kind::RCurly, opening_span);
        self.ast.alloc_function_body(self.end_span(span), directives, statements)
    }

    pub(crate) fn parse_formal_parameters(
        &mut self,
        func_kind: FunctionKind,
        params_kind: FormalParameterKind,
    ) -> (Option<TSThisParameter<'a>>, Box<'a, FormalParameters<'a>>) {
        let span = self.start_span();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LParen);

        if self.options.recover_from_errors {
            self.context_stack.push(ParsingContext::Parameters);
        }
        let this_param = if self.is_ts && self.at(Kind::This) {
            let param = self.parse_ts_this_parameter();
            self.bump(Kind::Comma);
            Some(param)
        } else {
            None
        };
        let (list, rest) = self.parse_formal_parameters_list(func_kind, opening_span);
        if self.options.recover_from_errors {
            self.context_stack.pop();
        }

        // M6.6.0: Use expect_closing to properly pop from paren stack
        self.expect_closing(Kind::RParen, opening_span);

        let formal_parameters =
            self.ast.alloc_formal_parameters(self.end_span(span), params_kind, list, rest);
        (this_param, formal_parameters)
    }

    fn parse_formal_parameters_list(
        &mut self,
        func_kind: FunctionKind,
        opening_span: Span,
    ) -> (oxc_allocator::Vec<'a, FormalParameter<'a>>, Option<Box<'a, FormalParameterRest<'a>>>)
    {
        // Safeguard: prevent infinite loops in error recovery
        const MAX_PARAMETERS: usize = 1000;

        let mut list = self.ast.vec();
        let mut rest: Option<Box<'a, FormalParameterRest<'a>>> = None;
        let mut first = true;
        let mut param_count = 0;

        loop {
            // Safety check: prevent infinite loop on malformed input
            if param_count >= MAX_PARAMETERS {
                if self.options.recover_from_errors {
                    self.error(diagnostics::unexpected_token(self.cur_token().span()));
                }
                break;
            }
            param_count += 1;
            let kind = self.cur_kind();
            if kind == Kind::RParen
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
                        Kind::RParen.to_str(),
                        Kind::Comma.to_str(),
                        kind.to_str(),
                        comma_span,
                        opening_span,
                    );

                    // Error recovery: decide whether to skip or abort
                    if self.options.recover_from_errors {
                        self.error(error);
                        let decision =
                            self.synchronize_on_error(crate::context::ParsingContext::Parameters);
                        match decision {
                            crate::synchronization::RecoveryDecision::Skip => continue,
                            crate::synchronization::RecoveryDecision::Abort => break,
                        }
                    } else {
                        // M6.5.6: Non-recovery mode - fatal error
                        self.set_fatal_error(error);
                        break;
                    }
                }
                self.bump_any();
                let kind = self.cur_kind();
                if kind == Kind::RParen {
                    if rest.is_some() && !self.ctx.has_ambient() {
                        self.error(diagnostics::rest_element_trailing_comma(comma_span));
                    }
                    break;
                }
            }

            if let Some(r) = &rest {
                let error =
                    diagnostics::rest_parameter_last(r.type_annotation.as_ref().map_or_else(
                        || r.rest.span,
                        |type_annotation| r.rest.span.merge(type_annotation.span()),
                    ));

                // Error recovery: rest parameter must be last
                if self.options.recover_from_errors {
                    self.error(error);
                    let decision =
                        self.synchronize_on_error(crate::context::ParsingContext::Parameters);
                    match decision {
                        crate::synchronization::RecoveryDecision::Skip => continue,
                        crate::synchronization::RecoveryDecision::Abort => break,
                    }
                } else {
                    // M6.5.6: Non-recovery mode - fatal error
                    self.set_fatal_error(error);
                    break;
                }
            }

            if self.at(Kind::Dot3) {
                let rest_element = self.parse_rest_element_for_formal_parameter();
                let rest_span = rest_element.span;
                let type_annotation =
                    if self.is_ts { self.parse_ts_type_annotation() } else { None };
                rest = Some(self.ast.alloc_formal_parameter_rest(
                    rest_span,
                    rest_element,
                    type_annotation,
                ));
            } else {
                list.push(self.parse_formal_parameter(func_kind));
            }
        }

        (list, rest)
    }

    /// Creates a dummy parameter for error recovery.
    ///
    /// When parameter parsing fails completely and recovery cannot proceed normally,
    /// this function creates a placeholder parameter with the name `__invalid_param__`.
    /// This allows the AST to remain complete and parsing to continue.
    ///
    /// # Returns
    /// A `FormalParameter` with:
    /// - Pattern: Binding identifier `__invalid_param__`
    /// - No type annotation
    /// - No initializer
    /// - Not a rest parameter
    ///
    /// # Example Usage
    /// ```ignore
    /// // When encountering completely malformed parameter syntax:
    /// let dummy = self.create_dummy_parameter();
    /// list.push(dummy);
    /// ```
    #[expect(dead_code, reason = "Reserved for future error recovery scenarios")]
    fn create_dummy_parameter(&self) -> FormalParameter<'a> {
        let span = self.cur_token().span();

        // Create identifier binding: __invalid_param__
        let pattern =
            self.ast.binding_pattern_binding_identifier(span, self.ast.atom("__invalid_param__"));

        self.ast.formal_parameter(
            span,
            self.ast.vec(), // No decorators
            pattern,
            Option::<Box<'a, TSTypeAnnotation>>::None, // No type annotation
            Option::<Box<'a, Expression>>::None,       // No initializer
            false,                                     // Not optional
            Option::<TSAccessibility>::None,           // No accessibility
            false,                                     // Not readonly
            false,                                     // Not override
        )
    }

    fn parse_formal_parameter(&mut self, func_kind: FunctionKind) -> FormalParameter<'a> {
        let span = self.start_span();
        let decorators = self.parse_decorators();
        let modifiers = self.parse_modifiers(false, false);
        if self.is_ts {
            let allowed_modifiers = if func_kind == FunctionKind::Constructor {
                ModifierFlags::ACCESSIBILITY | ModifierFlags::OVERRIDE | ModifierFlags::READONLY
            } else {
                ModifierFlags::empty()
            };
            self.verify_modifiers(
                &modifiers,
                allowed_modifiers,
                true,
                diagnostics::cannot_appear_on_a_parameter,
            );
        } else {
            self.verify_modifiers(
                &modifiers,
                ModifierFlags::empty(),
                true,
                diagnostics::parameter_modifiers_in_ts,
            );
        }
        let pattern = self.parse_binding_pattern();

        let optional = self.is_ts && self.eat(Kind::Question);
        let type_annotation = self.parse_ts_type_annotation();

        // Now parse the initializer if present
        let init = if self.eat(Kind::Eq) {
            let init =
                self.context_add(Context::In, ParserImpl::parse_assignment_expression_or_higher);
            if optional {
                self.error(diagnostics::a_parameter_cannot_have_question_mark_and_initializer(
                    pattern.span(),
                ));
            }
            Some(init)
        } else {
            None
        };

        if (modifiers.accessibility().is_some()
            || modifiers.contains_readonly()
            || modifiers.contains_override())
            && !pattern.is_binding_identifier()
        {
            self.error(diagnostics::parameter_property_cannot_be_binding_pattern(Span::new(
                span,
                self.prev_token_end,
            )));
        }

        let are_decorators_allowed =
            matches!(func_kind, FunctionKind::ClassMethod | FunctionKind::Constructor)
                && self.is_ts;
        if !are_decorators_allowed {
            for decorator in &decorators {
                self.error(diagnostics::decorators_are_not_valid_here(decorator.span));
            }
        }
        self.ast.formal_parameter(
            self.end_span(span),
            decorators,
            pattern,
            type_annotation,
            init,
            optional,
            modifiers.accessibility(),
            modifiers.contains_readonly(),
            modifiers.contains_override(),
        )
    }

    pub(crate) fn parse_function(
        &mut self,
        span: u32,
        id: Option<BindingIdentifier<'a>>,
        r#async: bool,
        generator: bool,
        func_kind: FunctionKind,
        param_kind: FormalParameterKind,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, Function<'a>> {
        let ctx = self.ctx;
        self.ctx = self.ctx.and_in(true).and_await(r#async).and_yield(generator);
        let type_parameters = self.parse_ts_type_parameters();
        let (this_param, params) = self.parse_formal_parameters(func_kind, param_kind);
        let return_type = if self.is_ts { self.parse_ts_return_type_annotation() } else { None };
        let mut body = if self.at(Kind::LCurly) || func_kind == FunctionKind::Expression {
            Some(self.parse_function_body())
        } else {
            None
        };
        self.ctx =
            self.ctx.and_in(ctx.has_in()).and_await(ctx.has_await()).and_yield(ctx.has_yield());
        if (!self.is_ts || matches!(func_kind, FunctionKind::ObjectMethod)) && body.is_none() {
            // Error recovery: create empty function body if missing
            if self.options.recover_from_errors {
                let body_span = self.end_span(span);
                self.error(diagnostics::expect_function_body(body_span));

                // Create an empty function body as a dummy to allow parsing to continue
                body = Some(self.ast.alloc_function_body(
                    body_span,
                    self.ast.vec(), // Empty directives
                    self.ast.vec(), // Empty statements
                ));
            } else {
                return self.fatal_error(diagnostics::expect_function_body(self.end_span(span)));
            }
        }
        let function_type = match func_kind {
            FunctionKind::Declaration | FunctionKind::DefaultExport => {
                if body.is_none() {
                    FunctionType::TSDeclareFunction
                } else {
                    FunctionType::FunctionDeclaration
                }
            }
            FunctionKind::Expression
            | FunctionKind::ClassMethod
            | FunctionKind::Constructor
            | FunctionKind::ObjectMethod => {
                if body.is_none() {
                    FunctionType::TSEmptyBodyFunctionExpression
                } else {
                    FunctionType::FunctionExpression
                }
            }
            FunctionKind::TSDeclaration => FunctionType::TSDeclareFunction,
        };

        if FunctionType::TSDeclareFunction == function_type
            || FunctionType::TSEmptyBodyFunctionExpression == function_type
        {
            self.asi();
        }

        if ctx.has_ambient()
            && modifiers.contains_declare()
            && let Some(body) = &body
        {
            self.error(diagnostics::implementation_in_ambient(Span::empty(body.span.start)));
        }
        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE | ModifierFlags::ASYNC,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );

        self.ast.alloc_function(
            self.end_span(span),
            function_type,
            id,
            generator,
            r#async,
            modifiers.contains_declare(),
            type_parameters,
            this_param,
            params,
            return_type,
            body,
        )
    }

    /// [Function Declaration](https://tc39.es/ecma262/#prod-FunctionDeclaration)
    pub(crate) fn parse_function_declaration(
        &mut self,
        span: u32,
        r#async: bool,
        stmt_ctx: StatementContext,
    ) -> Statement<'a> {
        let func_kind = FunctionKind::Declaration;
        let decl = self.parse_function_impl(span, r#async, func_kind);
        if stmt_ctx.is_single_statement() {
            if decl.r#async {
                self.error(diagnostics::async_function_declaration(Span::new(
                    decl.span.start,
                    decl.params.span.end,
                )));
            } else if decl.generator {
                self.error(diagnostics::generator_function_declaration(Span::new(
                    decl.span.start,
                    decl.params.span.end,
                )));
            }
        }
        Statement::FunctionDeclaration(decl)
    }

    /// Parse function implementation in Javascript, cursor
    /// at `function` or `async function`
    pub(crate) fn parse_function_impl(
        &mut self,
        span: u32,
        r#async: bool,
        func_kind: FunctionKind,
    ) -> Box<'a, Function<'a>> {
        self.expect(Kind::Function);
        let generator = self.eat(Kind::Star);
        let id = self.parse_function_id(func_kind, r#async, generator);
        self.parse_function(
            span,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            &Modifiers::empty(),
        )
    }

    /// Parse function implementation in Typescript, cursor
    /// at `function`
    pub(crate) fn parse_ts_function_impl(
        &mut self,
        start_span: u32,
        func_kind: FunctionKind,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, Function<'a>> {
        let r#async = modifiers.contains(ModifierKind::Async);
        self.expect(Kind::Function);
        let generator = self.eat(Kind::Star);
        let id = self.parse_function_id(func_kind, r#async, generator);
        self.parse_function(
            start_span,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            modifiers,
        )
    }

    /// [Function Expression](https://tc39.es/ecma262/#prod-FunctionExpression)
    pub(crate) fn parse_function_expression(&mut self, span: u32, r#async: bool) -> Expression<'a> {
        let func_kind = FunctionKind::Expression;
        self.expect(Kind::Function);

        let generator = self.eat(Kind::Star);
        let id = self.parse_function_id(func_kind, r#async, generator);
        let function = self.parse_function(
            span,
            id,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::FormalParameter,
            &Modifiers::empty(),
        );
        Expression::FunctionExpression(function)
    }

    /// Section 15.4 Method Definitions
    /// `ClassElementName` ( `UniqueFormalParameters` ) { `FunctionBody` }
    /// * `GeneratorMethod`
    ///   * `ClassElementName`
    /// * `AsyncMethod`
    ///   async `ClassElementName`
    /// * `AsyncGeneratorMethod`
    ///   async * `ClassElementName`
    pub(crate) fn parse_method(
        &mut self,
        r#async: bool,
        generator: bool,
        func_kind: FunctionKind,
    ) -> Box<'a, Function<'a>> {
        let span = self.start_span();
        self.parse_function(
            span,
            None,
            r#async,
            generator,
            func_kind,
            FormalParameterKind::UniqueFormalParameters,
            &Modifiers::empty(),
        )
    }

    /// Section 15.5 Yield Expression
    /// yield
    /// yield [no `LineTerminator` here] `AssignmentExpression`
    /// yield [no `LineTerminator` here] * `AssignmentExpression`
    pub(crate) fn parse_yield_expression(&mut self) -> Expression<'a> {
        let span = self.start_span();
        self.bump_any(); // advance `yield`

        let has_yield = self.ctx.has_yield();
        if !has_yield {
            self.error(diagnostics::yield_expression(Span::sized(span, 5)));
        }

        let mut delegate = false;
        let mut argument = None;

        if !self.cur_token().is_on_new_line() {
            delegate = self.eat(Kind::Star);
            let not_assignment_expr = matches!(
                self.cur_kind(),
                Kind::Semicolon
                    | Kind::Eof
                    | Kind::RCurly
                    | Kind::RParen
                    | Kind::RBrack
                    | Kind::Colon
                    | Kind::Comma
            );
            if !not_assignment_expr || delegate {
                self.ctx = self.ctx.union_yield_if(true);
                argument = Some(self.parse_assignment_expression_or_higher());
                self.ctx = self.ctx.and_yield(has_yield);
            }
        }

        self.ast.expression_yield(self.end_span(span), delegate, argument)
    }

    // id: None - for AnonymousDefaultExportedFunctionDeclaration
    pub(crate) fn parse_function_id(
        &mut self,
        func_kind: FunctionKind,
        r#async: bool,
        generator: bool,
    ) -> Option<BindingIdentifier<'a>> {
        let kind = self.cur_kind();
        if kind.is_binding_identifier() {
            let mut ctx = self.ctx;
            if func_kind.is_expression() {
                ctx = ctx.and_await(r#async).and_yield(generator);
            }
            self.check_identifier(kind, ctx);

            let (span, name) = self.parse_identifier_kind(Kind::Ident);
            Some(self.ast.binding_identifier(span, name))
        } else {
            if func_kind.is_id_required() {
                match self.cur_kind() {
                    Kind::LParen => {
                        self.error(diagnostics::expect_function_name(self.cur_token().span()));
                    }
                    kind if kind.is_reserved_keyword() => self.expect_without_advance(Kind::Ident),
                    _ => {}
                }
            }

            None
        }
    }
}
