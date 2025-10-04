/** @type {import('changelogithub').Config} */
module.exports = {
  // Repository information
  repo: 'vtcode',  // Updated to match the actual repository name
  owner: 'vinhnx',

  // Changelog configuration
  types: {
    feat: { title: 'Features', semver: 'minor' },
    fix: { title: 'Bug Fixes', semver: 'patch' },
    perf: { title: 'Performance Improvements', semver: 'patch' },
    refactor: { title: 'Code Refactoring', semver: 'patch' },
    docs: { title: 'Documentation', semver: 'patch' },
    test: { title: 'Tests', semver: 'patch' },
    build: { title: 'Build System', semver: 'patch' },
    ci: { title: 'CI/CD', semver: 'patch' },
    chore: { title: 'Chores', semver: 'patch' },
    style: { title: 'Styles', semver: 'patch' },
    revert: { title: 'Reverts', semver: 'patch' }
  },

  // Output configuration
  output: {
    // Use the existing CHANGELOG.md file
    changelogFilename: 'CHANGELOG.md'
  },

  // Git configuration
  git: {
    // Use conventional commits
    conventional: true,
    // Filter out merge commits
    filter: (commit) => !commit.subject.startsWith('Merge')
  },

  // Release configuration
  release: {
    // Create GitHub releases
    create: true,
    // Draft releases for manual review
    draft: false,
    // Pre-release for beta/rc versions
    prerelease: false
  },

  // Additional configuration
  config: {
    // Include all commits in changelog
    includeAllCommits: true,
    // Group commits by type
    groupByType: true,
    // Sort commits by date
    sortBy: 'date'
  }
}