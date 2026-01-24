//! Human-readable slug generator for plan file names
//!
//! Generates memorable identifiers by combining random adjectives and nouns,
//! producing slugs like "gentle-harbor" or "cosmic-wizard".
//!
//! Based on OpenCode's slug utility pattern for plan mode file naming.

use rand::Rng;

/// Adjectives for slug generation (30 options)
const ADJECTIVES: &[&str] = &[
    "brave", "calm", "clever", "cosmic", "crisp", "curious", "eager", "gentle", "glowing", "happy",
    "hidden", "jolly", "kind", "lucky", "mighty", "misty", "neon", "nimble", "playful", "proud",
    "quick", "quiet", "shiny", "silent", "stellar", "sunny", "swift", "tidy", "witty", "bright",
];

/// Nouns for slug generation (32 options)
const NOUNS: &[&str] = &[
    "cabin", "cactus", "canyon", "circuit", "comet", "eagle", "engine", "falcon", "forest",
    "garden", "harbor", "island", "knight", "lagoon", "meadow", "moon", "mountain", "nebula",
    "orchid", "otter", "panda", "pixel", "planet", "river", "rocket", "sailor", "squid", "star",
    "tiger", "wizard", "wolf", "stream",
];

/// Create a human-readable slug by combining a random adjective with a random noun.
///
/// # Examples
///
/// ```
/// use vtcode_commons::slug;
///
/// let slug = slug::create();
/// // Returns something like "gentle-harbor", "cosmic-wizard", etc.
/// assert!(slug.contains('-'));
/// ```
pub fn create() -> String {
    let mut rng = rand::rng();
    let adj_idx = rng.random_range(0..ADJECTIVES.len());
    let noun_idx = rng.random_range(0..NOUNS.len());

    format!("{}-{}", ADJECTIVES[adj_idx], NOUNS[noun_idx])
}

/// Create a timestamped slug with a human-readable suffix.
///
/// Format: `{timestamp_millis}-{adjective}-{noun}`
///
/// # Examples
///
/// ```
/// use vtcode_commons::slug;
///
/// let slug = slug::create_timestamped();
/// // Returns something like "1768330644696-gentle-harbor"
/// ```
pub fn create_timestamped() -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    format!("{}-{}", timestamp, create())
}

/// Create a slug with a custom prefix.
///
/// # Examples
///
/// ```
/// use vtcode_commons::slug;
///
/// let slug = slug::create_with_prefix("plan");
/// // Returns something like "plan-gentle-harbor"
/// ```
pub fn create_with_prefix(prefix: &str) -> String {
    format!("{}-{}", prefix, create())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_slug() {
        let slug = create();
        assert!(slug.contains('-'));
        let parts: Vec<&str> = slug.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_create_timestamped() {
        let slug = create_timestamped();
        let parts: Vec<&str> = slug.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].parse::<u128>().is_ok());
    }

    #[test]
    fn test_create_with_prefix() {
        let slug = create_with_prefix("plan");
        assert!(slug.starts_with("plan-"));
        let parts: Vec<&str> = slug.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "plan");
    }

    #[test]
    fn test_uniqueness() {
        let slugs: Vec<String> = (0..100).map(|_| create()).collect();
        let unique_count = slugs.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(unique_count > 50, "Expected mostly unique slugs");
    }
}
