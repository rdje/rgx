use rgx_core::Regex;

fn main() {
    println!("Testing RGX alternation patterns...\n");
    
    // Test simple alternation
    let pattern = "cat|dog";
    println!("Pattern: {}", pattern);
    
    if let Ok(regex) = Regex::compile(pattern) {
        let test_cases = vec![
            "cat",        // Should match
            "dog",        // Should match  
            "bird",       // Should not match
            "cat and dog", // Should match (cat)
        ];
        
        for text in test_cases {
            let result = regex.is_match(text);
            println!("  '{}' -> {}", text, result);
        }
    } else {
        println!("  Failed to compile pattern");
    }
    
    println!();
    
    // Test more complex alternation
    let pattern2 = "foo|bar|baz";
    println!("Pattern: {}", pattern2);
    
    if let Ok(regex2) = Regex::compile(pattern2) {
        let test_cases2 = vec![
            "foo",
            "bar", 
            "baz",
            "qux",
        ];
        
        for text in test_cases2 {
            let result = regex2.is_match(text);
            println!("  '{}' -> {}", text, result);
        }
    } else {
        println!("  Failed to compile pattern");
    }
}
