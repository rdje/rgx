use rgx_core::Regex;

fn main() {
    println!("Testing character class [a-c]...\n");
    
    // Force debug output in compiler
    std::env::set_var("RUST_BACKTRACE", "1");
    
    let re = Regex::compile("[a-c]").expect("Failed to compile");
    println!("Compiled successfully!");
    
    let text = "abc";
    println!("Testing against text: {}", text);
    
    let matches = re.find_all(text);
    println!("Matches found: {:?}", matches);
    
    if re.is_match(text) {
        println!("is_match returned true");
    } else {
        println!("is_match returned false");
    }
}