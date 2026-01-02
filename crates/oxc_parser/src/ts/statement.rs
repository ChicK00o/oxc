use oxc_allocator::{Box, Vec};
use oxc_ast::ast::*;
use oxc_span::GetSpan;

use crate::{
    ParserImpl,
    context::ParsingContext,
    diagnostics,
    js::{FunctionKind, VariableDeclarationParent},
    lexer::Kind,
    modifiers::{ModifierFlags, ModifierKind, Modifiers},
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum CallOrConstructorSignature {
    Call,
    Constructor,
}

impl<'a> ParserImpl<'a> {
    /* ------------------- Enum ------------------ */
    /// `https://www.typescriptlang.org/docs/handbook/enums.html`
    pub(crate) fn parse_ts_enum_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Declaration<'a> {
        self.bump_any(); // bump `enum`
        let id = self.parse_binding_identifier();
        let body = self.parse_ts_enum_body();
        let span = self.end_span(span);
        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE | ModifierFlags::CONST,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );
        self.ast.declaration_ts_enum(
            span,
            id,
            body,
            modifiers.contains_const(),
            modifiers.contains_declare(),
        )
    }

    pub(crate) fn parse_ts_enum_body(&mut self) -> TSEnumBody<'a> {
        let span = self.start_span();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);

        if self.options.recover_from_errors {
            self.context_stack.push(ParsingContext::EnumMembers);
        }

        // Custom loop with error recovery for enum members
        let mut members = self.ast.vec();

        let kind = self.cur_kind();
        if kind != Kind::RCurly
            && !matches!(kind, Kind::Eof | Kind::Undetermined)
            && self.fatal_error.is_none()
        {
            members.push(self.parse_ts_enum_member());

            loop {
                let kind = self.cur_kind();

                // Check termination conditions
                if kind == Kind::RCurly
                    || matches!(kind, Kind::Eof | Kind::Undetermined)
                    || self.fatal_error.is_some()
                {
                    break;
                }

                // Expect comma separator
                if kind != Kind::Comma {
                    let error = diagnostics::expect_closing_or_separator(
                        Kind::RCurly.to_str(),
                        Kind::Comma.to_str(),
                        kind.to_str(),
                        self.cur_token().span(),
                        opening_span,
                    );

                    // Error recovery: decide whether to skip or abort
                    if self.options.recover_from_errors {
                        self.error(error);
                        let decision =
                            self.synchronize_on_error(crate::context::ParsingContext::EnumMembers);
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

                self.bump(Kind::Comma);

                // Check for trailing comma
                if self.cur_kind() == Kind::RCurly {
                    break;
                }

                members.push(self.parse_ts_enum_member());
            }
        }

        if self.options.recover_from_errors {
            self.context_stack.pop();
        }

        self.expect(Kind::RCurly);
        self.ast.ts_enum_body(self.end_span(span), members)
    }

    pub(crate) fn parse_ts_enum_member(&mut self) -> TSEnumMember<'a> {
        let span = self.start_span();
        let id = self.parse_ts_enum_member_name();
        let initializer = if self.eat(Kind::Eq) {
            Some(self.parse_assignment_expression_or_higher())
        } else {
            None
        };
        self.ast.ts_enum_member(self.end_span(span), id, initializer)
    }

    fn parse_ts_enum_member_name(&mut self) -> TSEnumMemberName<'a> {
        match self.cur_kind() {
            Kind::Str => {
                let literal = self.parse_literal_string();
                TSEnumMemberName::String(self.alloc(literal))
            }
            Kind::LBrack => match self.parse_computed_property_name() {
                Expression::StringLiteral(literal) => TSEnumMemberName::ComputedString(literal),
                Expression::TemplateLiteral(template) if template.is_no_substitution_template() => {
                    TSEnumMemberName::ComputedTemplateString(template)
                }
                Expression::NumericLiteral(literal) => {
                    let error = diagnostics::enum_member_cannot_have_numeric_name(literal.span());
                    if self.options.recover_from_errors {
                        self.error(error);
                        // Convert numeric literal to valid identifier by prefixing with '_'
                        let num_str = literal.value.to_string();
                        let identifier = self
                            .ast
                            .identifier_name(literal.span(), self.ast.atom(&format!("_{num_str}")));
                        TSEnumMemberName::Identifier(self.alloc(identifier))
                    } else {
                        self.fatal_error(error)
                    }
                }
                expr => {
                    let error =
                        diagnostics::computed_property_names_not_allowed_in_enums(expr.span());
                    if self.options.recover_from_errors {
                        self.error(error);
                        // Create dummy identifier for computed property
                        let identifier =
                            self.ast.identifier_name(expr.span(), self.ast.atom("__computed__"));
                        TSEnumMemberName::Identifier(self.alloc(identifier))
                    } else {
                        self.fatal_error(error)
                    }
                }
            },
            Kind::NoSubstitutionTemplate | Kind::TemplateHead => {
                let error = diagnostics::computed_property_names_not_allowed_in_enums(
                    self.cur_token().span(),
                );
                if self.options.recover_from_errors {
                    self.error(error);
                    // Create dummy identifier for template literal
                    let span = self.cur_token().span();
                    let identifier = self.ast.identifier_name(span, self.ast.atom("__template__"));
                    self.bump_any(); // Consume the template token
                    TSEnumMemberName::Identifier(self.alloc(identifier))
                } else {
                    self.fatal_error(error)
                }
            }
            kind if kind.is_number() => {
                let error =
                    diagnostics::enum_member_cannot_have_numeric_name(self.cur_token().span());
                if self.options.recover_from_errors {
                    self.error(error);
                    // Convert numeric token to valid identifier by prefixing with '_'
                    let span = self.cur_token().span();
                    let num_str = self.cur_src();
                    let identifier =
                        self.ast.identifier_name(span, self.ast.atom(&format!("_{num_str}")));
                    self.bump_any(); // Consume the numeric token
                    TSEnumMemberName::Identifier(self.alloc(identifier))
                } else {
                    self.fatal_error(error)
                }
            }
            _ => {
                let ident_name = self.parse_identifier_name();
                TSEnumMemberName::Identifier(self.alloc(ident_name))
            }
        }
    }

    /* ------------------- Annotation ----------------- */

    pub(crate) fn parse_ts_type_annotation(&mut self) -> Option<Box<'a, TSTypeAnnotation<'a>>> {
        if !self.is_ts {
            return None;
        }
        if !self.at(Kind::Colon) {
            return None;
        }
        let span = self.start_span();
        self.bump_any(); // bump ':'
        let type_annotation = self.parse_ts_type();
        Some(self.ast.alloc_ts_type_annotation(self.end_span(span), type_annotation))
    }

    pub(crate) fn parse_ts_type_alias_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Declaration<'a> {
        self.expect(Kind::Type);

        let id = self.parse_binding_identifier();
        let params = self.parse_ts_type_parameters();
        self.expect(Kind::Eq);

        let intrinsic_token = self.cur_token();
        let ty = if self.at(Kind::Intrinsic) {
            self.bump_any();
            if self.at(Kind::Dot) {
                // `type something = intrinsic. ...`
                let left_name = self.ast.ts_type_name_identifier_reference(
                    intrinsic_token.span(),
                    self.token_source(&intrinsic_token),
                );
                let type_name =
                    self.parse_ts_qualified_type_name(intrinsic_token.start(), left_name);
                let type_parameters = self.parse_type_arguments_of_type_reference();
                self.ast.ts_type_type_reference(
                    self.end_span(intrinsic_token.start()),
                    type_name,
                    type_parameters,
                )
            } else {
                // `type something = intrinsic`
                self.ast.ts_type_intrinsic_keyword(intrinsic_token.span())
            }
        } else {
            // `type something = ...`
            self.parse_ts_type()
        };

        self.asi();
        let span = self.end_span(span);

        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );

        self.ast.declaration_ts_type_alias(span, id, params, ty, modifiers.contains_declare())
    }

    /* ---------------------  Interface  ------------------------ */

    pub(crate) fn parse_ts_interface_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Declaration<'a> {
        let id = self.parse_binding_identifier();
        let type_parameters = self.parse_ts_type_parameters();
        let (extends, implements) = self.parse_heritage_clause();
        let body = self.parse_ts_interface_body();
        let extends = extends.map_or_else(
            || self.ast.vec(),
            |e| {
                self.ast.vec_from_iter(e.into_iter().map(|(expression, type_parameters, span)| {
                    TSInterfaceHeritage { span, expression, type_arguments: type_parameters }
                }))
            },
        );
        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );
        if let Some((implements_kw_span, _)) = implements {
            self.error(diagnostics::interface_implements(implements_kw_span));
        }
        for extend in &extends {
            if self.fatal_error.is_some() {
                break;
            }
            if !extend.expression.is_entity_name_expression() {
                self.error(diagnostics::interface_extend(extend.span));
            }
        }
        self.ast.declaration_ts_interface(
            self.end_span(span),
            id,
            type_parameters,
            extends,
            body,
            modifiers.contains_declare(),
        )
    }

    fn parse_ts_interface_body(&mut self) -> Box<'a, TSInterfaceBody<'a>> {
        let span = self.start_span();
        let opening_span = self.cur_token().span();
        self.expect(Kind::LCurly);

        if self.options.recover_from_errors {
            self.context_stack.push(ParsingContext::TypeMembers);
        }

        // Custom loop with error recovery for type members
        let mut body_list = self.ast.vec();
        loop {
            let kind = self.cur_kind();

            // Check termination conditions
            if kind == Kind::RCurly
                || matches!(kind, Kind::Eof | Kind::Undetermined)
                || self.fatal_error.is_some()
            {
                break;
            }

            // Skip semicolons (member separators)
            if self.eat(Kind::Semicolon) {
                while self.eat(Kind::Semicolon) {}
                if self.at(Kind::RCurly) {
                    break;
                }
            }

            // Check if we can start a type member here (for error recovery)
            if self.options.recover_from_errors
                && !self
                    .is_context_element_start(crate::context::ParsingContext::TypeMembers, false)
            {
                // Not a valid type member start - report error and synchronize
                let error = diagnostics::expect_token(
                    "type member",
                    self.cur_kind().to_str(),
                    self.cur_token().span(),
                );
                self.error(error);

                let decision =
                    self.synchronize_on_error(crate::context::ParsingContext::TypeMembers);
                match decision {
                    crate::synchronization::RecoveryDecision::Skip => continue,
                    crate::synchronization::RecoveryDecision::Abort => break,
                }
            }

            // Parse type member
            body_list.push(Self::parse_ts_type_signature(self));
        }

        if self.options.recover_from_errors {
            self.context_stack.pop();
        }

        self.expect_closing(Kind::RCurly, opening_span);
        self.ast.alloc_ts_interface_body(self.end_span(span), body_list)
    }

    pub(crate) fn parse_ts_type_signature(&mut self) -> TSSignature<'a> {
        let span = self.start_span();
        let kind = self.cur_kind();

        if matches!(kind, Kind::LParen | Kind::LAngle) {
            return self.parse_signature_member(CallOrConstructorSignature::Call);
        }

        if kind == Kind::New
            && matches!(self.lexer.peek_token().kind(), Kind::LParen | Kind::LAngle)
        {
            return self.parse_signature_member(CallOrConstructorSignature::Constructor);
        }

        let modifiers = self.parse_modifiers(
            /* permit_const_as_modifier */ true,
            /* stop_on_start_of_class_static_block */ false,
        );

        if self.is_index_signature() {
            self.verify_modifiers(
                &modifiers,
                ModifierFlags::READONLY,
                true,
                diagnostics::cannot_appear_on_an_index_signature,
            );
            return TSSignature::TSIndexSignature(
                self.parse_index_signature_declaration(span, &modifiers),
            );
        }

        self.verify_modifiers(
            &modifiers,
            ModifierFlags::READONLY,
            true,
            diagnostics::cannot_appear_on_a_type_member,
        );

        if self.parse_contextual_modifier(Kind::Get) {
            return self.parse_getter_setter_signature_member(span, TSMethodSignatureKind::Get);
        }

        if self.parse_contextual_modifier(Kind::Set) {
            return self.parse_getter_setter_signature_member(span, TSMethodSignatureKind::Set);
        }

        self.parse_property_or_method_signature(span, &modifiers)
    }

    pub(crate) fn is_index_signature(&mut self) -> bool {
        self.at(Kind::LBrack) && self.lookahead(Self::is_unambiguously_index_signature)
    }

    fn is_unambiguously_index_signature(&mut self) -> bool {
        self.bump_any();
        if matches!(self.cur_kind(), Kind::Dot3 | Kind::LBrack) {
            return true;
        }
        if self.cur_kind().is_modifier_kind() {
            self.bump_any();
            if self.cur_kind().is_identifier() {
                return true;
            }
        } else if !self.cur_kind().is_identifier() {
            return false;
        } else {
            self.bump_any();
        }
        if matches!(self.cur_kind(), Kind::Colon | Kind::Comma) {
            return true;
        }
        if self.cur_kind() != Kind::Question {
            return false;
        }
        self.bump_any();
        matches!(self.cur_kind(), Kind::Colon | Kind::Comma | Kind::RBrack)
    }

    /* ----------------------- Namespace & Module ----------------------- */

    fn parse_ts_module_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, TSModuleDeclaration<'a>> {
        let kind = if self.eat(Kind::Namespace) {
            TSModuleDeclarationKind::Namespace
        } else {
            self.expect(Kind::Module);
            if self.at(Kind::Str) {
                return self.parse_ambient_external_module_declaration(span, modifiers);
            }
            TSModuleDeclarationKind::Module
        };
        self.parse_module_or_namespace_declaration(span, kind, modifiers)
    }

    fn parse_ambient_external_module_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, TSModuleDeclaration<'a>> {
        let id = TSModuleDeclarationName::StringLiteral(self.parse_literal_string());
        let body = if self.at(Kind::LCurly) {
            let block = self.parse_ts_module_block();
            Some(TSModuleDeclarationBody::TSModuleBlock(block))
        } else {
            self.asi();
            None
        };
        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );
        self.ast.alloc_ts_module_declaration(
            self.end_span(span),
            id,
            body,
            TSModuleDeclarationKind::Module,
            modifiers.contains_declare(),
        )
    }

    fn parse_ts_module_block(&mut self) -> Box<'a, TSModuleBlock<'a>> {
        let span = self.start_span();
        self.expect(Kind::LCurly);
        // M6.5.6 Out of Scope: Parse directives and check for strict mode
        let (directives, statements, has_use_strict) =
            self.parse_directives_and_statements(/* is_top_level */ false);
        // M6.5.6 Out of Scope: Track strict mode in TS module blocks
        let _ = has_use_strict;
        self.expect(Kind::RCurly);
        self.ast.alloc_ts_module_block(self.end_span(span), directives, statements)
    }

    fn parse_module_or_namespace_declaration(
        &mut self,
        span: u32,
        kind: TSModuleDeclarationKind,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, TSModuleDeclaration<'a>> {
        let id = TSModuleDeclarationName::Identifier(self.parse_binding_identifier());
        let body = if self.eat(Kind::Dot) {
            let span = self.start_span();
            let decl = self.parse_module_or_namespace_declaration(span, kind, &Modifiers::empty());
            TSModuleDeclarationBody::TSModuleDeclaration(decl)
        } else {
            let block = self.parse_ts_module_block();
            TSModuleDeclarationBody::TSModuleBlock(block)
        };
        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );
        self.ast.alloc_ts_module_declaration(
            self.end_span(span),
            id,
            Some(body),
            kind,
            modifiers.contains_declare(),
        )
    }

    fn parse_ts_global_declaration(
        &mut self,
        span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, TSGlobalDeclaration<'a>> {
        let keyword_span_start = self.start_span();
        self.expect(Kind::Global);
        let keyword_span = self.end_span(keyword_span_start);

        let body = self.parse_ts_module_block().unbox();

        self.verify_modifiers(
            modifiers,
            ModifierFlags::DECLARE,
            true,
            diagnostics::modifier_cannot_be_used_here,
        );

        self.ast.alloc_ts_global_declaration(
            self.end_span(span),
            keyword_span,
            body,
            modifiers.contains_declare(),
        )
    }

    /* ----------------------- declare --------------------- */

    pub(crate) fn parse_ts_declaration_statement(&mut self, start_span: u32) -> Statement<'a> {
        let reserved_ctx = self.ctx;
        let modifiers = self.eat_modifiers_before_declaration();
        self.ctx = self
            .ctx
            .union_ambient_if(modifiers.contains_declare())
            .and_await(modifiers.contains_async());
        let decl = self.parse_declaration(start_span, &modifiers, self.ast.vec());
        self.ctx = reserved_ctx;
        Statement::from(decl)
    }

    pub(crate) fn parse_declaration(
        &mut self,
        start_span: u32,
        modifiers: &Modifiers<'a>,
        decorators: Vec<'a, Decorator<'a>>,
    ) -> Declaration<'a> {
        let kind = self.cur_kind();
        if kind != Kind::Class {
            for decorator in &decorators {
                self.error(diagnostics::decorators_are_not_valid_here(decorator.span));
            }
        }
        match kind {
            Kind::Var | Kind::Let | Kind::Const => {
                let kind = self.get_variable_declaration_kind();
                self.bump_any();
                self.verify_modifiers(
                    modifiers,
                    ModifierFlags::DECLARE,
                    true,
                    diagnostics::modifier_cannot_be_used_here,
                );
                let decl = self.parse_variable_declaration(
                    start_span,
                    kind,
                    VariableDeclarationParent::Statement,
                    modifiers.contains_declare(),
                );
                Declaration::VariableDeclaration(decl)
            }
            Kind::Using if self.is_using_declaration() => {
                if self.options.recover_from_errors {
                    // Get identifier for error message before consuming tokens
                    self.expect(Kind::Using);
                    let identifier = self.parse_identifier_kind(self.cur_kind()).1.as_str();
                    self.error(diagnostics::using_declaration_cannot_be_exported(
                        identifier,
                        self.end_span(start_span),
                    ));
                    // Parse the using declaration manually (Using token already consumed)
                    // Parse variable declarators
                    let kind = VariableDeclarationKind::Using;
                    let mut declarations = self.ast.vec();
                    loop {
                        let declaration = self
                            .parse_variable_declarator(VariableDeclarationParent::Statement, kind);
                        declarations.push(declaration);
                        if !self.eat(Kind::Comma) {
                            break;
                        }
                    }
                    self.asi();
                    let using_decl = self.ast.alloc_variable_declaration(
                        self.end_span(start_span),
                        kind,
                        declarations,
                        false, // declare
                    );
                    Declaration::VariableDeclaration(using_decl)
                } else {
                    self.expect(Kind::Using);
                    let identifier = self.parse_identifier_kind(self.cur_kind()).1.as_str();
                    self.fatal_error(diagnostics::using_declaration_cannot_be_exported(
                        identifier,
                        self.end_span(start_span),
                    ))
                }
            }
            Kind::Await if self.is_using_statement() => {
                if self.options.recover_from_errors {
                    self.expect(Kind::Await);
                    self.expect(Kind::Using);
                    let identifier = self.parse_identifier_kind(self.cur_kind()).1.as_str();
                    self.error(diagnostics::using_declaration_cannot_be_exported(
                        identifier,
                        self.end_span(start_span),
                    ));
                    // Parse the await using declaration manually (Await and Using tokens already consumed)
                    let kind = VariableDeclarationKind::AwaitUsing;
                    let mut declarations = self.ast.vec();
                    loop {
                        let declaration = self
                            .parse_variable_declarator(VariableDeclarationParent::Statement, kind);
                        declarations.push(declaration);
                        if !self.eat(Kind::Comma) {
                            break;
                        }
                    }
                    self.asi();
                    let using_decl = self.ast.alloc_variable_declaration(
                        self.end_span(start_span),
                        kind,
                        declarations,
                        false, // declare
                    );
                    Declaration::VariableDeclaration(using_decl)
                } else {
                    self.expect(Kind::Await);
                    self.expect(Kind::Using);
                    let identifier = self.parse_identifier_kind(self.cur_kind()).1.as_str();
                    self.fatal_error(diagnostics::using_declaration_cannot_be_exported(
                        identifier,
                        self.end_span(start_span),
                    ))
                }
            }
            Kind::Class => {
                let decl = self.parse_class_declaration(start_span, modifiers, decorators);
                Declaration::ClassDeclaration(decl)
            }
            Kind::Import => {
                self.bump_any();
                let token = self.cur_token();
                let mut import_kind = ImportOrExportKind::Value;
                let mut identifier = self.parse_binding_identifier();
                if self.is_ts
                    && token.kind() == Kind::Type
                    && self.cur_kind().is_binding_identifier()
                {
                    // `import type something ...`
                    identifier = self.parse_binding_identifier();
                    import_kind = ImportOrExportKind::Type;
                }
                self.parse_ts_import_equals_declaration(import_kind, identifier, start_span)
            }
            Kind::Module | Kind::Namespace if self.is_ts => {
                let decl = self.parse_ts_module_declaration(start_span, modifiers);
                Declaration::TSModuleDeclaration(decl)
            }
            Kind::Global if self.is_ts => {
                let decl = self.parse_ts_global_declaration(start_span, modifiers);
                Declaration::TSGlobalDeclaration(decl)
            }
            Kind::Type if self.is_ts => self.parse_ts_type_alias_declaration(start_span, modifiers),
            Kind::Enum if self.is_ts => self.parse_ts_enum_declaration(start_span, modifiers),
            Kind::Interface if self.is_ts => {
                self.bump_any();
                self.parse_ts_interface_declaration(start_span, modifiers)
            }
            _ if self.at_function_with_async() => {
                let declare = modifiers.contains(ModifierKind::Declare);
                if declare {
                    let decl = self.parse_ts_declare_function(start_span, modifiers);
                    Declaration::FunctionDeclaration(decl)
                } else if self.is_ts {
                    let decl = self.parse_ts_function_impl(
                        start_span,
                        FunctionKind::Declaration,
                        modifiers,
                    );
                    Declaration::FunctionDeclaration(decl)
                } else {
                    let span = self.start_span();
                    let r#async = self.eat(Kind::Async);
                    let decl = self.parse_function_impl(span, r#async, FunctionKind::Declaration);
                    Declaration::FunctionDeclaration(decl)
                }
            }
            _ => self.unexpected(),
        }
    }

    pub(crate) fn parse_ts_declare_function(
        &mut self,
        start_span: u32,
        modifiers: &Modifiers<'a>,
    ) -> Box<'a, Function<'a>> {
        let r#async = modifiers.contains(ModifierKind::Async);
        self.expect(Kind::Function);
        let func_kind = FunctionKind::TSDeclaration;
        let id = self.parse_function_id(func_kind, r#async, false);
        self.parse_function(
            start_span,
            id,
            r#async,
            false,
            func_kind,
            FormalParameterKind::FormalParameter,
            modifiers,
        )
    }

    pub(crate) fn parse_ts_type_assertion(&mut self) -> Expression<'a> {
        let span = self.start_span();
        self.expect(Kind::LAngle);
        let type_annotation = self.parse_ts_type();
        self.expect(Kind::RAngle);
        let lhs_span = self.start_span();
        let expression = self.parse_simple_unary_expression(lhs_span);
        self.ast.expression_ts_type_assertion(self.end_span(span), type_annotation, expression)
    }

    pub(crate) fn parse_ts_import_equals_declaration(
        &mut self,
        import_kind: ImportOrExportKind,
        identifier: BindingIdentifier<'a>,
        span: u32,
    ) -> Declaration<'a> {
        self.expect(Kind::Eq);

        let reference_span = self.start_span();
        let module_reference = if self.eat(Kind::Require) {
            self.expect(Kind::LParen);
            let expression = self.parse_literal_string();
            self.expect(Kind::RParen);
            self.ast.ts_module_reference_external_module_reference(
                self.end_span(reference_span),
                expression,
            )
        } else {
            let type_name = self.parse_ts_type_name();
            TSModuleReference::from(type_name)
        };

        self.asi();

        let span = self.end_span(span);

        if !self.is_ts {
            self.error(diagnostics::import_equals_can_only_be_used_in_typescript_files(span));
        }

        self.ast.declaration_ts_import_equals(span, identifier, module_reference, import_kind)
    }

    pub(crate) fn parse_ts_this_parameter(&mut self) -> TSThisParameter<'a> {
        let span = self.start_span();
        self.bump_any();
        let this_span = self.end_span(span);

        let type_annotation = self.parse_ts_type_annotation();
        self.ast.ts_this_parameter(self.end_span(span), this_span, type_annotation)
    }

    pub(crate) fn at_start_of_ts_declaration(&mut self) -> bool {
        self.lookahead(Self::at_start_of_ts_declaration_worker)
    }

    /// Check if the parser is at a start of a ts declaration
    fn at_start_of_ts_declaration_worker(&mut self) -> bool {
        loop {
            match self.cur_kind() {
                Kind::Var | Kind::Let | Kind::Const | Kind::Function | Kind::Class | Kind::Enum => {
                    return true;
                }
                Kind::Interface | Kind::Type => {
                    self.bump_any();
                    return self.cur_kind().is_binding_identifier()
                        && !self.cur_token().is_on_new_line();
                }
                Kind::Module | Kind::Namespace => {
                    self.bump_any();
                    return !self.cur_token().is_on_new_line()
                        && (self.cur_kind().is_binding_identifier()
                            || self.cur_kind() == Kind::Str);
                }
                Kind::Abstract
                | Kind::Accessor
                | Kind::Async
                | Kind::Declare
                | Kind::Private
                | Kind::Protected
                | Kind::Public
                | Kind::Readonly => {
                    self.bump_any();
                    if self.cur_token().is_on_new_line() {
                        return false;
                    }
                }
                Kind::Global => {
                    self.bump_any();
                    return matches!(self.cur_kind(), Kind::Ident | Kind::LCurly | Kind::Export);
                }
                Kind::Import => {
                    self.bump_any();
                    let kind = self.cur_kind();
                    return matches!(kind, Kind::Str | Kind::Star | Kind::LCurly)
                        || kind.is_identifier();
                }
                Kind::Export => {
                    self.bump_any();
                    self.bump(Kind::Type); // optional `type` after `export`
                    // This allows constructs like
                    // `export *`, `export default`, `export {}`, `export = {}` along with all
                    // export [declaration]
                    if matches!(
                        self.cur_kind(),
                        Kind::Eq | Kind::Star | Kind::Default | Kind::LCurly | Kind::At | Kind::As
                    ) {
                        return true;
                    }
                    // falls through to check next token
                }
                Kind::Static => {
                    self.bump_any();
                }
                _ => {
                    return false;
                }
            }
        }
    }
}
