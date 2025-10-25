/** @type {import('changelogithub').ChangelogOptions} */
module.exports = {
  types: {
    feat: { title: 'Features' },
    fix: { title: 'Bug Fixes' },
    perf: { title: 'Performance' },
    refactor: { title: 'Refactors' },
    docs: { title: 'Documentation' },
    test: { title: 'Tests' },
    build: { title: 'Build' },
    ci: { title: 'CI' }
    // chore: { title: 'Chore' } - Excluded to avoid cluttering the changelog
  },
  excludeTypes: ['chore'], // Exclude chore commits from the changelog
  // Exclude version bump and release commits using regex patterns
  exclude: [
    // Matches commits that are purely version bumps
    /chore\(release\):/,
    /bump version/i,
    /update version/i,
    /version bump/i,
    /release v\d+\.\d+\.\d+/i,
    /chore.*version/i,
    /chore.*release/i,
    /build.*version/i,
    /update.*version.*number/i,
    /bump.*version.*to/i
  ],
  capitalize: true,
  group: true,
  emoji: false
}