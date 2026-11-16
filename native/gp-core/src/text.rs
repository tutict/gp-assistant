pub(crate) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| {
        let needle = needle.to_lowercase();
        if needle.is_ascii()
            && needle
                .chars()
                .any(|character| character.is_ascii_alphabetic())
        {
            // Match English keywords as words, while allowing adjacent Chinese text.
            text.match_indices(&needle).any(|(start, matched)| {
                let word_character =
                    |character: char| character.is_ascii_alphanumeric() || character == '_';
                text[..start]
                    .chars()
                    .next_back()
                    .is_none_or(|character| !word_character(character))
                    && text[start + matched.len()..]
                        .chars()
                        .next()
                        .is_none_or(|character| !word_character(character))
            })
        } else {
            text.contains(&needle)
        }
    })
}
