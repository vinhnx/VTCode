pub(super) fn has_unsafe_sed_options(arguments: &[String]) -> bool {
    let mut expects_expression = false;
    let mut expression_seen = false;

    for word in arguments {
        if expects_expression {
            if sed_script_may_write(word) {
                return true;
            }
            expects_expression = false;
            expression_seen = true;
            continue;
        }

        if word == "-e" || word == "--expression" {
            expects_expression = true;
            continue;
        }
        if let Some(expression) = word.strip_prefix("--expression=") {
            if sed_script_may_write(expression) {
                return true;
            }
            expression_seen = true;
            continue;
        }
        if let Some(expression) = word.strip_prefix("-e")
            && !expression.is_empty()
        {
            if sed_script_may_write(expression) {
                return true;
            }
            expression_seen = true;
            continue;
        }

        if word == "-f"
            || word.starts_with("-f")
            || word.starts_with("--file")
            || word == "-i"
            || word.starts_with("-i")
            || word == "-I"
            || word.starts_with("-I")
            || word.starts_with("--in-place")
        {
            return true;
        }

        if word == "--" {
            expects_expression = true;
            continue;
        }
        if !word.starts_with('-') && !expression_seen {
            if sed_script_may_write(word) {
                return true;
            }
            expression_seen = true;
        }
    }

    // A missing argument makes the command invalid. Treat it as unsafe here
    // so malformed input never receives a read-only classification.
    expects_expression || !expression_seen
}

fn sed_script_may_write(script: &str) -> bool {
    let chars = script.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        while chars
            .get(index)
            .is_some_and(|character| character.is_whitespace() || *character == ';')
        {
            index += 1;
        }

        if let Some(after_address) = consume_sed_address(&chars, index) {
            index = after_address;
            if chars.get(index) == Some(&',') {
                index += 1;
                let Some(after_second_address) = consume_sed_address(&chars, index) else {
                    return true;
                };
                index = after_second_address;
            }
            if chars.get(index) == Some(&'!') {
                index += 1;
            }
            while chars.get(index).is_some_and(|character| character.is_whitespace()) {
                index += 1;
            }
        }

        let Some(&command) = chars.get(index) else {
            break;
        };
        match command {
            'e' | 'r' | 'w' => return true,
            's' => {
                let Some(after_replacement) = consume_sed_substitution(&chars, index) else {
                    return true;
                };
                if sed_substitution_may_write(&chars, after_replacement) {
                    return true;
                }
                index = after_replacement;
                while chars.get(index).is_some_and(|character| !matches!(*character, ';' | '\n')) {
                    index += 1;
                }
            }
            _ => {
                while chars.get(index).is_some_and(|character| !matches!(*character, ';' | '\n')) {
                    index += 1;
                }
            }
        }
    }

    false
}

fn consume_sed_address(chars: &[char], start: usize) -> Option<usize> {
    match chars.get(start)? {
        character if character.is_ascii_digit() => {
            let mut index = start + 1;
            while chars.get(index).is_some_and(|character| character.is_ascii_digit()) {
                index += 1;
            }
            Some(index)
        }
        '$' => Some(start + 1),
        '/' => find_unescaped_sed_delimiter(chars, start + 1, '/').map(|index| index + 1),
        _ => None,
    }
}

fn find_unescaped_sed_delimiter(chars: &[char], start: usize, delimiter: char) -> Option<usize> {
    let mut index = start;
    while index < chars.len() {
        if chars[index] == '\\' {
            index = index.saturating_add(2);
            continue;
        }
        if chars[index] == delimiter {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn consume_sed_substitution(chars: &[char], start: usize) -> Option<usize> {
    let delimiter = *chars.get(start + 1)?;
    if delimiter.is_whitespace() {
        return None;
    }
    let pattern_end = find_unescaped_sed_delimiter(chars, start + 2, delimiter)?;
    let replacement_end = find_unescaped_sed_delimiter(chars, pattern_end + 1, delimiter)?;
    Some(replacement_end + 1)
}

fn sed_substitution_may_write(chars: &[char], start: usize) -> bool {
    let mut index = start;
    while chars.get(index).is_some_and(|character| character.is_whitespace()) {
        index += 1;
    }
    matches!(chars.get(index), Some('e' | 'w'))
        && chars
            .get(index + 1)
            .is_none_or(|character| character.is_whitespace() || *character == ';')
}

/// Return whether an `awk` invocation uses an option or program shape that can
/// write workspace state or execute commands.
///
/// Only data-only options pass: `-v`/`--assign` (variable binding) and
/// `-F`/`--field-separator`, each either attached or consuming the next word.
/// Every other option fails closed — `-i`/`--include` edits in place,
/// `-f`/`--file`/`--source` load program text this scanner cannot see,
/// `-l`/`--load` loads native extensions, `-p`/`--profile`,
/// `-W`/`--dump-variables`, and `--pretty-print` write files, and the
/// remaining option surface is too wide to allow-list safely. `--` ends option
/// parsing. The first bare word is the program and must
/// be write-free per [`awk_program_may_write`]; later words are filenames. A
/// missing program is malformed, so it fails closed like the `sed` guard.
pub(super) fn has_unsafe_awk_options(arguments: &[String]) -> bool {
    let mut program_seen = false;
    let mut options_ended = false;
    let mut index = 0;

    while index < arguments.len() {
        let word = arguments[index].as_str();
        if !options_ended && word == "--" {
            options_ended = true;
            index += 1;
            continue;
        }
        if !options_ended && word.starts_with('-') && word.len() > 1 {
            if word == "-v" || word == "--assign" || word == "-F" || word == "--field-separator" {
                // Value-taking option with a separate argument.
                index += 2;
                continue;
            }
            if word.starts_with("-v")
                || word.starts_with("--assign=")
                || word.starts_with("-F")
                || word.starts_with("--field-separator=")
            {
                // Attached value (`-F:`, `-vlimit=10`).
                index += 1;
                continue;
            }
            return true;
        }
        if !program_seen {
            program_seen = true;
            if awk_program_may_write(word) {
                return true;
            }
        }
        index += 1;
    }

    !program_seen
}

/// Return whether an `awk` program can write files, pipe into commands,
/// execute them, or load external code. Single `>` (output redirection /
/// append `>>`) and bare `|` (command pipe) are writes; `>=` and `||` are
/// comparisons/operators. `system()` runs shell commands; `@` invokes gawk
/// indirect calls (`@func()`) and directives (`@include`, `@load`), which can
/// execute or load arbitrary code — including a `system` name smuggled via `-v`.
///
/// A single `>` is a comparison (not redirection) when it cannot be `print` /
/// `printf` redirection: inside `(...)` / `[...]` parentheses (e.g.
/// `if(p2==0 && i>1)`), or in a statement with no prior `print` / `printf`
/// (e.g. pattern `$3>100`). `print > file`, `print x > file`, and `print >>`
/// stay mutating. `|` stays conservative: only `||` and quoted `"|"` pass.
///
/// Double-quoted string literals are scanned as data: a quoted `"|"` passed to
/// `index()` is a literal, not a command pipe, so the read-only shape
/// `awk '... index(rest,"|") ...' file` is not misclassified. To keep this
/// safe the scanner distinguishes string literals, regex literals (`/.../`),
/// and division using awk's operand-vs-operator rule, so a quote *inside* a
/// regex (`/"/`, `/a"b/`) can never desync string tracking and hide a pipe.
/// `@` and `system()` fail closed everywhere — including inside string and
/// regex literals — and `>`/`|`/`@` inside regex literals stay mutating, so the
/// pinned `/a|b/` and `"a@b"` shapes remain conservative.
fn awk_program_may_write(program: &str) -> bool {
    let chars = program.chars().collect::<Vec<_>>();
    let mut index = 0;
    // Whether the previous significant token can end an operand. This decides
    // whether `/` opens a regex literal or is a division operator.
    let mut prev_operand = false;
    // `print` / `printf` seen in the current statement (reset on `;`/`{`/`}`).
    // A single `>` can only be output redirection inside such a statement;
    // without it (pattern `$3>100`) or inside parens (e.g. `if(i>1)`) it is a
    // numeric comparison. `>>` is always redirection.
    let mut seen_print_in_statement = false;
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    while index < chars.len() {
        let character = chars[index];
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if character == '#' {
            // Comments are not AWK syntax. In particular, a commented `(`
            // must not make a later print redirection look parenthesized.
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if character == '"' {
            index += 1;
            let mut terminated = false;
            while index < chars.len() {
                let inner = chars[index];
                if inner == '\\' {
                    index += 2;
                    continue;
                }
                if inner == '"' {
                    index += 1;
                    terminated = true;
                    break;
                }
                // `@`/`system()` fail closed even inside a literal; `>`/`|`
                // are literal data here and are skipped.
                if inner == '@' {
                    return true;
                }
                if (inner == 's' || inner == 'S') && awk_calls_system(&chars, index) {
                    return true;
                }
                index += 1;
            }
            if !terminated {
                return true;
            }
            prev_operand = true;
            continue;
        }
        if character == '/' && !prev_operand {
            // Regex literal. Quotes are regex content, not string delimiters,
            // so they cannot desync string parsing; `>`/`|`/`@` inside still
            // fail closed, matching the pinned `/a|b/` policy.
            index += 1;
            let mut terminated = false;
            while index < chars.len() {
                let inner = chars[index];
                if inner == '\\' {
                    index += 2;
                    continue;
                }
                if inner == '/' {
                    index += 1;
                    terminated = true;
                    break;
                }
                if inner == '@' {
                    return true;
                }
                if inner == '>' {
                    if chars.get(index + 1) == Some(&'=') {
                        index += 2;
                        continue;
                    }
                    return true;
                }
                if inner == '|' {
                    if chars.get(index + 1) == Some(&'|') {
                        index += 2;
                        continue;
                    }
                    return true;
                }
                index += 1;
            }
            if !terminated {
                return true;
            }
            prev_operand = true;
            continue;
        }
        if character == '@' {
            return true;
        }
        if (character == 's' || character == 'S') && awk_calls_system(&chars, index) {
            return true;
        }
        if character == '>' {
            if chars.get(index + 1) == Some(&'=') {
                index += 2;
                prev_operand = false;
                continue;
            }
            // `>>` is always append-redirection.
            if chars.get(index + 1) == Some(&'>') {
                return true;
            }
            // Single `>` is redirection only inside a `print`/`printf`
            // statement at paren/bracket depth 0. Otherwise it is a numeric
            // comparison (`if(p2==0 && i>1)`, pattern `$3>100`).
            if paren_depth == 0 && bracket_depth == 0 && seen_print_in_statement {
                return true;
            }
            index += 1;
            prev_operand = false;
            continue;
        }
        if character == '|' {
            if chars.get(index + 1) == Some(&'|') {
                index += 2;
                prev_operand = false;
                continue;
            }
            return true;
        }
        if character == '/' {
            // Division: an operand precedes it.
            prev_operand = false;
            index += 1;
            continue;
        }
        if character.is_ascii_alphanumeric() || character == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_ascii_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            if word == "print" || word == "printf" {
                seen_print_in_statement = true;
            }
            prev_operand = !is_awk_keyword(&word);
            continue;
        }
        if character == '(' {
            paren_depth = paren_depth.saturating_add(1);
            prev_operand = false;
            index += 1;
            continue;
        }
        if character == '[' {
            bracket_depth = bracket_depth.saturating_add(1);
            prev_operand = false;
            index += 1;
            continue;
        }
        if character == ')' {
            paren_depth = paren_depth.saturating_sub(1);
            prev_operand = true;
            index += 1;
            continue;
        }
        if character == ']' {
            bracket_depth = bracket_depth.saturating_sub(1);
            prev_operand = true;
            index += 1;
            continue;
        }
        if character == '$' {
            prev_operand = true;
            index += 1;
            continue;
        }
        if character == ';' || character == '{' || character == '}' {
            // New statement: a later `>` needs its own `print`/`printf`.
            seen_print_in_statement = false;
            prev_operand = false;
            index += 1;
            continue;
        }
        prev_operand = false;
        index += 1;
    }
    false
}

/// Awk keywords that do not end an operand, so a following `/` opens a regex
/// literal (`print /re/`) rather than being a division operator. Kept minimal
/// and conservative: an unlisted keyword is treated as an operand, which at
/// worst misreads a regex as division — still fail-closed, because regex
/// contents are then scanned as top-level code and any `>`/`|`/`@` is caught.
fn is_awk_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "else"
            | "while"
            | "for"
            | "do"
            | "break"
            | "continue"
            | "next"
            | "nextfile"
            | "exit"
            | "return"
            | "delete"
            | "in"
            | "getline"
            | "print"
            | "printf"
            | "function"
    )
}

/// Return whether `chars[start..]` invokes awk's `system()` builtin: the
/// identifier `system` (case-insensitive) on an identifier boundary followed
/// by optional whitespace and `(`.
fn awk_calls_system(chars: &[char], start: usize) -> bool {
    const NAME: &[char] = &['s', 'y', 's', 't', 'e', 'm'];
    if start > 0 && is_awk_ident_char(chars[start - 1]) {
        return false;
    }
    let candidate = chars.get(start..start + NAME.len());
    if candidate.is_none_or(|slice| {
        slice
            .iter()
            .zip(NAME.iter())
            .any(|(actual, expected)| !actual.eq_ignore_ascii_case(expected))
    }) {
        return false;
    }
    let mut index = start + NAME.len();
    while chars.get(index).is_some_and(|character| character.is_whitespace()) {
        index += 1;
    }
    chars.get(index) == Some(&'(')
}

fn is_awk_ident_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
