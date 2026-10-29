pub(super) fn image_attachment_placeholder(number: usize) -> String {
    format!("[Image #{number}]")
}

pub(super) fn image_attachment_placeholders(content: &str) -> Vec<(usize, String)> {
    let marker = "[Image #";
    let mut placeholders = Vec::new();
    let mut search_start = 0;

    while let Some(offset) = content[search_start..].find(marker) {
        let start = search_start + offset;
        let digits_start = start + marker.len();
        let rest = &content[digits_start..];
        let digits_len = rest.bytes().take_while(|byte| byte.is_ascii_digit()).count();
        let placeholder_end = digits_start + digits_len + 1;

        if digits_len > 0 && content.as_bytes().get(placeholder_end - 1) == Some(&b']') {
            if let Ok(number) = content[digits_start..digits_start + digits_len].parse::<usize>()
                && number > 0
            {
                placeholders.push((number, content[start..placeholder_end].to_owned()));
            }
            search_start = placeholder_end;
        } else {
            search_start = digits_start;
        }
    }

    placeholders
}

pub(super) fn attachment_placeholders_for_content(content: &str, attachment_count: usize) -> Vec<Option<String>> {
    let placeholders = image_attachment_placeholders(content);
    let mut sorted_visible_placeholders = placeholders.clone();
    sorted_visible_placeholders.sort_by_key(|(number, _)| *number);
    sorted_visible_placeholders.dedup_by_key(|(number, _)| *number);

    if sorted_visible_placeholders.len() >= attachment_count {
        return sorted_visible_placeholders
            .into_iter()
            .take(attachment_count)
            .map(|(_, placeholder)| Some(placeholder))
            .collect();
    }

    (0..attachment_count)
        .map(|index| {
            let expected_number = index + 1;
            placeholders
                .iter()
                .find_map(|(number, placeholder)| (*number == expected_number).then(|| placeholder.clone()))
                .or_else(|| {
                    if attachment_count == 1 && placeholders.len() == 1 {
                        Some(placeholders[0].1.clone())
                    } else {
                        None
                    }
                })
        })
        .collect()
}
