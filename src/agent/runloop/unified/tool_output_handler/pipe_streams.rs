//! Pipe-stream alias detection and canonical stream ordering.

pub(super) fn contains_line_block(container: &str, candidate: &str) -> bool {
    !line_block_ranges(container, candidate).is_empty()
}

pub(super) fn line_block_ranges(container: &str, candidate: &str) -> Vec<(usize, usize)> {
    let container_lines = container.lines().collect::<Vec<_>>();
    let candidate_lines = candidate.lines().collect::<Vec<_>>();
    if candidate_lines.is_empty() || candidate_lines.len() > container_lines.len() {
        return Vec::new();
    }

    container_lines
        .windows(candidate_lines.len())
        .enumerate()
        .filter_map(|(start, window)| {
            (window == candidate_lines.as_slice()).then_some((start, start + candidate_lines.len()))
        })
        .collect()
}

pub(super) fn contains_distinct_line_blocks(container: &str, first: &str, second: &str) -> bool {
    let first_ranges = line_block_ranges(container, first);
    let second_ranges = line_block_ranges(container, second);
    first_ranges.iter().any(|&(first_start, first_end)| {
        second_ranges
            .iter()
            .any(|&(second_start, second_end)| first_end <= second_start || second_end <= first_start)
    })
}

pub(super) fn streams_are_aliases(left: &str, right: &str) -> bool {
    contains_line_block(left, right) || contains_line_block(right, left)
}

pub(super) fn output_text<'a>(output: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    output
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim_end)
        .filter(|text| !text.trim().is_empty())
}

pub(super) fn stderr_for_inline_display(output: &serde_json::Value) -> Option<&str> {
    let stderr = output_text(output, "stderr")?;
    // Named streams are distinct unless the authoritative merged `output`
    // field proves that stderr is already present in the terminal capture.
    // stdout and stderr can legitimately contain identical text.
    let already_visible = output_text(output, "output").is_some_and(|merged| {
        output_text(output, "stdout").map_or_else(
            || contains_line_block(merged, stderr),
            |stdout| contains_distinct_line_blocks(merged, stdout, stderr),
        )
    });
    if already_visible { None } else { Some(stderr) }
}

pub(super) fn ordered_stream_texts(output: &serde_json::Value) -> Vec<&str> {
    canonical_pipe_streams(output).into_iter().map(|stream| stream.text).collect()
}

#[derive(Clone, Copy)]
pub(super) struct CanonicalOutputStream<'a> {
    pub(super) label: Option<&'static str>,
    pub(super) text: &'a str,
}

pub(super) fn append_named_streams<'a>(
    streams: &mut Vec<CanonicalOutputStream<'a>>,
    stdout: Option<&'a str>,
    stderr: Option<&'a str>,
) {
    if let Some(stdout) = stdout {
        streams.push(CanonicalOutputStream { label: Some("stdout"), text: stdout });
    }
    if let Some(stderr) = stderr {
        streams.push(CanonicalOutputStream { label: Some("stderr"), text: stderr });
    }
}

pub(super) fn append_content_stream<'a>(streams: &mut Vec<CanonicalOutputStream<'a>>, content: Option<&'a str>) {
    let Some(content) = content else {
        return;
    };
    if !streams.iter().any(|stream| contains_line_block(stream.text, content)) {
        streams.push(CanonicalOutputStream { label: None, text: content });
    }
}

pub(super) fn canonical_pipe_streams(output: &serde_json::Value) -> Vec<CanonicalOutputStream<'_>> {
    let merged = output_text(output, "output");
    let stdout = output_text(output, "stdout");
    let stderr = output_text(output, "stderr");
    let content = output_text(output, "content");
    let mut streams = Vec::new();

    if let Some(merged) = merged {
        let stdout_is_in_merged = stdout.is_some_and(|text| contains_line_block(merged, text));
        let stderr_is_in_merged = stderr.is_some_and(|text| contains_line_block(merged, text));
        let stdout_contains_merged = stdout.is_some_and(|text| contains_line_block(text, merged));
        let stderr_contains_merged = stderr.is_some_and(|text| contains_line_block(text, merged));

        // A combined `output` field is authoritative when it contains both
        // named streams as distinct blocks. Requiring non-overlapping blocks
        // matters when stdout and stderr happen to be identical or one is a
        // prefix of the other: one occurrence cannot prove both are copies.
        if let (Some(stdout), Some(stderr)) = (stdout, stderr)
            && contains_distinct_line_blocks(merged, stdout, stderr)
        {
            streams.push(CanonicalOutputStream { label: None, text: merged });
            append_content_stream(&mut streams, content);
            return streams;
        }

        // When both named streams contain the merged value, the merged field
        // is a bounded preview. Keep each complete, labeled stream instead of
        // guessing that the single preview occurrence represents both pipes.
        if stdout_contains_merged && stderr_contains_merged {
            append_named_streams(&mut streams, stdout, stderr);
            append_content_stream(&mut streams, content);
            return streams;
        }

        // A preview nested in either one named stream is best represented by
        // the complete named values. The other stream remains labeled even
        // when its content is not present in the preview.
        if stdout_contains_merged || stderr_contains_merged {
            append_named_streams(&mut streams, stdout, stderr);
            append_content_stream(&mut streams, content);
            return streams;
        }

        // If both named streams are present in the merged field but overlap,
        // preserve their labels and retain merged-only lines when neither
        // named value covers the whole merged field.
        if stdout_is_in_merged && stderr_is_in_merged {
            append_named_streams(&mut streams, stdout, stderr);
            if !stdout_contains_merged && !stderr_contains_merged {
                streams.push(CanonicalOutputStream { label: None, text: merged });
            }
            append_content_stream(&mut streams, content);
            return streams;
        }

        // A merged value containing only one named stream still carries
        // unlabelled content. Keep that merged value and append the other
        // named stream rather than dropping it as an apparent alias.
        streams.push(CanonicalOutputStream { label: None, text: merged });
        if let Some(stdout) = stdout
            && !stdout_is_in_merged
        {
            streams.push(CanonicalOutputStream { label: Some("stdout"), text: stdout });
        }
        if let Some(stderr) = stderr
            && !stderr_is_in_merged
        {
            streams.push(CanonicalOutputStream { label: Some("stderr"), text: stderr });
        }
        append_content_stream(&mut streams, content);
        return streams;
    }

    if let Some(stdout) = stdout {
        streams.push(CanonicalOutputStream { label: Some("stdout"), text: stdout });
    }
    if let Some(stderr) = stderr {
        // Without a merged authoritative field, stdout and stderr are
        // separate pipes even when their contents happen to match.
        streams.push(CanonicalOutputStream { label: Some("stderr"), text: stderr });
    }
    append_content_stream(&mut streams, content);
    streams
}
