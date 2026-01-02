use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Debugging Number Recovery ===\n");

    // Test individual statements
    println!("Test A: Single invalid binary");
    test_case("let a = 0b2;");

    println!("\nTest B: Invalid binary + valid statement");
    test_case("let a = 0b2;\nlet b = 10;");

    println!("\nTest C: Valid + invalid");
    test_case("let a = 1;\nlet b = 0b2;");

    println!("\nTest D: Two separate statements on same line");
    test_case("let a = 0b2; let b = 10;");

    println!("\nTest E: Valid binary");
    test_case("let a = 0b101;");
}

fn test_case(source: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
    let result = Parser::new(&allocator, source, source_type).with_options(options).parse();

    println!("  Source: {:?}", source);
    println!("  Errors: {}", result.errors.len());
    for (i, error) in result.errors.iter().enumerate() {
        println!("    {}: {}", i + 1, error.message);
    }
    println!("  Statements parsed: {}", result.program.body.len());
}
