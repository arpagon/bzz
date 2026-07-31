pub fn text(input: &str) -> String {
    input
        .chars()
        .map(|character| if safe(character) { character } else { '�' })
        .collect()
}

fn safe(character: char) -> bool {
    match character {
        '\n' | '\t' => true,
        '\u{0000}'..='\u{001f}'
        | '\u{007f}'..='\u{009f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}' => false,
        _ => true,
    }
}

pub fn single_line(input: &str) -> String {
    text(input).replace(['\n', '\t'], " ")
}
