# OXC Patches for tstc

This document describes all modifications made to the OXC parser for tstc's requirements.

## Overview

**Patches Applied**: 7 locations
**File Modified**: `crates/oxc_parser/src/js/grammar.rs`
**Purpose**: Convert fatal errors to recoverable errors for invalid assignment targets
**Date**: 2025-12-27
**OXC Version**: 0.105.0

## Rationale

TypeScript compiler (tsc) reports **ALL** syntax errors in a file, not just the first one. OXC's `fatal_error()` method terminates parsing immediately, preventing subsequent error detection.

**Example**: Test file `typescript/tests/cases/conformance/expressions/assignmentLHSIsValue.ts`
- Contains 39 invalid assignment targets
- **Without patches**: OXC reports 1 error, parser terminates
- **With patches**: OXC reports all 39 errors, parsing continues

**Impact on tstc**: Unblocks M1.6 Phase 1 completion, achieves 120/120 M1 tests (100%)

## Patch Strategy

### Pattern

**BEFORE** (fatal error, terminates parsing):
```rust
_ => p.fatal_error(diagnostics::invalid_assignment(expr.span()))
```

**AFTER** (recoverable error, continues parsing):
```rust
_ => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    // Return dummy identifier to allow parsing to continue
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

### Why Dummy Node?

- Parser expects a return value of type `SimpleAssignmentTarget`
- Must return something to continue parsing beyond the error
- Dummy identifier (`__invalid_assign_target__`) allows parser to proceed
- Type checker will later validate and report proper TS2364 error

## Detailed Patch Locations

### File: `crates/oxc_parser/src/js/grammar.rs`

#### Patch 1: ParenthesizedExpression with Object/Array (Line 42)

**Context**: `Expression::ParenthesizedExpression`
**Error**: Invalid assignment to object/array expression inside parentheses
**Example**: `(obj) = value` or `([1, 2]) = value`

```rust
// Original (line 42):
p.fatal_error(diagnostics::invalid_assignment(span))

// Patched:
p.error(diagnostics::invalid_assignment(span));
SimpleAssignmentTarget::AssignmentTargetIdentifier(
    p.ast.alloc(p.ast.identifier_reference(span, p.ast.atom("__invalid_assign_target__")))
)
```

---

#### Patch 2: TSAsExpression Invalid Target (Line 58)

**Context**: `Expression::TSAsExpression` with invalid inner expression
**Error**: Type assertion on non-assignable expression
**Example**: `(1 as any) = value`

```rust
// Original (line 54):
_ => p.fatal_error(diagnostics::invalid_assignment(expr.span())),

// Patched (line 58):
_ => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

---

#### Patch 3: TSSatisfiesExpression Invalid Target (Line 74)

**Context**: `Expression::TSSatisfiesExpression` with invalid inner expression
**Error**: Satisfies expression on non-assignable expression
**Example**: `(1 satisfies number) = value`

```rust
// Original (line 64):
_ => p.fatal_error(diagnostics::invalid_assignment(expr.span())),

// Patched (line 74):
_ => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

---

#### Patch 4: TSNonNullExpression Invalid Target (Line 90)

**Context**: `Expression::TSNonNullExpression` with invalid inner expression
**Error**: Non-null assertion on non-assignable expression
**Example**: `(obj.prop!) = value` where obj.prop is not assignable

```rust
// Original (line 74):
_ => p.fatal_error(diagnostics::invalid_assignment(expr.span())),

// Patched (line 90):
_ => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

---

#### Patch 5: TSTypeAssertion Invalid Target (Line 105)

**Context**: `Expression::TSTypeAssertion` with invalid inner expression
**Error**: Type assertion (angle bracket syntax) on non-assignable expression
**Example**: `<any>1 = value`

```rust
// Original (line 83):
_ => p.fatal_error(diagnostics::invalid_assignment(expr.span())),

// Patched (line 105):
_ => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

---

#### Patch 6: TSInstantiationExpression (Line 114)

**Context**: `Expression::TSInstantiationExpression`
**Error**: Generic instantiation expression as assignment LHS
**Example**: `foo<number> = value`

```rust
// Original (line 86):
p.fatal_error(diagnostics::invalid_lhs_assignment(expr.span()))

// Patched (line 114):
p.error(diagnostics::invalid_lhs_assignment(expr.span()));
SimpleAssignmentTarget::AssignmentTargetIdentifier(
    p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
)
```

---

#### Patch 7: Catch-All Invalid Expression (Line 121)

**Context**: Default case for any other invalid expression
**Error**: Any expression type not explicitly handled above
**Example**: `this = value`, `null = value`, `true = value`, `42 = value`

```rust
// Original (line 88):
expr => p.fatal_error(diagnostics::invalid_assignment(expr.span())),

// Patched (line 121):
expr => {
    p.error(diagnostics::invalid_assignment(expr.span()));
    SimpleAssignmentTarget::AssignmentTargetIdentifier(
        p.ast.alloc(p.ast.identifier_reference(expr.span(), p.ast.atom("__invalid_assign_target__")))
    )
}
```

## Testing

### Test Case: `assignmentLHSIsValue.ts`

**Location**: `typescript/tests/cases/conformance/expressions/assignmentLHSIsValue.ts`

**Contents** (excerpt):
```typescript
var value: any;

// this
this = value;

// literals
null = value;
true = value;
false = value;
0 = value;
'' = value;

// More expressions...
// (39 invalid assignments total)
```

**Expected Behavior**:
- All 39 invalid assignments reported
- Parser completes parsing entire file
- Type checker reports TS2364 for each invalid target

### Verification

```bash
# Build OXC with patches
cd crates/tstc-parser/oxc
cargo build

# Run tstc tests
cd /Users/rohitbhosle/project/personal/tstc
cargo run --bin test-runner --release -- --milestone 1

# Expected result: 120/120 tests passing (100%)
```

## Upstream Contribution

### Potential Approaches

1. **Lenient Mode Option**: Add `ParseOptions::lenient` flag
   ```rust
   pub struct ParseOptions {
       pub lenient: bool,  // NEW: Allow error recovery for more cases
       // ... existing options
   }
   ```

2. **Error Recovery Policy**: Make assignment target validation recoverable by default
   - Rationale: Other parsers (Babel, TypeScript) recover from these errors
   - Benefit: Better error reporting for users

3. **Per-Error Recovery Control**: Fine-grained control over which errors are fatal
   ```rust
   pub enum ErrorRecovery {
       Fatal,        // Terminate parsing
       Recoverable,  // Continue parsing
   }
   ```

### Filing Upstream Issue

**Title**: "Add error recovery for invalid assignment targets"

**Description**:
> Currently, OXC's parser uses `fatal_error()` for invalid assignment targets, which terminates parsing immediately. This prevents reporting multiple errors in a file.
>
> TypeScript compiler (tsc) and other parsers (Babel) report all errors and continue parsing. This is important for providing comprehensive error feedback to users.
>
> **Proposal**: Convert `fatal_error()` to `error()` for invalid assignment target validation, with dummy node generation to allow parsing to continue.
>
> **Benefits**:
> - Better error reporting (all errors shown, not just first)
> - Consistent with tsc behavior
> - Improved developer experience
>
> **Implementation**: See patch in [tstc fork](https://github.com/ChicK00o/oxc/tree/tstc-dev)

## Maintenance

### Syncing with Upstream

When pulling updates from upstream OXC:

```bash
cd crates/tstc-parser/oxc

# Fetch upstream changes
git fetch upstream

# Update main branch
git checkout main
git pull upstream main
git push origin main

# Rebase our patches on updated main
git checkout tstc-dev
git rebase main

# If conflicts in grammar.rs:
# 1. Check if upstream modified same lines
# 2. Re-apply our patches (7 locations)
# 3. Update this PATCHES.md with new line numbers
# 4. Test thoroughly

git push origin tstc-dev --force-with-lease
```

### Line Number Updates

If upstream changes `grammar.rs`:
1. Search for `SimpleAssignmentTarget::cover`
2. Locate all 7 error cases
3. Update line numbers in this document
4. Verify patches are still applied correctly

## Impact Summary

**Before Patches**:
- ❌ 112/120 M1 tests passing (93.3%)
- ❌ `assignmentLHSIsValue.ts` reports 1 error (expects 39)
- ❌ Parser terminates on first invalid assignment

**After Patches**:
- ✅ 120/120 M1 tests passing (100%)
- ✅ `assignmentLHSIsValue.ts` reports all 39 errors
- ✅ Parser continues and reports all errors

**Performance Impact**: None (error handling is not on hot path)

## References

- **OXC Repository**: https://github.com/oxc-project/oxc
- **Our Fork**: https://github.com/ChicK00o/oxc (branch: `tstc-dev`)
- **tstc Parser Architecture**: `/docs/parser-architecture.md`
- **M1.5.1 Milestone**: `/docs/milestones/todos/M1.5.1-custom-parser.md`
- **TypeScript Test Suite**: `typescript/tests/cases/conformance/expressions/assignmentLHSIsValue.ts`
