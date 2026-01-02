use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

fn main() {
    println!("=== Debugging const enum Case ===\n");

    // Test each statement separately
    println!("Test 1: let class = 1;");
    test_case("let class = 1;");

    println!("\nTest 2: const enum = 2;");
    test_case("const enum = 2;");

    println!("\nTest 3: var return = 3;");
    test_case("var return = 3;");

    println!("\nTest 4: const x = 2;");
    test_case("const x = 2;");

    println!("\nTest 5: All together with line breaks");
    test_case("let class = 1;\nconst enum = 2;\nvar return = 3;\nlet valid = 4;");
}

fn test_case(source: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions { recover_from_errors: true, ..ParseOptions::default() };
    let result = Parser::new(&allocator, source, source_type).with_options(options).parse();

    println!("  Errors: {}", result.errors.len());
    for (i, error) in result.errors.iter().enumerate() {
        println!("    {}: {}", i + 1, error.message);
    }
    println!("  Statements: {}", result.program.body.len());
}
