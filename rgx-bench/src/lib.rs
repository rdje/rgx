//! Shared benchmark fixtures for `rgx-bench`.

/// Test data sizes used by the criterion throughput suite.
pub const INPUT_SIZES: &[usize] = &[100, 1_000, 10_000, 100_000];

/// Benchmark pattern metadata shared across benchmark entry points.
#[derive(Debug, Clone, Copy)]
pub struct BenchmarkPattern {
    /// Stable benchmark identifier.
    pub name: &'static str,
    /// Regex pattern text compiled by both rgx and PCRE2.
    pub pattern: &'static str,
    /// Short human-readable description of the benchmark case.
    pub description: &'static str,
}

/// Shared benchmark pattern corpus.
pub const PATTERNS: &[BenchmarkPattern] = &[
    BenchmarkPattern {
        name: "literal_simple",
        pattern: r"test",
        description: "Simple 4-character literal match",
    },
    BenchmarkPattern {
        name: "email_basic",
        pattern: r"\b\w+@\w+\.\w+\b",
        description: "Basic email pattern with word boundaries",
    },
    BenchmarkPattern {
        name: "digit_sequence",
        pattern: r"\d{3}-\d{2}-\d{4}",
        description: "SSN-like pattern with digit quantifiers",
    },
    BenchmarkPattern {
        name: "character_class",
        pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
        description: "Email with character classes",
    },
    BenchmarkPattern {
        name: "alternation",
        pattern: r"cat|dog|bird",
        description: "Simple alternation",
    },
    BenchmarkPattern {
        name: "capture_groups",
        pattern: r"(\d{4})-(\d{2})-(\d{2})",
        description: "Date capture groups",
    },
    BenchmarkPattern {
        name: "url_simple",
        pattern: r"https?://\S+",
        description: "Simple URL detection",
    },
];

/// Generate representative input data for the given pattern and requested size.
#[must_use]
pub fn generate_test_data(size: usize, pattern: &str) -> String {
    match pattern {
        r"test" => {
            let mut data = String::with_capacity(size + 100);
            data.push_str("prefix ");
            while data.len() < size {
                data.push_str("test ");
                data.push_str("other ");
            }
            data.push_str(" suffix");
            data
        }
        r"\b\w+@\w+\.\w+\b" => {
            let mut data = String::with_capacity(size + 200);
            data.push_str("Contact info: ");
            while data.len() < size {
                data.push_str("user@example.com ");
                data.push_str("admin@test.org ");
                data.push_str("some text without emails ");
            }
            data.push_str(" end.");
            data
        }
        r"\d{3}-\d{2}-\d{4}" => {
            let mut data = String::with_capacity(size + 200);
            data.push_str("SSNs: ");
            while data.len() < size {
                data.push_str("123-45-6789 ");
                data.push_str("987-65-4321 ");
                data.push_str("random text 555-123-9999 more text ");
            }
            data.push_str(" done.");
            data
        }
        _ => {
            let mut data = String::with_capacity(size + 100);
            data.push_str("Start: ");
            while data.len() < size {
                data.push_str("sample text that might match various patterns ");
            }
            data.push_str(" :End");
            data
        }
    }
}
