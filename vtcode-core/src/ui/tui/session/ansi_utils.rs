/// Strips ANSI escape codes from text to ensure plain text output
#[allow(dead_code)]
pub(super) fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    let mut param_length = 0;
                    while let Some(&next_ch) = chars.peek() {
                        chars.next();
                        param_length += 1;
                        if next_ch.is_ascii_digit() || next_ch == ';' || next_ch == ':' {
                            continue;
                        } else if ('@'..='~').contains(&next_ch) {
                            break;
                        } else if param_length > 20 {
                            break;
                        } else {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(&next_ch) = chars.peek() {
                        chars.next();
                        if next_ch == '\x07' || next_ch == '\x1b' {
                            if next_ch == '\x1b' {
                                if let Some(&'\\') = chars.peek() {
                                    chars.next();
                                }
                            }
                            break;
                        }
                    }
                }
                Some(_) => {
                    let next_ch = chars.peek().unwrap();
                    match next_ch {
                        '7' | '8' | '=' | '>' | 'D' | 'E' | 'H' | 'M' | 'O' | 'P' | 'V' | 'W'
                        | 'X' | 'Z' | '[' | '\\' | ']' | '^' | '_' => {
                            chars.next();
                        }
                        _ => {
                            result.push(ch);
                        }
                    }
                }
                None => {
                    result.push(ch);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}
