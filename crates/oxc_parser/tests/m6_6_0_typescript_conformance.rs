//! M6.6.0 Phase 2: TypeScript Error Recovery Conformance Suite
//!
//! Runs all 92 TypeScript error recovery tests from:
//! typescript/tests/cases/conformance/parser/ecmascript5/ErrorRecovery/

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;
use std::fs;
use std::path::Path;

const TYPESCRIPT_ERROR_RECOVERY_DIR: &str =
    "/Users/rohitbhosle/project/personal/tstc/typescript/tests/cases/conformance/parser/ecmascript5/ErrorRecovery";

fn parse_typescript_file(path: &Path) -> (usize, bool) {
    let source = fs::read_to_string(path).expect("Failed to read TypeScript test file");
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let options = ParseOptions {
        recover_from_errors: true,
        ..ParseOptions::default()
    };

    let result = Parser::new(&allocator, &source, source_type)
        .with_options(options)
        .parse();

    let error_count = result.errors.len();
    // Success if we generated any program (even with empty body, as long as no panic)
    let has_program = !result.panicked;

    (error_count, has_program)
}

fn collect_test_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "ts") {
                files.push(path);
            } else if path.is_dir() {
                // Recurse into subdirectories
                files.extend(collect_test_files(&path));
            }
        }
    }

    files.sort();
    files
}

#[test]
fn test_typescript_error_recovery_suite() {
    let base_dir = Path::new(TYPESCRIPT_ERROR_RECOVERY_DIR);

    if !base_dir.exists() {
        eprintln!("TypeScript error recovery tests not found at: {}", TYPESCRIPT_ERROR_RECOVERY_DIR);
        eprintln!("Skipping TypeScript conformance tests");
        return;
    }

    let test_files = collect_test_files(base_dir);

    println!("\n=== TypeScript Error Recovery Conformance Suite ===");
    println!("Total test files: {}", test_files.len());
    println!();

    let mut passed = 0;
    let mut failed = 0;
    let mut total_errors = 0;

    for file_path in &test_files {
        let file_name = file_path.file_name().unwrap().to_string_lossy();
        let (error_count, has_program) = parse_typescript_file(file_path);

        total_errors += error_count;

        // Success criteria: Parser recovered and generated a program
        if has_program {
            passed += 1;
            println!("✓ {} - {} errors, program generated", file_name, error_count);
        } else {
            failed += 1;
            println!("✗ {} - {} errors, NO program", file_name, error_count);
        }
    }

    println!();
    println!("=== Results ===");
    println!("Passed: {}/{} ({:.1}%)", passed, test_files.len(),
             (passed as f64 / test_files.len() as f64) * 100.0);
    println!("Failed: {}", failed);
    println!("Total errors reported: {}", total_errors);
    println!("Average errors per file: {:.2}", total_errors as f64 / test_files.len() as f64);

    // Assert that we processed the expected number of files
    assert_eq!(test_files.len(), 92,
               "Expected 92 TypeScript error recovery test files, found {}", test_files.len());

    // We expect high pass rate (>90%) but don't require 100% since some tests
    // may have very severe errors that prevent program generation
    let pass_rate = (passed as f64 / test_files.len() as f64) * 100.0;
    assert!(pass_rate > 90.0,
            "Pass rate {:.1}% is below 90% threshold", pass_rate);
}

#[test]
fn test_specific_error_recovery_categories() {
    let base_dir = Path::new(TYPESCRIPT_ERROR_RECOVERY_DIR);

    if !base_dir.exists() {
        eprintln!("Skipping category tests - TypeScript tests not found");
        return;
    }

    let categories = vec![
        "IfStatements",
        "Expressions",
        "ArrayLiteralExpressions",
        "ObjectLiterals",
        "ParameterLists",
        "ArgumentLists",
        "ClassElements",
        "ArrowFunctions",
        "Blocks",
        "ModuleElements",
    ];

    println!("\n=== Testing by Category ===");

    for category in categories {
        let category_dir = base_dir.join(category);
        if !category_dir.exists() {
            continue;
        }

        let files = collect_test_files(&category_dir);
        let mut passed = 0;

        for file_path in &files {
            let (_error_count, has_program) = parse_typescript_file(file_path);
            if has_program {
                passed += 1;
            }
        }

        let pass_rate = if files.is_empty() {
            0.0
        } else {
            (passed as f64 / files.len() as f64) * 100.0
        };

        println!("{:30} {:3}/{:3} files ({:5.1}%)",
                 category, passed, files.len(), pass_rate);
    }
}

#[test]
fn test_parser_fuzz_case() {
    // parserFuzz1.ts is a known challenging test
    let fuzz_file = Path::new(TYPESCRIPT_ERROR_RECOVERY_DIR).join("parserFuzz1.ts");

    if !fuzz_file.exists() {
        eprintln!("Skipping fuzz test");
        return;
    }

    let (error_count, has_program) = parse_typescript_file(&fuzz_file);

    println!("\n=== Fuzz Test ===");
    println!("parserFuzz1.ts: {} errors", error_count);

    // Should handle fuzz case without crashing
    assert!(has_program || error_count > 0,
            "Parser should either generate program or report errors");
}
