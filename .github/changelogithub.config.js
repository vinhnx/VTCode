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
    ci: { title: 'CI' },
    chore: { title: 'Chore' }
  },
  capitalize: true,
  group: true,
  emoji: false
}