// Test the truncate_text function with multi-byte characters
fn truncate_text(text: &str, max_chars: usize) -> String {
    // Count actual characters, not bytes
    let char_count = text.chars().count();
    
    if char_count <= max_chars {
        text.to_string()
    } else {
        // Reserve space for "..."
        let target_chars = max_chars.saturating_sub(3);
        
        // Collect characters up to the target count
        let mut chars_collected = 0;
        let mut byte_index = 0;
        
        for (i, ch) in text.char_indices() {
            if chars_collected >= target_chars {
                byte_index = i;
                break;
            }
            chars_collected += 1;
        }
        
        // If we didn't break early, use the full string length
        if chars_collected < target_chars {
            byte_index = text.len();
        }
        
        // Try to break at a space for better readability
        let truncated = &text[..byte_index];
        if let Some(space_pos) = truncated.rfind(' ') {
            // Only use the space if it's not too far back (at least halfway)
            let space_chars = text[..space_pos].chars().count();
            if space_chars > target_chars / 2 {
                return format!("{}...", &text[..space_pos]);
            }
        }
        
        format!("{}...", truncated)
    }
}

fn main() {
    // Test case from the panic - truncating at character 50 which was in the middle of '"'
    let test_str = r#"The story of the town and people who live above "The Loop," a machine built to unlock and explore the mysteries of the universe – making things possible that were previously relegated only to science fiction."#;
    
    println!("Original string ({} chars, {} bytes):", test_str.chars().count(), test_str.len());
    println!("{:?}", test_str);
    println!();
    
    // Test truncating at 50 characters (the original panic point)
    let truncated = truncate_text(test_str, 50);
    println!("Truncated to 50 chars:");
    println!("{:?}", truncated);
    println!("Result: {} chars, {} bytes", truncated.chars().count(), truncated.len());
    println!();
    
    // Show the bytes around position 50
    println!("Bytes around position 50:");
    for (i, byte) in test_str.bytes().enumerate() {
        if i >= 45 && i <= 55 {
            println!("  Byte {}: {:3} ({:?})", i, byte, byte as char);
        }
    }
    println!();
    
    // Test other multi-byte characters
    let test_cases = vec![
        ("Hello 世界!", 8),
        ("Emoji test 😀🎉🚀", 12),
        ("Mixed: café ñoño", 10),
        (""Fancy quotes" and —em dashes—", 20),
    ];
    
    for (text, max_chars) in test_cases {
        let result = truncate_text(text, max_chars);
        println!("Input: {:?} (truncate to {})", text, max_chars);
        println!("Output: {:?}", result);
        println!();
    }
}