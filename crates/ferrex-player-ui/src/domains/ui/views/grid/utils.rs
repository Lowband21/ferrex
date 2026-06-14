/// Truncate text to fit within a given width with ellipsis
pub fn truncate_text(text: &str, max_chars: usize) -> String {
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

        for (i, _) in text.char_indices() {
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
