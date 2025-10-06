use rgx_core::Regex;

fn main() {
    println!("Testing character class support...\n");
    
    // Test 1: Basic character class
    let re = Regex::compile("[abc]").expect("Failed to compile [abc]");
    let text = "def abc xyz";
    let matches = re.find_all(text);
    println!("Pattern: [abc]");
    println!("Text: {}", text);
    println!("Matches: {:?}", matches);
    println!();
    
    // Test 2: Range character class
    let re2 = Regex::compile("[0-9]+").expect("Failed to compile [0-9]+");
    let text2 = "abc 123 def 456";
    let matches2 = re2.find_all(text2);
    println!("Pattern: [0-9]+");
    println!("Text: {}", text2);
    println!("Matches: {:?}", matches2);
    println!();
    
    // Test 3: Negated character class
    let re3 = Regex::compile("[^0-9]+").expect("Failed to compile [^0-9]+");
    let text3 = "abc123def456";
    let matches3 = re3.find_all(text3);
    println!("Pattern: [^0-9]+");
    println!("Text: {}", text3);
    println!("Matches: {:?}", matches3);
    println!();
    
    // Test 4: Mixed ranges
    let re4 = Regex::compile("[a-zA-Z]+").expect("Failed to compile [a-zA-Z]+");
    let text4 = "Hello123World";
    let matches4 = re4.find_all(text4);
    println!("Pattern: [a-zA-Z]+");
    println!("Text: {}", text4);
    println!("Matches: {:?}", matches4);
}