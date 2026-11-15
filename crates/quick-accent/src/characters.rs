use std::collections::HashSet;

/// Returns the accented variants for `letter` given the active languages.
/// Characters are deduplicated preserving order across languages.
pub fn accents_for_letter(letter: char, languages: &[String]) -> Vec<char> {
    let lower = letter.to_lowercase().next().unwrap_or(letter);
    let is_upper = letter.is_uppercase();
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for lang in languages {
        if let Some(chars) = chars_for_lang(lower, lang) {
            for &ch in chars {
                let adjusted = if is_upper {
                    ch.to_uppercase().next().unwrap_or(ch)
                } else {
                    ch
                };
                if seen.insert(adjusted) {
                    result.push(adjusted);
                }
            }
        }
    }
    result
}

/// True if the given letter has any accents in the given language set.
pub fn has_accents(letter: char, languages: &[String]) -> bool {
    !accents_for_letter(letter, languages).is_empty()
}

fn chars_for_lang(letter: char, lang: &str) -> Option<&'static [char]> {
    match (lang, letter) {
        // French
        ("fr", 'a') => Some(&['à', 'â', 'æ']),
        ("fr", 'c') => Some(&['ç']),
        ("fr", 'e') => Some(&['é', 'è', 'ê', 'ë']),
        ("fr", 'i') => Some(&['î', 'ï']),
        ("fr", 'o') => Some(&['ô', 'œ']),
        ("fr", 'u') => Some(&['ù', 'û', 'ü']),
        ("fr", 'y') => Some(&['ÿ']),

        // Spanish
        ("es", 'a') => Some(&['á']),
        ("es", 'e') => Some(&['é']),
        ("es", 'i') => Some(&['í']),
        ("es", 'n') => Some(&['ñ']),
        ("es", 'o') => Some(&['ó']),
        ("es", 'u') => Some(&['ú', 'ü']),

        // German
        ("de", 'a') => Some(&['ä']),
        ("de", 'o') => Some(&['ö']),
        ("de", 'u') => Some(&['ü']),
        ("de", 's') => Some(&['ß']),

        // Portuguese
        ("pt", 'a') => Some(&['á', 'â', 'ã', 'à']),
        ("pt", 'c') => Some(&['ç']),
        ("pt", 'e') => Some(&['é', 'ê']),
        ("pt", 'i') => Some(&['í']),
        ("pt", 'o') => Some(&['ó', 'ô', 'õ']),
        ("pt", 'u') => Some(&['ú']),

        // Italian
        ("it", 'a') => Some(&['à']),
        ("it", 'e') => Some(&['è', 'é']),
        ("it", 'i') => Some(&['ì', 'í']),
        ("it", 'o') => Some(&['ò', 'ó']),
        ("it", 'u') => Some(&['ù']),

        // Dutch
        ("nl", 'a') => Some(&['á', 'ä']),
        ("nl", 'e') => Some(&['é', 'ë']),
        ("nl", 'i') => Some(&['í', 'ï']),
        ("nl", 'o') => Some(&['ó', 'ö']),
        ("nl", 'u') => Some(&['ú', 'ü']),

        // Swedish
        ("sv", 'a') => Some(&['å', 'ä']),
        ("sv", 'o') => Some(&['ö']),

        // Norwegian / Danish
        ("no" | "da", 'a') => Some(&['å', 'æ']),
        ("no" | "da", 'o') => Some(&['ø']),

        // Polish
        ("pl", 'a') => Some(&['ą']),
        ("pl", 'c') => Some(&['ć']),
        ("pl", 'e') => Some(&['ę']),
        ("pl", 'l') => Some(&['ł']),
        ("pl", 'n') => Some(&['ń']),
        ("pl", 'o') => Some(&['ó']),
        ("pl", 's') => Some(&['ś']),
        ("pl", 'z') => Some(&['ź', 'ż']),

        // Turkish
        ("tr", 'c') => Some(&['ç']),
        ("tr", 'g') => Some(&['ğ']),
        ("tr", 'i') => Some(&['ı', 'İ']),
        ("tr", 'o') => Some(&['ö']),
        ("tr", 's') => Some(&['ş']),
        ("tr", 'u') => Some(&['ü']),

        // Romanian
        ("ro", 'a') => Some(&['ă', 'â']),
        ("ro", 'i') => Some(&['î']),
        ("ro", 's') => Some(&['ș']),
        ("ro", 't') => Some(&['ț']),

        // Czech
        ("cs", 'a') => Some(&['á']),
        ("cs", 'c') => Some(&['č']),
        ("cs", 'd') => Some(&['ď']),
        ("cs", 'e') => Some(&['é', 'ě']),
        ("cs", 'i') => Some(&['í']),
        ("cs", 'n') => Some(&['ň']),
        ("cs", 'o') => Some(&['ó']),
        ("cs", 'r') => Some(&['ř']),
        ("cs", 's') => Some(&['š']),
        ("cs", 't') => Some(&['ť']),
        ("cs", 'u') => Some(&['ú', 'ů']),
        ("cs", 'y') => Some(&['ý']),
        ("cs", 'z') => Some(&['ž']),

        // Hungarian
        ("hu", 'a') => Some(&['á']),
        ("hu", 'e') => Some(&['é']),
        ("hu", 'i') => Some(&['í']),
        ("hu", 'o') => Some(&['ó', 'ö', 'ő']),
        ("hu", 'u') => Some(&['ú', 'ü', 'ű']),

        _ => None,
    }
}

/// All language codes with accent tables.
pub const LANGUAGES: &[&str] = &[
    "fr", "es", "de", "pt", "it", "nl", "sv", "no", "da", "pl", "tr", "ro", "cs", "hu",
];

/// Unicode name of `ch` for the characters this module offers (both cases).
/// Upstream `ShowUnicodeDescription`. The table is sorted by code point.
pub fn unicode_name(ch: char) -> Option<&'static str> {
    UNICODE_NAMES
        .binary_search_by_key(&ch, |(c, _)| *c)
        .ok()
        .map(|i| UNICODE_NAMES[i].1)
}

/// `U+00E9 LATIN SMALL LETTER E WITH ACUTE` style description.
pub fn unicode_description(ch: char) -> String {
    match unicode_name(ch) {
        Some(name) => format!("U+{:04X} {name}", ch as u32),
        None => format!("U+{:04X}", ch as u32),
    }
}

const UNICODE_NAMES: &[(char, &str)] = &[
    ('À', "LATIN CAPITAL LETTER A WITH GRAVE"),
    ('Á', "LATIN CAPITAL LETTER A WITH ACUTE"),
    ('Â', "LATIN CAPITAL LETTER A WITH CIRCUMFLEX"),
    ('Ã', "LATIN CAPITAL LETTER A WITH TILDE"),
    ('Ä', "LATIN CAPITAL LETTER A WITH DIAERESIS"),
    ('Å', "LATIN CAPITAL LETTER A WITH RING ABOVE"),
    ('Æ', "LATIN CAPITAL LETTER AE"),
    ('Ç', "LATIN CAPITAL LETTER C WITH CEDILLA"),
    ('È', "LATIN CAPITAL LETTER E WITH GRAVE"),
    ('É', "LATIN CAPITAL LETTER E WITH ACUTE"),
    ('Ê', "LATIN CAPITAL LETTER E WITH CIRCUMFLEX"),
    ('Ë', "LATIN CAPITAL LETTER E WITH DIAERESIS"),
    ('Ì', "LATIN CAPITAL LETTER I WITH GRAVE"),
    ('Í', "LATIN CAPITAL LETTER I WITH ACUTE"),
    ('Î', "LATIN CAPITAL LETTER I WITH CIRCUMFLEX"),
    ('Ï', "LATIN CAPITAL LETTER I WITH DIAERESIS"),
    ('Ñ', "LATIN CAPITAL LETTER N WITH TILDE"),
    ('Ò', "LATIN CAPITAL LETTER O WITH GRAVE"),
    ('Ó', "LATIN CAPITAL LETTER O WITH ACUTE"),
    ('Ô', "LATIN CAPITAL LETTER O WITH CIRCUMFLEX"),
    ('Õ', "LATIN CAPITAL LETTER O WITH TILDE"),
    ('Ö', "LATIN CAPITAL LETTER O WITH DIAERESIS"),
    ('Ø', "LATIN CAPITAL LETTER O WITH STROKE"),
    ('Ù', "LATIN CAPITAL LETTER U WITH GRAVE"),
    ('Ú', "LATIN CAPITAL LETTER U WITH ACUTE"),
    ('Û', "LATIN CAPITAL LETTER U WITH CIRCUMFLEX"),
    ('Ü', "LATIN CAPITAL LETTER U WITH DIAERESIS"),
    ('Ý', "LATIN CAPITAL LETTER Y WITH ACUTE"),
    ('ß', "LATIN SMALL LETTER SHARP S"),
    ('à', "LATIN SMALL LETTER A WITH GRAVE"),
    ('á', "LATIN SMALL LETTER A WITH ACUTE"),
    ('â', "LATIN SMALL LETTER A WITH CIRCUMFLEX"),
    ('ã', "LATIN SMALL LETTER A WITH TILDE"),
    ('ä', "LATIN SMALL LETTER A WITH DIAERESIS"),
    ('å', "LATIN SMALL LETTER A WITH RING ABOVE"),
    ('æ', "LATIN SMALL LETTER AE"),
    ('ç', "LATIN SMALL LETTER C WITH CEDILLA"),
    ('è', "LATIN SMALL LETTER E WITH GRAVE"),
    ('é', "LATIN SMALL LETTER E WITH ACUTE"),
    ('ê', "LATIN SMALL LETTER E WITH CIRCUMFLEX"),
    ('ë', "LATIN SMALL LETTER E WITH DIAERESIS"),
    ('ì', "LATIN SMALL LETTER I WITH GRAVE"),
    ('í', "LATIN SMALL LETTER I WITH ACUTE"),
    ('î', "LATIN SMALL LETTER I WITH CIRCUMFLEX"),
    ('ï', "LATIN SMALL LETTER I WITH DIAERESIS"),
    ('ñ', "LATIN SMALL LETTER N WITH TILDE"),
    ('ò', "LATIN SMALL LETTER O WITH GRAVE"),
    ('ó', "LATIN SMALL LETTER O WITH ACUTE"),
    ('ô', "LATIN SMALL LETTER O WITH CIRCUMFLEX"),
    ('õ', "LATIN SMALL LETTER O WITH TILDE"),
    ('ö', "LATIN SMALL LETTER O WITH DIAERESIS"),
    ('ø', "LATIN SMALL LETTER O WITH STROKE"),
    ('ù', "LATIN SMALL LETTER U WITH GRAVE"),
    ('ú', "LATIN SMALL LETTER U WITH ACUTE"),
    ('û', "LATIN SMALL LETTER U WITH CIRCUMFLEX"),
    ('ü', "LATIN SMALL LETTER U WITH DIAERESIS"),
    ('ý', "LATIN SMALL LETTER Y WITH ACUTE"),
    ('ÿ', "LATIN SMALL LETTER Y WITH DIAERESIS"),
    ('Ă', "LATIN CAPITAL LETTER A WITH BREVE"),
    ('ă', "LATIN SMALL LETTER A WITH BREVE"),
    ('Ą', "LATIN CAPITAL LETTER A WITH OGONEK"),
    ('ą', "LATIN SMALL LETTER A WITH OGONEK"),
    ('Ć', "LATIN CAPITAL LETTER C WITH ACUTE"),
    ('ć', "LATIN SMALL LETTER C WITH ACUTE"),
    ('Č', "LATIN CAPITAL LETTER C WITH CARON"),
    ('č', "LATIN SMALL LETTER C WITH CARON"),
    ('Ď', "LATIN CAPITAL LETTER D WITH CARON"),
    ('ď', "LATIN SMALL LETTER D WITH CARON"),
    ('Ę', "LATIN CAPITAL LETTER E WITH OGONEK"),
    ('ę', "LATIN SMALL LETTER E WITH OGONEK"),
    ('Ě', "LATIN CAPITAL LETTER E WITH CARON"),
    ('ě', "LATIN SMALL LETTER E WITH CARON"),
    ('Ğ', "LATIN CAPITAL LETTER G WITH BREVE"),
    ('ğ', "LATIN SMALL LETTER G WITH BREVE"),
    ('İ', "LATIN CAPITAL LETTER I WITH DOT ABOVE"),
    ('ı', "LATIN SMALL LETTER DOTLESS I"),
    ('Ł', "LATIN CAPITAL LETTER L WITH STROKE"),
    ('ł', "LATIN SMALL LETTER L WITH STROKE"),
    ('Ń', "LATIN CAPITAL LETTER N WITH ACUTE"),
    ('ń', "LATIN SMALL LETTER N WITH ACUTE"),
    ('Ň', "LATIN CAPITAL LETTER N WITH CARON"),
    ('ň', "LATIN SMALL LETTER N WITH CARON"),
    ('Ő', "LATIN CAPITAL LETTER O WITH DOUBLE ACUTE"),
    ('ő', "LATIN SMALL LETTER O WITH DOUBLE ACUTE"),
    ('Œ', "LATIN CAPITAL LIGATURE OE"),
    ('œ', "LATIN SMALL LIGATURE OE"),
    ('Ř', "LATIN CAPITAL LETTER R WITH CARON"),
    ('ř', "LATIN SMALL LETTER R WITH CARON"),
    ('Ś', "LATIN CAPITAL LETTER S WITH ACUTE"),
    ('ś', "LATIN SMALL LETTER S WITH ACUTE"),
    ('Ş', "LATIN CAPITAL LETTER S WITH CEDILLA"),
    ('ş', "LATIN SMALL LETTER S WITH CEDILLA"),
    ('Š', "LATIN CAPITAL LETTER S WITH CARON"),
    ('š', "LATIN SMALL LETTER S WITH CARON"),
    ('Ť', "LATIN CAPITAL LETTER T WITH CARON"),
    ('ť', "LATIN SMALL LETTER T WITH CARON"),
    ('Ů', "LATIN CAPITAL LETTER U WITH RING ABOVE"),
    ('ů', "LATIN SMALL LETTER U WITH RING ABOVE"),
    ('Ű', "LATIN CAPITAL LETTER U WITH DOUBLE ACUTE"),
    ('ű', "LATIN SMALL LETTER U WITH DOUBLE ACUTE"),
    ('Ÿ', "LATIN CAPITAL LETTER Y WITH DIAERESIS"),
    ('Ź', "LATIN CAPITAL LETTER Z WITH ACUTE"),
    ('ź', "LATIN SMALL LETTER Z WITH ACUTE"),
    ('Ż', "LATIN CAPITAL LETTER Z WITH DOT ABOVE"),
    ('ż', "LATIN SMALL LETTER Z WITH DOT ABOVE"),
    ('Ž', "LATIN CAPITAL LETTER Z WITH CARON"),
    ('ž', "LATIN SMALL LETTER Z WITH CARON"),
    ('Ș', "LATIN CAPITAL LETTER S WITH COMMA BELOW"),
    ('ș', "LATIN SMALL LETTER S WITH COMMA BELOW"),
    ('Ț', "LATIN CAPITAL LETTER T WITH COMMA BELOW"),
    ('ț', "LATIN SMALL LETTER T WITH COMMA BELOW"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_accents() {
        let langs = vec!["fr".to_string()];
        let accents = accents_for_letter('e', &langs);
        assert_eq!(accents, vec!['é', 'è', 'ê', 'ë']);
    }

    #[test]
    fn uppercase_accents() {
        let langs = vec!["fr".to_string()];
        let accents = accents_for_letter('E', &langs);
        assert_eq!(accents, vec!['É', 'È', 'Ê', 'Ë']);
    }

    #[test]
    fn multi_language_dedup() {
        let langs = vec!["fr".to_string(), "pt".to_string()];
        let accents = accents_for_letter('a', &langs);
        // fr: à, â, æ — pt: á, â, ã, à → merged: à, â, æ, á, ã
        assert_eq!(accents, vec!['à', 'â', 'æ', 'á', 'ã']);
    }

    #[test]
    fn no_accents_for_x() {
        let langs = vec!["fr".to_string()];
        assert!(!has_accents('x', &langs));
    }

    #[test]
    fn unicode_table_sorted() {
        assert!(UNICODE_NAMES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn every_offered_char_has_a_name() {
        let langs: Vec<String> = LANGUAGES.iter().map(|s| s.to_string()).collect();
        for base in 'a'..='z' {
            for letter in [base, base.to_ascii_uppercase()] {
                for ch in accents_for_letter(letter, &langs) {
                    if ch.is_ascii() {
                        continue;
                    }
                    assert!(unicode_name(ch).is_some(), "missing name for {ch:?}");
                }
            }
        }
    }

    #[test]
    fn description_format() {
        assert_eq!(
            unicode_description('é'),
            "U+00E9 LATIN SMALL LETTER E WITH ACUTE"
        );
        assert_eq!(unicode_description('x'), "U+0078");
    }
}
