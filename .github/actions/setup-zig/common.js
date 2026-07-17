const os = require('os');
const fs = require('fs');
const path = require('path');
const core = require('@actions/core');
const github = require('@actions/github');

const VERSIONS_JSON = 'https://ziglang.org/download/index.json';
const MACH_VERSIONS_JSON = 'https://pkg.machengine.org/zig/index.json';
const CACHE_PREFIX = "setup-zig-global-cache-";

// The following regexes pull specific values out of ZON.
// This is bad and should be replaced with an actual parser -- see #50.

// Mach uses `mach_zig_version` in `build.zig.zon` to signify Mach nominated versions.
// See: https://github.com/marler8997/anyzig?tab=readme-ov-file#mach-versions-and-download-mirror
const MACH_ZIG_VERSION_REGEX = /\.\s*mach_zig_version\s*=\s*"(.*?)"/;
const MINIMUM_ZIG_VERSION_REGEX = /\.\s*minimum_zig_version\s*=\s*"(.*?)"/;

let _cached_version = null;
async function getVersion() {
  if (_cached_version != null) {
    return _cached_version;
  }

  let raw = core.getInput('version');
  if (raw === '') {
    try {
      const zon = await fs.promises.readFile('build.zig.zon', 'utf8');

      // Look for `mach_zig_version` first
      let match = MACH_ZIG_VERSION_REGEX.exec(zon);
      if (match !== null) {
        _cached_version = await getMachVersion(match[1]);
        return _cached_version;
      }

      // Else, look for `minimum_zig_version`
      match = MINIMUM_ZIG_VERSION_REGEX.exec(zon);
      if (match !== null) {
        _cached_version = match[1];
        return _cached_version;
      }

      core.info('Failed to find `mach_zig_version` or `minimum_zig_version` in build.zig.zon (using latest)');
    } catch (e) {
      core.info(`Failed to read build.zig.zon (using latest): ${e}`);
    }

    raw = 'latest';
  }

  if (raw === 'master') {
    _cached_version = await getMasterVersion();
  } else if (raw === 'latest') {
    _cached_version = await getLatestVersion();
  } else if (raw.includes("mach")) {
    _cached_version = await getMachVersion(raw);
  } else {
    _cached_version = raw;
  }

  return _cached_version;
}

async function getMachVersion(raw) {
  const resp = await fetch(MACH_VERSIONS_JSON);
  const versions = await resp.json();
  if (!(raw in versions)) {
    throw new Error(`Mach nominated version '${raw}' not found`);
  }
  return versions[raw].version;
}
async function getMasterVersion() {
  const resp = await fetch(VERSIONS_JSON);
  const versions = await resp.json();
  return versions['master'].version;
}
async function getLatestVersion() {
  const resp = await fetch(VERSIONS_JSON);
  const versions = await resp.json();
  let latest = null;
  let latest_major;
  let latest_minor;
  let latest_patch;
  for (const version in versions) {
    if (version === 'master') continue;
    const [major_str, minor_str, patch_str] = version.split('.')
    const major = Number(major_str);
    const minor = Number(minor_str);
    const patch = Number(patch_str);
    if (latest === null) {
      latest = version;
      latest_major = major;
      latest_minor = minor;
      latest_patch = patch;
      continue;
    }
    if (major > latest_major ||
        (major == latest_major && minor > latest_minor) ||
        (major == latest_major && minor == latest_minor && patch > latest_patch))
    {
      latest = version;
      latest_major = major;
      latest_minor = minor;
      latest_patch = patch;
    }
  }
  return latest;
}

async function getTarballName() {
  const version = await getVersion();

  let arch = {
    arm:      'arm',
    arm64:    'aarch64',
    loong64:  'loongarch64',
    mips:     'mips',
    mipsel:   'mipsel',
    mips64:   'mips64',
    mips64el: 'mips64el',
    ppc64:    'powerpc64',
    riscv64:  'riscv64',
    s390x:    's390x',
    ia32:     'x86',
    x64:      'x86_64',
  }[os.arch()];

  // For some incomprehensible reason, Node.js's brain-damaged build system explicitly throws away
  // the knowledge that it is building for ppc64le, so os.arch() will identify it as ppc64 even on
  // little endian.
  if (arch === 'powerpc64' && os.endianness() === 'LE') {
    arch = 'powerpc64le';
  }

  // Before 0.15.1, Zig used 'armv7a' as the arch name for ARM binaries.
  if (arch === 'arm' && versionLessThan(version, "0.15.1")) {
    arch = 'armv7a';
  }

  const platform = {
    android: 'android',
    freebsd: 'freebsd',
    sunos:   'illumos',
    linux:   'linux',
    darwin:  'macos',
    netbsd:  'netbsd',
    openbsd: 'openbsd',
    win32:   'windows',
  }[os.platform()];

  // Before 0.14.1, Zig tarballs were named like 'zig-linux-x86_64-0.14.0', with the arch
  // and OS fields reversed from the order we use today.
  if (versionLessThan(version, "0.15.0-dev.631+9a3540d61") && versionLessThan(version, "0.14.1")) {
    return `zig-${platform}-${arch}-${version}`;
  }

  return `zig-${arch}-${platform}-${version}`;
}

// Returns `true` if `cur_ver` represents a version less then (i.e. older than) `min_ver`.
// Otherwise, returns `false`.
// If `cur_ver` or `min_ver` is malformed, returns `false`.
function versionLessThan(cur_ver, min_ver) {
  const cur = parseVersion(cur_ver);
  const min = parseVersion(min_ver);
  if (cur === null || min === null) return false;
  // Treating 0.1.2 as 0.1.2-dev+INF makes the comparisons easy!
  const cur_dev = cur.dev === null ? Infinity : cur.dev;
  const min_dev = min.dev === null ? Infinity : min.dev;

  if (cur.major != min.major) return cur.major < min.major;
  if (cur.minor != min.minor) return cur.minor < min.minor;
  if (cur.patch != min.patch) return cur.patch < min.patch;
  return cur_dev < min_dev;
}

// Returns object with keys 'major', 'minor', 'patch', and 'dev'.
// 'dev' is `null` if `str` was not a dev version.
// On failure, returns `null`.
function parseVersion(str) {
  const match = /^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)(?:-dev\.(?<dev>\d+)\+[0-9a-f]*)?$/.exec(str);
  if (match === null) return null;
  return {
    major: parseInt(match.groups['major']),
    minor: parseInt(match.groups['minor']),
    patch: parseInt(match.groups['patch']),
    dev: match.groups['dev'] === undefined ? null : parseInt(match.groups['dev']),
  };
}

async function getTarballExt() {
  if (os.platform() == 'win32') return '.zip';
  return '.tar.xz';
}

async function getCachePrefix() {
  const tarball_name = await getTarballName();
  const job_name = github.context.job.replaceAll(/[^\w]/g, "_");
  const user_key = core.getInput('cache-key');

  return `setup-zig-cache-v2-${job_name}-${tarball_name}-${user_key}-`;
}

function getZigCachePath() {
  return path.join(process.env['GITHUB_WORKSPACE'] ?? process.cwd(), '.zig-cache');
}

module.exports = {
  getVersion,
  getTarballName,
  getTarballExt,
  getCachePrefix,
  getZigCachePath,
};
