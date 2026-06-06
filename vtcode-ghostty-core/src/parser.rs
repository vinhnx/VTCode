/// Parser states for the VT state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParserState {
    Ground,
    Escape,
    Csi,
    Osc,
}

/// A parsed CSI sequence.
#[derive(Clone, Debug)]
pub(crate) struct CsiSequence {
    pub(crate) raw: String,
    pub(crate) private: bool,
    pub(crate) params: Vec<Option<usize>>,
}

impl CsiSequence {
    pub(crate) fn parse(raw: &str) -> Self {
        let private = raw.starts_with('?');
        let clean = if private { &raw[1..] } else { raw };

        // Strip leading intermediate characters (>, =, <)
        let clean = clean.trim_start_matches(|c: char| c == '>' || c == '=' || c == '<');

        let params = clean
            .split(';')
            .map(|s| {
                if s.is_empty() {
                    None
                } else {
                    s.parse::<usize>().ok()
                }
            })
            .collect();

        Self {
            raw: raw.to_string(),
            private,
            params,
        }
    }

    /// Get parameter at index, or the default value.
    pub(crate) fn param_or(&self, index: usize, default: usize) -> usize {
        self.params.get(index).and_then(|p| *p).unwrap_or(default)
    }

    /// Convert a 1-based parameter to 0-based (saturating).
    pub(crate) fn one_based_to_zero(&self, index: usize) -> usize {
        self.param_or(index, 1).saturating_sub(1)
    }
}
