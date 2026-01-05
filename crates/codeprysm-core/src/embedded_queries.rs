//! Embedded Tree-sitter queries for code parsing.
//!
//! This module contains all the tree-sitter query files embedded at compile time,
//! allowing the binary to work without external query files.
//!
//! ## Query Types
//!
//! - **Code Tags**: Extract code entities (functions, classes, etc.) from source files
//! - **Manifest Tags**: Extract component info and dependencies from manifest files

use crate::parser::{ManifestLanguage, SupportedLanguage};

// Base tag queries - embedded at compile time
const C_TAGS: &str = include_str!("../queries/c-tags.scm");
const CPP_TAGS: &str = include_str!("../queries/cpp-tags.scm");
const CSHARP_TAGS: &str = include_str!("../queries/csharp-tags.scm");
const GO_TAGS: &str = include_str!("../queries/go-tags.scm");
const JAVASCRIPT_TAGS: &str = include_str!("../queries/javascript-tags.scm");
const PYTHON_TAGS: &str = include_str!("../queries/python-tags.scm");
const RUST_TAGS: &str = include_str!("../queries/rust-tags.scm");
const TYPESCRIPT_TAGS: &str = include_str!("../queries/typescript-tags.scm");

// Test overlay queries - embedded at compile time
const C_TEST: &str = include_str!("../queries/overlays/c-test.scm");
const CPP_TEST: &str = include_str!("../queries/overlays/cpp-test.scm");
const CSHARP_TEST: &str = include_str!("../queries/overlays/csharp-test.scm");
const GO_TEST: &str = include_str!("../queries/overlays/go-test.scm");
const JAVASCRIPT_TEST: &str = include_str!("../queries/overlays/javascript-test.scm");
const PYTHON_TEST: &str = include_str!("../queries/overlays/python-test.scm");
const RUST_TEST: &str = include_str!("../queries/overlays/rust-test.scm");
const TYPESCRIPT_TEST: &str = include_str!("../queries/overlays/typescript-test.scm");

// Manifest tag queries - embedded at compile time
const JSON_MANIFEST_TAGS: &str = include_str!("../queries/json-manifest-tags.scm");
const TOML_MANIFEST_TAGS: &str = include_str!("../queries/toml-manifest-tags.scm");
const GOMOD_MANIFEST_TAGS: &str = include_str!("../queries/gomod-manifest-tags.scm");
const XML_MANIFEST_TAGS: &str = include_str!("../queries/xml-manifest-tags.scm");
const CMAKE_MANIFEST_TAGS: &str = include_str!("../queries/cmake-manifest-tags.scm");

/// Get the embedded query source for a language.
///
/// Returns the base tags query concatenated with any overlay queries.
pub fn get_query(language: SupportedLanguage) -> Option<String> {
    let base = get_base_query(language)?;
    let overlay = get_test_overlay(language);

    match overlay {
        Some(test_query) => Some(format!("{}\n\n{}", base, test_query)),
        None => Some(base.to_string()),
    }
}

/// Get only the base tags query for a language.
pub fn get_base_query(language: SupportedLanguage) -> Option<&'static str> {
    match language {
        SupportedLanguage::C => Some(C_TAGS),
        SupportedLanguage::Cpp => Some(CPP_TAGS),
        SupportedLanguage::CSharp => Some(CSHARP_TAGS),
        SupportedLanguage::Go => Some(GO_TAGS),
        SupportedLanguage::JavaScript => Some(JAVASCRIPT_TAGS),
        SupportedLanguage::Python => Some(PYTHON_TAGS),
        SupportedLanguage::Rust => Some(RUST_TAGS),
        SupportedLanguage::TypeScript => Some(TYPESCRIPT_TAGS),
        _ => None,
    }
}

/// Get the test overlay query for a language.
fn get_test_overlay(language: SupportedLanguage) -> Option<&'static str> {
    match language {
        SupportedLanguage::C => Some(C_TEST),
        SupportedLanguage::Cpp => Some(CPP_TEST),
        SupportedLanguage::CSharp => Some(CSHARP_TEST),
        SupportedLanguage::Go => Some(GO_TEST),
        SupportedLanguage::JavaScript => Some(JAVASCRIPT_TEST),
        SupportedLanguage::Python => Some(PYTHON_TEST),
        SupportedLanguage::Rust => Some(RUST_TEST),
        SupportedLanguage::TypeScript => Some(TYPESCRIPT_TEST),
        _ => None,
    }
}

/// Check if embedded queries are available for a language.
pub fn has_embedded_query(language: SupportedLanguage) -> bool {
    get_base_query(language).is_some()
}

/// Get a list of all languages with embedded queries.
pub fn supported_languages() -> &'static [SupportedLanguage] {
    &[
        SupportedLanguage::C,
        SupportedLanguage::Cpp,
        SupportedLanguage::CSharp,
        SupportedLanguage::Go,
        SupportedLanguage::JavaScript,
        SupportedLanguage::Python,
        SupportedLanguage::Rust,
        SupportedLanguage::TypeScript,
    ]
}

// ============================================================================
// Manifest Queries
// ============================================================================

/// Get the embedded manifest query for a manifest language.
///
/// Returns the query source for extracting component names and dependencies
/// from manifest files (package.json, Cargo.toml, etc.).
pub fn get_manifest_query(language: ManifestLanguage) -> &'static str {
    match language {
        ManifestLanguage::Json => JSON_MANIFEST_TAGS,
        ManifestLanguage::Toml => TOML_MANIFEST_TAGS,
        ManifestLanguage::GoMod => GOMOD_MANIFEST_TAGS,
        ManifestLanguage::Xml => XML_MANIFEST_TAGS,
        ManifestLanguage::CMake => CMAKE_MANIFEST_TAGS,
    }
}

/// Check if embedded manifest queries are available for a manifest language.
///
/// Always returns true since all manifest languages have embedded queries.
pub fn has_manifest_query(language: ManifestLanguage) -> bool {
    // All manifest languages have embedded queries
    match language {
        ManifestLanguage::Json
        | ManifestLanguage::Toml
        | ManifestLanguage::GoMod
        | ManifestLanguage::Xml
        | ManifestLanguage::CMake => true,
    }
}

/// Get a list of all manifest languages with embedded queries.
pub fn supported_manifest_languages() -> &'static [ManifestLanguage] {
    &[
        ManifestLanguage::Json,
        ManifestLanguage::Toml,
        ManifestLanguage::GoMod,
        ManifestLanguage::Xml,
        ManifestLanguage::CMake,
    ]
}

/// Test function for verifying incremental sync works.
/// This function exists solely to test that graph updates and search indexing work correctly.
pub fn sync_verification_test_function() -> &'static str {
    "sync_verification_success"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_queries_exist() {
        for lang in supported_languages() {
            let query = get_query(*lang);
            assert!(query.is_some(), "Query missing for {:?}", lang);
            assert!(!query.unwrap().is_empty(), "Query empty for {:?}", lang);
        }
    }

    #[test]
    fn test_base_queries_contain_definitions() {
        // All base queries should contain some form of definition pattern
        for lang in supported_languages() {
            let base = get_base_query(*lang).unwrap();
            assert!(
                base.contains("definition") || base.contains("@name"),
                "Base query for {:?} should contain definition patterns",
                lang
            );
        }
    }

    #[test]
    fn test_test_overlays_contain_test_patterns() {
        // All test overlays should contain test-related patterns
        for lang in supported_languages() {
            if let Some(overlay) = get_test_overlay(*lang) {
                assert!(
                    overlay.contains("test") || overlay.contains("Test"),
                    "Test overlay for {:?} should contain test patterns",
                    lang
                );
            }
        }
    }

    // ========================================================================
    // Manifest Query Tests
    // ========================================================================

    #[test]
    fn test_manifest_queries_exist() {
        for lang in supported_manifest_languages() {
            let query = get_manifest_query(*lang);
            assert!(!query.is_empty(), "Manifest query empty for {:?}", lang);
        }
    }

    #[test]
    fn test_manifest_queries_contain_component_patterns() {
        // All manifest queries should contain component name patterns
        for lang in supported_manifest_languages() {
            let query = get_manifest_query(*lang);
            assert!(
                query.contains("manifest.component") || query.contains("@manifest"),
                "Manifest query for {:?} should contain manifest.component patterns",
                lang
            );
        }
    }

    #[test]
    fn test_manifest_queries_contain_dependency_patterns() {
        // All manifest queries should contain dependency patterns
        for lang in supported_manifest_languages() {
            let query = get_manifest_query(*lang);
            assert!(
                query.contains("manifest.dependency"),
                "Manifest query for {:?} should contain manifest.dependency patterns",
                lang
            );
        }
    }

    #[test]
    fn test_has_manifest_query() {
        for lang in supported_manifest_languages() {
            assert!(
                has_manifest_query(*lang),
                "Should have manifest query for {:?}",
                lang
            );
        }
    }

    #[test]
    fn test_json_manifest_query_patterns() {
        let query = get_manifest_query(ManifestLanguage::Json);
        assert!(
            query.contains("package.json")
                || query.contains("vcpkg.json")
                || query.contains("JSON")
        );
        assert!(query.contains("dependencies"));
        assert!(query.contains("workspace"));
    }

    #[test]
    fn test_toml_manifest_query_patterns() {
        let query = get_manifest_query(ManifestLanguage::Toml);
        assert!(
            query.contains("Cargo.toml")
                || query.contains("pyproject.toml")
                || query.contains("TOML")
        );
        assert!(query.contains("[package]") || query.contains("package"));
        assert!(query.contains("dependencies"));
    }

    #[test]
    fn test_gomod_manifest_query_patterns() {
        let query = get_manifest_query(ManifestLanguage::GoMod);
        assert!(query.contains("go.mod") || query.contains("Go Module"));
        assert!(query.contains("module_directive") || query.contains("module_path"));
        assert!(query.contains("replace"));
    }

    #[test]
    fn test_xml_manifest_query_patterns() {
        let query = get_manifest_query(ManifestLanguage::Xml);
        assert!(query.contains(".csproj") || query.contains("XML") || query.contains("MSBuild"));
        assert!(query.contains("AssemblyName") || query.contains("assembly"));
        assert!(query.contains("ProjectReference"));
    }

    #[test]
    fn test_cmake_manifest_query_patterns() {
        let query = get_manifest_query(ManifestLanguage::CMake);
        assert!(query.contains("CMakeLists.txt") || query.contains("CMake"));
        assert!(query.contains("project"));
        assert!(query.contains("add_subdirectory"));
    }

    // ========================================================================
    // Manifest Query Compilation Tests
    // ========================================================================

    use tree_sitter::Query;

    #[test]
    fn test_manifest_queries_compile_json() {
        let query_src = get_manifest_query(ManifestLanguage::Json);
        let lang = ManifestLanguage::Json.tree_sitter_language();
        let result = Query::new(&lang, query_src);
        assert!(
            result.is_ok(),
            "JSON manifest query failed to compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_manifest_queries_compile_toml() {
        let query_src = get_manifest_query(ManifestLanguage::Toml);
        let lang = ManifestLanguage::Toml.tree_sitter_language();
        let result = Query::new(&lang, query_src);
        assert!(
            result.is_ok(),
            "TOML manifest query failed to compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_manifest_queries_compile_gomod() {
        let query_src = get_manifest_query(ManifestLanguage::GoMod);
        let lang = ManifestLanguage::GoMod.tree_sitter_language();
        let result = Query::new(&lang, query_src);
        assert!(
            result.is_ok(),
            "GoMod manifest query failed to compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_manifest_queries_compile_xml() {
        let query_src = get_manifest_query(ManifestLanguage::Xml);
        let lang = ManifestLanguage::Xml.tree_sitter_language();
        let result = Query::new(&lang, query_src);
        assert!(
            result.is_ok(),
            "XML manifest query failed to compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_manifest_queries_compile_cmake() {
        let query_src = get_manifest_query(ManifestLanguage::CMake);
        let lang = ManifestLanguage::CMake.tree_sitter_language();
        let result = Query::new(&lang, query_src);
        assert!(
            result.is_ok(),
            "CMake manifest query failed to compile: {:?}",
            result.err()
        );
    }
}
