const os = require('os');
const path = require('path');
const fs = require('fs').promises;
const core = require('@actions/core');
const tc = require('@actions/tool-cache');
const cache = require('@actions/cache');
const exec = require('@actions/exec');
const common = require('./common');
const minisign = require('./minisign');

// Upstream's minisign key, from https://ziglang.org/download
const MINISIGN_KEY = 'RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U';

// The base URL of the official builds of Zig. This is only used as a fallback, if all mirrors fail.
const CANONICAL_DEV = 'https://ziglang.org/builds';
const CANONICAL_RELEASE = 'https://ziglang.org/download';

// The URL of the mirror list. This should be an ASCII-encoded text file, with one mirror per LF-separated line.
const MIRRORS_URL = 'https://ziglang.org/download/community-mirrors.txt';

async function downloadFromMirror(mirror, tarball_filename) {
  const tarball_path = await tc.downloadTool(`${mirror}/${tarball_filename}?source=github-mlugg-setup-zig`);

  const signature_response = await fetch(`${mirror}/${tarball_filename}.minisig?source=github-mlugg-setup-zig`);
  const signature_data = Buffer.from(await signature_response.arrayBuffer());

  const tarball_data = await fs.readFile(tarball_path);

  const key = await minisign.parseKey(MINISIGN_KEY);
  const signature = minisign.parseSignature(signature_data);
  if (!await minisign.verifySignature(key, signature, tarball_data)) {
    throw new Error(`signature verification failed for '${mirror}/${tarball_filename}'`);
  }

  // Parse the trusted comment to validate the tarball name.
  // This prevents a malicious actor from trying to pass off one signed tarball as another.
  const match = /^timestamp:\d+\s+file:([^\s]+)\s+hashed$/.exec(signature.trusted_comment.toString());
  if (match === null || match[1] !== tarball_filename) {
    throw new Error(`filename verification failed for '${mirror}/${tarball_filename}'`);
  }

  return tarball_path;
}

async function downloadTarball(tarball_filename) {
  const preferred_mirror = core.getInput('mirror');
  if (preferred_mirror.includes("://ziglang.org/") || preferred_mirror.startsWith("ziglang.org/")) {
    throw new Error("'https://ziglang.org' cannot be used as mirror override; for more information see README.md");
  }
  if (preferred_mirror) {
    core.info(`Using mirror: ${preferred_mirror}`);
    return await downloadFromMirror(preferred_mirror, tarball_filename);
  }

  let mirrors;
  try {
    // Fetch an up-to-date list of available mirrors from ziglang.org.
    const mirrors_response = await fetch(MIRRORS_URL);
    mirrors = (await mirrors_response.text()).split('\n').filter((url) => url.length != 0);
  } catch {
    // ziglang.org itself is inaccessible. As a fallback (so that ziglang.org outages don't break
    // this Action), use a hardcoded mirror list. This was manually fetched from ziglang.org, and I
    // expect that at least some of these mirrors will continue working. It only needs to be updated
    // if the upstream list changes *significantly*.
    mirrors = [
      "https://pkg.machengine.org/zig",
      "https://zigmirror.hryx.net/zig",
      "https://zig.linus.dev/zig",
      "https://zig.squirl.dev",
      "https://zig.florent.dev",
      "https://zig.mirror.mschae23.de/zig",
      "https://zigmirror.meox.dev",
      "https://ziglang.freetls.fastly.net",
      "https://zig.tilok.dev",
      "https://zig-mirror.tsimnet.eu/zig",
      "https://zig.karearl.com/zig",
      "https://pkg.earth/zig",
      "https://fs.liujiacai.net/zigbuilds",
    ];
  }

  core.info(`Available mirrors: ${mirrors.join(", ")}`);

  // We will attempt all mirrors before making a last-ditch attempt to the official download.
  // To avoid hammering a single mirror, we first randomize the array.
  const shuffled_mirrors = mirrors.map((m) => [m, Math.random()]).sort((a, b) => a[1] - b[1]).map((a) => a[0]);
  for (const mirror of shuffled_mirrors) {
    core.info(`Attempting mirror: ${mirror}`);
    try {
      return await downloadFromMirror(mirror, tarball_filename);
    } catch (e) {
      core.info(`Mirror failed with error: ${e}`);
      // continue loop to next mirror
    }
  }

  // As a fallback, attempt ziglang.org.
  const zig_version = tarball_filename.match(/\d+\.\d+\.\d+(-dev\.\d+\+[0-9a-f]+)?/)[0];
  const canonical = zig_version.includes("-dev") ? CANONICAL_DEV : `${CANONICAL_RELEASE}/${zig_version}`;
  core.info(`Attempting official: ${canonical}`);
  return await downloadFromMirror(canonical, tarball_filename);
}

async function retrieveTarball(tarball_name, tarball_ext) {
  const cache_key = `setup-zig-tarball-${tarball_name}`;
  const tarball_basename = `${tarball_name}${tarball_ext}`;
  const tarball_cache_path = path.join(process.env['RUNNER_TEMP'], tarball_basename);

  if (await cache.restoreCache([tarball_cache_path], cache_key)) {
    return tarball_cache_path;
  }

  core.info(`Cache miss. Fetching Zig ${await common.getVersion()}`);
  const downloaded_path = await downloadTarball(tarball_basename);
  await fs.copyFile(downloaded_path, tarball_cache_path)
  await cache.saveCache([tarball_cache_path], cache_key);
  return tarball_cache_path;
}

async function main() {
  try {
    // We will check whether Zig is stored in the cache. We use two separate caches.
    // * 'tool-cache' caches the final extracted directory if the same Zig build is used multiple
    //   times by one job. We have this dependency anyway for archive extraction.
    // * 'cache' only caches the unextracted archive, but it does so across runs. It's a little
    //   less efficient, but still much preferable to fetching Zig from a mirror. We have this
    //   dependency anyway for caching the global Zig cache.
    //
    // Unfortunately, tool-cache can lead to serious performance problems on GitHub-hosted Actions
    // runners -- or their Windows ones at least, because the tool cache is stored on a slow drive.
    // There are even hacky workarounds for this in official Actions:
    //
    // https://github.com/actions/setup-go/blob/d35c59abb061a4a6fb18e82ac0862c26744d6ab5/src/installer.ts#L174
    //
    // Since tool-cache is only really useful on self-hosted runners, let's just disable it by
    // default on GitHub-hosted runners, and hence execute Zig straight out of its extracted dir.
    let use_tool_cache = core.getInput('use-tool-cache');
    if (use_tool_cache === 'true') {
      use_tool_cache = true;
    } else if (use_tool_cache === 'false') {
      use_tool_cache = false;
    } else if (use_tool_cache === '') {
      use_tool_cache = process.env['RUNNER_ENVIRONMENT'] !== 'github-hosted';
    } else {
      throw new Error("Invalid 'use-tool-cache' value. Valid values: 'true', 'false'");
    }
    core.info(`Using tool-cache: ${use_tool_cache}`);

    let zig_dir;
    if (use_tool_cache) {
      zig_dir = tc.find('zig', await common.getVersion());
    }
    if (zig_dir) {
      core.info('Using cached Zig installation from tool-cache');
    } else {
      const tarball_name = await common.getTarballName();
      const tarball_ext = await common.getTarballExt();

      core.info(`Fetching ${tarball_name}${tarball_ext}`);
      const fetch_start = Date.now();
      const tarball_path = await retrieveTarball(tarball_name, tarball_ext);
      core.info(`Fetch took ${Date.now() - fetch_start} ms`);

      core.info(`Extracting tarball ${tarball_name}${tarball_ext}`);

      const extract_start = Date.now();
      const zig_parent_dir = tarball_ext === '.zip' ?
        await tc.extractZip(tarball_path) :
        await tc.extractTar(tarball_path, null, 'xJ'); // J for xz
      core.info(`Extract took ${Date.now() - extract_start} ms`);

      const zig_inner_dir = path.join(zig_parent_dir, tarball_name);
      if (use_tool_cache) {
        core.info('Copying Zig installation to tool-cache');
        zig_dir = await tc.cacheDir(zig_inner_dir, 'zig', await common.getVersion());
      } else {
        zig_dir = zig_inner_dir;
      }
    }

    core.addPath(zig_dir);
    const zig_version = (await exec.getExecOutput('zig', ['version'])).stdout.trim();
    core.info(`Resolved Zig version ${zig_version}`);

    const cache_path = common.getZigCachePath();
    core.exportVariable('ZIG_GLOBAL_CACHE_DIR', cache_path);
    core.exportVariable('ZIG_LOCAL_CACHE_DIR', cache_path);

    if (core.getBooleanInput('use-cache')) {
      const cache_prefix = await common.getCachePrefix();
      core.info(`Attempting restore of Zig cache with prefix '${cache_prefix}'`);
      const hit = await cache.restoreCache([cache_path], cache_prefix);
      if (hit === undefined) {
        core.info(`Cache miss: leaving Zig cache directory at ${cache_path} unpopulated`);
      } else {
        core.info(`Cache hit (key '${hit}'): populating Zig cache directory at ${cache_path}`);
      }
    }
  } catch (err) {
    core.setFailed(err.message);
  }
}

main();
