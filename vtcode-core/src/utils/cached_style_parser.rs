use anstyle::Style as AnsiStyle;
use anyhow::{Context, Result};
use vtcode_commons::lr_map::LrMap;

/// Thread-safe cached parser for Git and LS_COLORS style strings.
pub struct CachedStyleParser {
    git_cache: LrMap<String, AnsiStyle>,
    ls_colors_cache: LrMap<String, AnsiStyle>,
}

impl CachedStyleParser {
    pub fn new() -> Self {
        Self {
            git_cache: LrMap::new(),
            ls_colors_cache: LrMap::new(),
        }
    }

    pub fn parse_git_style(&self, input: &str) -> Result<AnsiStyle> {
        if let Some(cached) = self.git_cache.get(input) {
            return Ok(cached);
        }

        let result = anstyle_git::parse(input)
            .map_err(|e| anyhow::anyhow!("Failed to parse Git style '{}': {:?}", input, e))?;

        self.git_cache.insert(input.to_string(), result);
        Ok(result)
    }

    pub fn parse_ls_colors(&self, input: &str) -> Result<AnsiStyle> {
        if let Some(cached) = self.ls_colors_cache.get(input) {
            return Ok(cached);
        }

        let result = anstyle_ls::parse(input)
            .ok_or_else(|| anyhow::anyhow!("Failed to parse LS_COLORS '{}'", input))?;

        self.ls_colors_cache.insert(input.to_string(), result);
        Ok(result)
    }

    pub fn parse_flexible(&self, input: &str) -> Result<AnsiStyle> {
        match self.parse_git_style(input) {
            Ok(style) => Ok(style),
            Err(_) => self
                .parse_ls_colors(input)
                .with_context(|| format!("Could not parse style string: '{}'", input)),
        }
    }
}

impl Default for CachedStyleParser {
    fn default() -> Self {
        Self::new()
    }
}
