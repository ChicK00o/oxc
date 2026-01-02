use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Debugging Reserved Word Cases ===\n");

    // Simple case that works
    println!("Test A: Simple let with reserved word");
    test_case("let import = 5;");

    // Test const separately
    println!("\nTest B: const with non-reserved word");
    test_case("const x = 1;");

    // Test const with reserved word (simplified)
    println!("\nTest C: const with reserved word");
    test_case("const class = 1;");

    // Test var with reserved word
    println!("\nTest D: var with reserved word");
    test_case("var return = 3;");

    // Test expression with reserved word
    println!("\nTest E: Reserved word in expression");
    test_case("let x = 5; let y = import;");

    // Test reserved word in addition
    println!("\nTest F: Reserved word in addition (simplified)");
    test_case("let y = 5 + 10;");

    println!("\nTest G: Reserved word in addition");
    test_case("let x = 5 + import;");
}

fn test_case(source: &str) {
    println!("  Source: {}", source.trim());

    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
    let result = Parser::new(&allocator, source, source_type).with_options(options).parse();

    println!("  Errors: {}", result.errors.len());
    for error in &result.errors {
        println!("    - {}", error.message);
    }
    println!("  Statements: {}", result.program.body.len());
}
