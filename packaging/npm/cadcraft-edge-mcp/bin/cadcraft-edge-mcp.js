#!/usr/bin/env node
'use strict';
// Thin launcher only. npm needs a JS entry point to pick the right native binary; every tool, the
// MCP protocol and file confinement live in the Rust `cadcraft-cli mcp`.
const { spawn, spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { version } = require('../package.json');

const exe = process.platform === 'win32' ? 'cadcraft-cli.exe' : 'cadcraft-cli';
const pkg = `cadcraft-edge-mcp-${process.platform}-${process.arch}`;
const say = (s) => process.stderr.write(`cadcraft-edge-mcp: ${s}\n`);

// The binary of the platform package at exactly this version, found from `from` (default: here).
function installed(from) {
  try {
    const json = require.resolve(`${pkg}/package.json`, from ? { paths: [from] } : undefined);
    if (JSON.parse(fs.readFileSync(json, 'utf8')).version !== version) return null;
    const bin = path.join(path.dirname(json), 'bin', exe);
    return fs.existsSync(bin) ? bin : null;
  } catch {
    return null;
  }
}

// npm skips an optional dependency it can't fetch (a 404 while packages are being published, a
// network error) without failing, and npx then reuses that broken install from its cache for good.
// So a missing platform package is installed once, at this exact version, into a folder of its own.
function installOnce() {
  const dir = path.join(process.env.CADCRAFT_EDGE_MCP_CACHE || path.join(os.homedir(), '.cache', 'cadcraft-edge-mcp'), version);
  const found = installed(dir);
  if (found) return found;
  say(`${pkg}@${version} is missing (npm skipped the optional dependency); installing it into ${dir}`);
  try {
    fs.mkdirSync(dir, { recursive: true });
  } catch (e) {
    say(`could not make ${dir}: ${e.message}`);
    return null;
  }
  // npm's output goes to stderr: stdout is the MCP channel.
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const r = spawnSync(npm, ['install', '--prefix', dir, '--no-save', '--no-audit', '--no-fund', '--no-package-lock', `${pkg}@${version}`], {
    stdio: ['ignore', 2, 2],
    shell: process.platform === 'win32',
  });
  return r.status === 0 ? installed(dir) : null;
}

function binary() {
  if (process.env.CADCRAFT_CLI) return process.env.CADCRAFT_CLI;
  const bin = installed() || installOnce();
  if (bin) return bin;
  const npx = __dirname.match(/^(.*[\\/]_npx[\\/][^\\/]+)[\\/]/);
  say(
    `no CADCraft binary for ${process.platform}-${process.arch}: ${pkg}@${version} isn't installed and couldn't be installed.\n` +
      (npx ? `  npx cached this incomplete install; delete ${npx[1]} and start the server again.\n` : '') +
      `  Or install it yourself: npm install ${pkg}@${version}\n` +
      '  Or set CADCRAFT_CLI to a cadcraft-cli binary (cargo build -p cadcraft-cli --release).',
  );
  process.exit(1);
}

// Confine the agent's files to CADCRAFT_ROOT, else the working directory, unless the caller passed
// a root of its own (`--root DIR` or `--root=DIR`). A shared server is always confined: `--root`
// alone is unrestricted, so `--confine` is added unless `--read-anywhere` already confines writes.
const args = process.argv.slice(2);
if (!args.some((a) => /^--root(=|$)/.test(a))) {
  args.push('--root', process.env.CADCRAFT_ROOT || process.cwd());
}
if (!args.includes('--confine') && !args.includes('--read-anywhere')) args.push('--confine');

const child = spawn(binary(), ['mcp', ...args], { stdio: 'inherit' });
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.on(sig, () => child.kill(sig));
child.on('error', (e) => {
  say(`could not start ${exe}: ${e.message}`);
  process.exit(1);
});
child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
