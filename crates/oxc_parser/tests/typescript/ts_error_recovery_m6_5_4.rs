//! Tests for M6.5.4: TypeScript-specific error recovery
//! Tests index signatures, enum members, and using declarations

use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParserReturn};
use oxc_span::SourceType;

fn parse_with_recovery(source: &str) -> ParserReturn {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let mut parser = Parser::new(&allocator, source, source_type);
    parser.options.recover_from_errors = true;
    parser.parse()
}

// ==================== Index Signature Tests ====================

#[test]
fn test_index_signature_missing_type_annotation() {
    let source = r#"
        interface Config {
            [key: string]
            other: string;
            value: number;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for missing type annotation
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("type annotation"));

    // But program should be parsed successfully
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_index_signature_with_valid_annotation() {
    let source = r#"
        interface Config {
            [key: string]: any;
            other: string;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have no errors
    assert_eq!(result.errors.len(), 0);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_multiple_index_signatures_with_errors() {
    let source = r#"
        interface MultiIndex {
            [stringKey: string]
            [numberKey: number]
            property: string;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 2 errors (one for each missing type annotation)
    assert_eq!(result.errors.len(), 2);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_readonly_index_signature_error() {
    let source = r#"
        interface ReadonlyIndex {
            readonly [key: string]
            other: number;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for missing type annotation
    assert_eq!(result.errors.len(), 1);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_nested_index_signature_errors() {
    let source = r#"
        interface Nested {
            [outer: string]: {
                [inner: string]
                value: number;
            }
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error (inner index signature missing type)
    assert_eq!(result.errors.len(), 1);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_type_literal_index_signature_error() {
    let source = r#"
        type T = {
            [key: string]
            other: boolean;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for missing type annotation
    assert_eq!(result.errors.len(), 1);
    assert!(!result.program.body.is_empty());
}

// ==================== Enum Member Tests ====================

#[test]
fn test_enum_numeric_decimal_member() {
    let source = r#"
        enum Numbers {
            123 = "test",
            Valid = "success",
            456 = "another"
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 2 errors (for 123 and 456)
    assert_eq!(result.errors.len(), 2);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_numeric_hex_member() {
    let source = r#"
        enum HexValues {
            0xFF = 255,
            Valid = 0,
            0x10 = 16
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 2 errors (for 0xFF and 0x10)
    assert_eq!(result.errors.len(), 2);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_numeric_binary_member() {
    let source = r#"
        enum BinaryValues {
            0b1010 = 10,
            Valid = 5
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error (for 0b1010)
    assert_eq!(result.errors.len(), 1);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_computed_property() {
    let source = r#"
        enum Computed {
            [x]: 1,
            Valid: 2,
            [y + z]: 3
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 2 errors (for computed properties)
    assert_eq!(result.errors.len(), 2);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_template_literal() {
    let source = r#"
        enum Templates {
            `simple`: 1,
            Valid: 2
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error (for template literal)
    assert_eq!(result.errors.len(), 1);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_mixed_errors() {
    let source = r#"
        enum Mixed {
            123 = 1,
            [computed]: 2,
            `template`: 3,
            Valid: 4,
            0xFF = 5
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have 4 errors (numeric, computed, template, hex)
    assert_eq!(result.errors.len(), 4);
    assert!(!result.program.body.is_empty());
}

#[test]
fn test_enum_valid_members() {
    let source = r#"
        enum ValidEnum {
            A = 1,
            B = 2,
            C = "string"
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have no errors
    assert_eq!(result.errors.len(), 0);
    assert!(!result.program.body.is_empty());
}

// ==================== Using Declaration Tests ====================

#[test]
fn test_using_declaration_export() {
    let source = r#"
        export using resource = getResource();
        let x = 5;
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for export using
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("exported") || result.errors[0].message.contains("using"));

    // Should have 2 statements (using declaration + let)
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_await_using_declaration_export() {
    let source = r#"
        export await using asyncResource = getAsyncResource();
        const y = 10;
    "#;

    let result = parse_with_recovery(source);

    // Should have 1 error for export await using
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("exported") || result.errors[0].message.contains("using"));

    // Should have 2 statements
    assert_eq!(result.program.body.len(), 2);
}

#[test]
fn test_multiple_using_errors() {
    let source = r#"
        export using r1 = get1();
        export await using r2 = get2();
        let valid = 3;
    "#;

    let result = parse_with_recovery(source);

    // Should have 2 errors (one for each export using)
    assert_eq!(result.errors.len(), 2);

    // Should have 3 statements
    assert_eq!(result.program.body.len(), 3);
}

#[test]
fn test_using_declaration_valid() {
    let source = r#"
        using resource = getResource();
        await using asyncResource = getAsync();
    "#;

    let result = parse_with_recovery(source);

    // Should have no errors (using without export is valid)
    assert_eq!(result.errors.len(), 0);
    assert_eq!(result.program.body.len(), 2);
}

// ==================== Integration Tests ====================

#[test]
fn test_all_typescript_errors_combined() {
    let source = r#"
        interface Config {
            [key: string]
            valid1: string;
        }

        enum Status {
            123 = "numeric",
            Valid = "ok"
        }

        export using resource = getResource();

        let x = 5;
    "#;

    let result = parse_with_recovery(source);

    // Should have 3 errors (index sig, enum member, using export)
    assert_eq!(result.errors.len(), 3);

    // Should have 4 top-level items (interface, enum, export, let)
    assert_eq!(result.program.body.len(), 4);
}

#[test]
fn test_recovery_continues_after_errors() {
    let source = r#"
        interface Bad {
            [k: string]
            [k2: number]
            good: string;
        }

        enum AlsoBad {
            0xFF,
            GoodMember
        }

        type T = {
            [x: string]
            prop: number;
        }
    "#;

    let result = parse_with_recovery(source);

    // Should have multiple errors but all constructs parsed
    assert!(result.errors.len() >= 3);
    assert_eq!(result.program.body.len(), 3);
}

#[test]
fn test_recovery_without_flag() {
    let source = r#"
        interface Config {
            [key: string]
            other: string;
        }
    "#;

    // Parse WITHOUT recovery flag
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let mut parser = Parser::new(&allocator, source, source_type);
    parser.options.recover_from_errors = false;
    let result = parser.parse();

    // Should still report error
    assert!(result.errors.len() >= 1);
}
