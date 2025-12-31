//! ECMAScript Grammar Contexts: `[In`] `[Yield]` `[Await]`

use bitflags::bitflags;

bitflags! {
    /// 5.1.5 Grammar Notation
    /// A production may be parameterized by a subscripted annotation of the form “[parameters]”,
    /// which may appear as a suffix to the nonterminal symbol defined by the production.
    /// “parameters” may be either a single name or a comma separated list of names.
    /// A parameterized production is shorthand for a set of productions defining all combinations of the parameter names,
    /// preceded by an underscore, appended to the parameterized nonterminal symbol.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Context: u8 {
        /// [In] Flag, i.e. the [In] part in RelationalExpression[In, Yield, Await]
        /// Section 13.10 Relational Operators Note 2:
        /// The [In] grammar parameter is needed to avoid confusing the in operator
        /// in a relational expression with the in operator in a for statement.
        const In = 1 << 0;

        /// [Yield] Flag
        const Yield = 1 << 1;

        /// [Await] Flag
        /// Section 15.8 Async Function Definitions Note 1:
        /// await is parsed as an AwaitExpression when the [Await] parameter is present
        const Await = 1 << 2;

        /// [Return] Flag
        /// i.e. the [Return] in Statement[Yield, Await, Return]
        const Return = 1<< 3;

        /// If node was parsed as part of a decorator
        const Decorator = 1 << 4;

        /// Typescript should parse extends clause as conditional type instead of type constrains.
        /// Used in infer clause
        ///
        /// type X<U, T> = T extends infer U extends number ? U : T;
        /// The "infer U extends number" is type constrains.
        ///
        /// type X<U, T> = T extends (infer U extends number ? U : T) ? U : T;
        /// The "(infer U extends number ? U : T)" is conditional type.
        const DisallowConditionalTypes = 1 << 5;

        /// A declaration file, or inside something with the `declare` modifier.
        /// Declarations that don't define an implementation is "ambient":
        ///   * ambient variable declaration => `declare var $: any`
        ///   * ambient class declaration => `declare class C { foo(); } , etc..`
        const Ambient = 1 << 6;
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::In
    }
}

impl Context {
    #[inline]
    pub(crate) fn has_in(self) -> bool {
        self.contains(Self::In)
    }

    #[inline]
    pub(crate) fn has_yield(self) -> bool {
        self.contains(Self::Yield)
    }

    #[inline]
    pub(crate) fn has_await(self) -> bool {
        self.contains(Self::Await)
    }

    #[inline]
    pub(crate) fn has_return(self) -> bool {
        self.contains(Self::Return)
    }

    #[inline]
    pub(crate) fn has_decorator(self) -> bool {
        self.contains(Self::Decorator)
    }

    #[inline]
    pub(crate) fn has_disallow_conditional_types(self) -> bool {
        self.contains(Self::DisallowConditionalTypes)
    }

    #[inline]
    pub(crate) fn has_ambient(self) -> bool {
        self.contains(Self::Ambient)
    }

    #[inline]
    pub(crate) fn union_await_if(self, include: bool) -> Self {
        self.union_if(Self::Await, include)
    }

    #[inline]
    pub(crate) fn union_ambient_if(self, include: bool) -> Self {
        self.union_if(Self::Ambient, include)
    }

    #[inline]
    pub(crate) fn union_yield_if(self, include: bool) -> Self {
        self.union_if(Self::Yield, include)
    }

    #[inline]
    fn union_if(self, other: Self, include: bool) -> Self {
        if include { self.union(other) } else { self }
    }

    #[inline]
    pub(crate) fn and_in(self, include: bool) -> Self {
        self.and(Self::In, include)
    }

    #[inline]
    pub(crate) fn and_yield(self, include: bool) -> Self {
        self.and(Self::Yield, include)
    }

    #[inline]
    pub(crate) fn and_await(self, include: bool) -> Self {
        self.and(Self::Await, include)
    }

    #[inline]
    pub(crate) fn and_return(self, include: bool) -> Self {
        self.and(Self::Return, include)
    }

    #[inline]
    pub(crate) fn and_decorator(self, include: bool) -> Self {
        self.and(Self::Decorator, include)
    }

    #[inline]
    pub(crate) fn and_ambient(self, include: bool) -> Self {
        self.and(Self::Ambient, include)
    }

    #[inline]
    fn and(self, flag: Self, set: bool) -> Self {
        if set { self | flag } else { self - flag }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatementContext {
    StatementList,
    TopLevelStatementList,
    If,
    Label,
    Do,
    While,
    With,
    For,
}

impl StatementContext {
    pub(crate) fn is_single_statement(self) -> bool {
        !matches!(self, Self::StatementList | Self::TopLevelStatementList)
    }

    pub(crate) fn is_top_level(self) -> bool {
        self == Self::TopLevelStatementList
    }
}

/// Parsing context types for error recovery synchronization.
///
/// These contexts track the current parsing position and are used by the error recovery
/// mechanism to determine where to synchronize after encountering a syntax error.
/// This follows TypeScript's error recovery strategy of maintaining a context stack
/// to make intelligent decisions about token skipping vs. aborting current parsing context.
///
/// # Error Recovery Strategy
///
/// When a syntax error is encountered:
/// 1. Determine the current parsing context (top of stack)
/// 2. Check if the current token could be valid in any parent context
/// 3. If yes, abort current context and return to parent (synchronize)
/// 4. If no, skip the token and continue trying to parse in current context
///
/// # Context Stack Example
///
/// ```text
/// class C {           // Push ClassMembers
///   method(a b) {     // Push Parameters (error: missing comma)
///                     // Synchronize: skip to ) and return
///     return a;       // Pop Parameters, push FunctionBody
///   }                 // Pop FunctionBody
/// }                   // Pop ClassMembers
/// ```
#[cfg_attr(not(test), expect(dead_code, reason = "M6.5: TypeAnnotation, TypeParameters, TypeArguments, JsxAttributes, JsxChildren will be used in future steps"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParsingContext {
    /// Top-level parsing context (module or script).
    /// This context is never popped and serves as the root of the context stack.
    TopLevel,

    /// Block statement context (`{ ... }`).
    /// Terminates on `}` or EOF.
    BlockStatements,

    /// Function body context (function/method body block).
    /// Terminates on `}` or EOF.
    FunctionBody,

    /// Parameter list context (function/method parameters).
    /// Terminates on `)` or other tokens indicating end of parameters.
    /// Also includes extra tokens like `{`, `extends`, `implements` for better error recovery.
    Parameters,

    /// Argument expression list context (function call arguments).
    /// Terminates on `)` or `;` (statement boundary for recovery).
    ArgumentExpressions,

    /// Type annotation context (`: Type` positions).
    /// Used to track when parsing type expressions.
    TypeAnnotation,

    /// Type members context (interface/type literal bodies).
    /// Terminates on `}` or EOF.
    TypeMembers,

    /// Class members context (class body).
    /// Terminates on `}` or EOF.
    ClassMembers,

    /// Enum members context (enum body).
    /// Terminates on `}` or EOF.
    EnumMembers,

    /// Object literal members context (object literal properties).
    /// Terminates on `}` or EOF.
    ObjectLiteralMembers,

    /// Array literal members context (array literal elements).
    /// Terminates on `]` or EOF.
    ArrayLiteralMembers,

    /// Switch clauses context (switch statement cases).
    /// Terminates on `}`, or can recover at `case`/`default`.
    SwitchClauses,

    /// Import specifiers context (import list).
    /// Terminates on `}`, `from`, or `;`.
    ImportSpecifiers,

    /// Export specifiers context (export list).
    /// Terminates on `}`, `from`, or `;`.
    ExportSpecifiers,

    /// Type parameters context (generic `<...>` declaration).
    /// Terminates on `>`, `{`, or `extends`.
    TypeParameters,

    /// Type arguments context (generic `<...>` application).
    /// Terminates on `>`, `)`, or `{`.
    TypeArguments,

    /// JSX attributes context (JSX element attributes).
    /// Terminates on `>` or `/>`.
    JsxAttributes,

    /// JSX children context (JSX element children).
    /// Terminates on closing JSX tag.
    JsxChildren,
}

/// Stack-based tracking of parsing contexts for error recovery.
///
/// This struct maintains a stack of `ParsingContext` values that represent the
/// current nesting of parsing contexts. The stack is used by the error recovery
/// mechanism to:
/// 1. Determine termination tokens for the current context
/// 2. Check if a token is valid in any parent context
/// 3. Make intelligent skip-or-abort decisions when encountering errors
///
/// # Invariants
///
/// - The stack always contains at least `TopLevel` (never empty)
/// - `TopLevel` is at the bottom and cannot be popped
/// - Push/pop operations must be balanced within parsing functions
///
/// # Usage Pattern
///
/// ```rust,ignore
/// fn parse_parameters(&mut self) -> Result<Vec<Parameter>> {
///     self.context_stack.push(ParsingContext::Parameters);
///     // ... parse parameters ...
///     self.context_stack.pop();
///     Ok(parameters)
/// }
/// ```
///
/// # Thread Safety
///
/// This struct is not thread-safe and is intended to be used within a single parser instance.
#[derive(Debug, Clone)]
pub struct ParsingContextStack {
    /// The stack of active parsing contexts.
    /// Index 0 is always `TopLevel`, and the last element is the current context.
    contexts: Vec<ParsingContext>,
}

impl ParsingContextStack {
    /// Creates a new context stack initialized with `TopLevel`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stack = ParsingContextStack::new();
    /// assert_eq!(stack.current(), ParsingContext::TopLevel);
    /// assert_eq!(stack.depth(), 1);
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self { contexts: vec![ParsingContext::TopLevel] }
    }

    /// Pushes a new parsing context onto the stack.
    ///
    /// # Parameters
    ///
    /// - `ctx`: The context to push
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// stack.push(ParsingContext::Parameters);
    /// assert_eq!(stack.current(), ParsingContext::Parameters);
    /// ```
    #[inline]
    pub fn push(&mut self, ctx: ParsingContext) {
        self.contexts.push(ctx);
    }

    /// Pops the top context from the stack.
    ///
    /// Returns `Some(context)` if a context was popped, or `None` if only `TopLevel` remains.
    /// This protects the invariant that `TopLevel` is never removed.
    ///
    /// # Returns
    ///
    /// - `Some(ParsingContext)` - The popped context
    /// - `None` - If the stack only contains `TopLevel` (protected from popping)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// stack.push(ParsingContext::Parameters);
    /// let popped = stack.pop();
    /// assert_eq!(popped, Some(ParsingContext::Parameters));
    ///
    /// // Cannot pop TopLevel
    /// let popped = stack.pop();
    /// assert_eq!(popped, None);
    /// ```
    #[inline]
    pub fn pop(&mut self) -> Option<ParsingContext> {
        if self.contexts.len() <= 1 {
            // Protect TopLevel from being popped
            debug_assert_eq!(
                self.contexts[0],
                ParsingContext::TopLevel,
                "First context must always be TopLevel"
            );
            return None;
        }

        self.contexts.pop()
    }

    /// Returns the current (top) parsing context.
    ///
    /// This never fails because the stack always contains at least `TopLevel`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(stack.current(), ParsingContext::TopLevel);
    /// stack.push(ParsingContext::Parameters);
    /// assert_eq!(stack.current(), ParsingContext::Parameters);
    /// ```
    #[inline]
    pub(crate) fn current(&self) -> ParsingContext {
        *self.contexts.last().expect("Context stack should never be empty")
    }

    /// Checks if a specific context is currently active in the stack.
    ///
    /// This searches the entire stack, not just the top.
    ///
    /// # Parameters
    ///
    /// - `ctx`: The context to search for
    ///
    /// # Returns
    ///
    /// `true` if the context is anywhere in the stack, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// stack.push(ParsingContext::ClassMembers);
    /// stack.push(ParsingContext::FunctionBody);
    ///
    /// assert!(stack.is_in_context(ParsingContext::TopLevel));
    /// assert!(stack.is_in_context(ParsingContext::ClassMembers));
    /// assert!(stack.is_in_context(ParsingContext::FunctionBody));
    /// assert!(!stack.is_in_context(ParsingContext::Parameters));
    /// ```
    #[inline]
    pub(crate) fn is_in_context(&self, ctx: ParsingContext) -> bool {
        self.contexts.contains(&ctx)
    }

    /// Returns a slice of all active contexts from bottom to top.
    ///
    /// The first element is always `TopLevel`, and the last is the current context.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let contexts = stack.active_contexts();
    /// assert_eq!(contexts[0], ParsingContext::TopLevel);
    /// assert_eq!(contexts[contexts.len() - 1], stack.current());
    /// ```
    #[inline]
    pub(crate) fn active_contexts(&self) -> &[ParsingContext] {
        &self.contexts
    }

    /// Returns the current depth of the context stack.
    ///
    /// The depth is always at least 1 (for `TopLevel`).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let stack = ParsingContextStack::new();
    /// assert_eq!(stack.depth(), 1);
    ///
    /// stack.push(ParsingContext::Parameters);
    /// assert_eq!(stack.depth(), 2);
    /// ```
    #[cfg_attr(not(test), expect(dead_code, reason = "M6.5: Will be used in Step 3 for error recovery"))]
    #[inline]
    pub(crate) fn depth(&self) -> usize {
        self.contexts.len()
    }
}

impl Default for ParsingContextStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_stack_initialization() {
        let stack = ParsingContextStack::new();
        assert_eq!(stack.current(), ParsingContext::TopLevel);
        assert_eq!(stack.depth(), 1);
        assert!(stack.is_in_context(ParsingContext::TopLevel));
    }

    #[test]
    fn test_context_stack_push_pop() {
        let mut stack = ParsingContextStack::new();

        stack.push(ParsingContext::Parameters);
        assert_eq!(stack.current(), ParsingContext::Parameters);
        assert_eq!(stack.depth(), 2);

        let popped = stack.pop();
        assert_eq!(popped, Some(ParsingContext::Parameters));
        assert_eq!(stack.current(), ParsingContext::TopLevel);
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_context_stack_toplevel_protection() {
        let mut stack = ParsingContextStack::new();

        // Try to pop TopLevel - should be protected
        let popped = stack.pop();
        assert_eq!(popped, None);
        assert_eq!(stack.current(), ParsingContext::TopLevel);
        assert_eq!(stack.depth(), 1);

        // TopLevel should still be there
        assert!(stack.is_in_context(ParsingContext::TopLevel));
    }

    #[test]
    fn test_context_stack_nested() {
        let mut stack = ParsingContextStack::new();

        // Build nested contexts
        stack.push(ParsingContext::ClassMembers);
        stack.push(ParsingContext::FunctionBody);
        stack.push(ParsingContext::Parameters);

        // Check depth
        assert_eq!(stack.depth(), 4);
        assert_eq!(stack.current(), ParsingContext::Parameters);

        // Check all contexts are active
        assert!(stack.is_in_context(ParsingContext::TopLevel));
        assert!(stack.is_in_context(ParsingContext::ClassMembers));
        assert!(stack.is_in_context(ParsingContext::FunctionBody));
        assert!(stack.is_in_context(ParsingContext::Parameters));

        // Check contexts not in stack
        assert!(!stack.is_in_context(ParsingContext::TypeMembers));
        assert!(!stack.is_in_context(ParsingContext::ArrayLiteralMembers));

        // Pop and verify
        assert_eq!(stack.pop(), Some(ParsingContext::Parameters));
        assert_eq!(stack.current(), ParsingContext::FunctionBody);
        assert!(!stack.is_in_context(ParsingContext::Parameters));

        assert_eq!(stack.pop(), Some(ParsingContext::FunctionBody));
        assert_eq!(stack.current(), ParsingContext::ClassMembers);

        assert_eq!(stack.pop(), Some(ParsingContext::ClassMembers));
        assert_eq!(stack.current(), ParsingContext::TopLevel);
    }

    #[test]
    fn test_context_stack_active_contexts() {
        let mut stack = ParsingContextStack::new();

        stack.push(ParsingContext::ClassMembers);
        stack.push(ParsingContext::FunctionBody);

        let active = stack.active_contexts();
        assert_eq!(active.len(), 3);
        assert_eq!(active[0], ParsingContext::TopLevel);
        assert_eq!(active[1], ParsingContext::ClassMembers);
        assert_eq!(active[2], ParsingContext::FunctionBody);
    }

    #[test]
    fn test_context_stack_push_pop_balance() {
        let mut stack = ParsingContextStack::new();

        // Simulate balanced push/pop in parsing
        stack.push(ParsingContext::BlockStatements);
        stack.push(ParsingContext::Parameters);
        stack.pop();
        stack.pop();

        // Should be back to initial state
        assert_eq!(stack.current(), ParsingContext::TopLevel);
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn test_context_stack_multiple_same_context() {
        let mut stack = ParsingContextStack::new();

        // Nested functions can have same context type
        stack.push(ParsingContext::FunctionBody);
        stack.push(ParsingContext::FunctionBody);
        stack.push(ParsingContext::FunctionBody);

        assert_eq!(stack.depth(), 4);
        assert_eq!(stack.current(), ParsingContext::FunctionBody);

        // is_in_context returns true if any instance exists
        assert!(stack.is_in_context(ParsingContext::FunctionBody));

        // Pop all function bodies
        stack.pop();
        stack.pop();
        stack.pop();

        assert_eq!(stack.current(), ParsingContext::TopLevel);
    }

    #[test]
    fn test_parsing_context_enum_all_variants() {
        // Ensure all variants are covered
        let contexts = [
            ParsingContext::TopLevel,
            ParsingContext::BlockStatements,
            ParsingContext::FunctionBody,
            ParsingContext::Parameters,
            ParsingContext::ArgumentExpressions,
            ParsingContext::TypeAnnotation,
            ParsingContext::TypeMembers,
            ParsingContext::ClassMembers,
            ParsingContext::EnumMembers,
            ParsingContext::ObjectLiteralMembers,
            ParsingContext::ArrayLiteralMembers,
            ParsingContext::SwitchClauses,
            ParsingContext::ImportSpecifiers,
            ParsingContext::ExportSpecifiers,
            ParsingContext::TypeParameters,
            ParsingContext::TypeArguments,
            ParsingContext::JsxAttributes,
            ParsingContext::JsxChildren,
        ];

        // Verify all contexts can be pushed/popped
        let mut stack = ParsingContextStack::new();
        for ctx in contexts {
            stack.push(ctx);
            assert_eq!(stack.current(), ctx);
            assert!(stack.is_in_context(ctx));
        }

        // Pop all (except TopLevel)
        for _i in 0..contexts.len() {
            stack.pop();
        }
        assert_eq!(stack.current(), ParsingContext::TopLevel);
    }

    #[test]
    fn test_context_stack_default() {
        let stack = ParsingContextStack::default();
        assert_eq!(stack.current(), ParsingContext::TopLevel);
        assert_eq!(stack.depth(), 1);
    }
}
