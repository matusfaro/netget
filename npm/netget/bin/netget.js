#!/usr/bin/env node
'use strict';

// Launcher for the platform-native netget binary.
//
// Resolution order:
//   1. NETGET_BINARY env override
//   2. The @smotana/netget-<platform> optional dependency
//   3. Cached download in the user cache dir
//   4. Download from GitHub Releases (base overridable via NETGET_DOWNLOAD_BASE)
//
// This script must NEVER write to stdout: in `--mcp` mode stdout carries
// JSON-RPC framing for the MCP client. All diagnostics go to stderr.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const pkg = require('../package.json');
const VERSION = pkg.version;
const DOWNLOAD_BASE =
  process.env.NETGET_DOWNLOAD_BASE ||
  'https://github.com/smotanacom/netget/releases/download';

const PLATFORMS = {
  'darwin-arm64': { triple: 'aarch64-apple-darwin', ext: 'tar.gz' },
  'darwin-x64': { triple: 'x86_64-apple-darwin', ext: 'tar.gz' },
  'linux-x64': { triple: 'x86_64-unknown-linux-gnu', ext: 'tar.gz' },
  'linux-arm64': { triple: 'aarch64-unknown-linux-gnu', ext: 'tar.gz' },
  'linux-x64-musl': { triple: 'x86_64-unknown-linux-musl', ext: 'tar.gz' },
  'win32-x64': { triple: 'x86_64-pc-windows-msvc', ext: 'zip' },
};

function isMusl() {
  // glibcVersionRuntime is absent on musl-based systems (e.g. Alpine).
  try {
    const report = process.report.getReport();
    return !report.header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function platformKey() {
  const { platform, arch } = process;
  if (platform === 'darwin' && arch === 'arm64') return 'darwin-arm64';
  if (platform === 'darwin' && arch === 'x64') return 'darwin-x64';
  if (platform === 'linux' && arch === 'x64')
    return isMusl() ? 'linux-x64-musl' : 'linux-x64';
  if (platform === 'linux' && arch === 'arm64') {
    if (isMusl()) fail(`no prebuilt netget binary for linux-arm64 (musl)`);
    return 'linux-arm64';
  }
  if (platform === 'win32' && arch === 'x64') return 'win32-x64';
  return null;
}

function fail(message) {
  process.stderr.write(`netget: ${message}\n`);
  process.exit(1);
}

function binName() {
  return process.platform === 'win32' ? 'netget.exe' : 'netget';
}

function resolveOptionalDep(key) {
  try {
    return require.resolve(`@smotana/netget-${key}/bin/${binName()}`);
  } catch {
    return null;
  }
}

function cacheDir() {
  if (process.platform === 'win32' && process.env.LOCALAPPDATA) {
    return path.join(process.env.LOCALAPPDATA, 'netget', 'bin', VERSION);
  }
  const base =
    process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
  return path.join(base, 'netget', VERSION);
}

async function download(url, dest) {
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) {
    throw new Error(`download failed: ${res.status} ${res.statusText} (${url})`);
  }
  const buf = Buffer.from(await res.arrayBuffer());
  fs.writeFileSync(dest, buf);
}

async function fetchBinary(key) {
  const { triple, ext } = PLATFORMS[key];
  const dir = cacheDir();
  const cached = path.join(dir, binName());
  if (fs.existsSync(cached)) return cached;

  const archiveName = `netget-${triple}.${ext}`;
  const url = `${DOWNLOAD_BASE}/v${VERSION}/${archiveName}`;
  process.stderr.write(`netget: downloading ${url}\n`);

  fs.mkdirSync(dir, { recursive: true });
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'netget-'));
  try {
    const archive = path.join(tmp, archiveName);
    await download(url, archive);
    // System tar handles .tar.gz everywhere and .zip on Windows (bsdtar).
    const tar = spawnSync('tar', ['-xf', archive, '-C', tmp], {
      stdio: ['ignore', 'ignore', 'inherit'],
    });
    if (tar.status !== 0) throw new Error('failed to extract archive');
    const extracted = path.join(tmp, binName());
    if (!fs.existsSync(extracted)) {
      throw new Error(`archive did not contain ${binName()}`);
    }
    if (process.platform !== 'win32') fs.chmodSync(extracted, 0o755);
    // Atomic within the same filesystem is not guaranteed across tmp -> cache,
    // so copy to a temp name inside the cache dir, then rename.
    const staging = path.join(dir, `.${binName()}.tmp-${process.pid}`);
    fs.copyFileSync(extracted, staging);
    if (process.platform !== 'win32') fs.chmodSync(staging, 0o755);
    fs.renameSync(staging, cached);
    return cached;
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

async function resolveBinary() {
  if (process.env.NETGET_BINARY) return process.env.NETGET_BINARY;

  const key = platformKey();
  if (!key) {
    fail(
      `unsupported platform ${process.platform}-${process.arch}; ` +
        `build from source: https://github.com/smotanacom/netget`
    );
  }
  const fromDep = resolveOptionalDep(key);
  if (fromDep) return fromDep;

  try {
    return await fetchBinary(key);
  } catch (err) {
    fail(
      `could not locate the @smotana/netget-${key} package or download the ` +
        `binary (${err.message}). Try reinstalling without --ignore-scripts/` +
        `--omit=optional, or set NETGET_BINARY to a netget binary.`
    );
  }
}

async function main() {
  const bin = await resolveBinary();
  const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' });

  // Forward termination signals; let the child drive shutdown so MCP servers
  // exit cleanly when the client (e.g. Claude Code) stops them.
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    process.on(sig, () => {
      if (!child.killed) child.kill(sig);
    });
  }

  child.on('error', (err) => fail(`failed to run ${bin}: ${err.message}`));
  child.on('exit', (code, signal) => {
    if (signal) {
      // Re-raise so our own exit status reflects the child's signal death.
      process.removeAllListeners(signal);
      process.kill(process.pid, signal);
    } else {
      process.exit(code == null ? 1 : code);
    }
  });
}

main().catch((err) => fail(err.message));
