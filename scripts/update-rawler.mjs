import { execFileSync } from 'node:child_process';
import { mkdtempDisposableSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const upstream = 'https://github.com/dnglab/dnglab.git';
const manifest = join(root, 'src-tauri', 'Cargo.toml');
const lockfile = join(root, 'src-tauri', 'Cargo.lock');
const adapter = join(root, 'src-tauri', 'rawler');
const pin = join(adapter, 'upstream-revision');
const args = process.argv.slice(2);
const removePatch = args[0] === '--remove-patch';
if (args.length && (!removePatch || args.length !== 1)) {
  throw new Error('Usage: npm run update:rawler [-- --remove-patch]');
}

const revision = execFileSync('git', ['ls-remote', upstream, 'HEAD'], { encoding: 'utf8' }).split(/\s+/)[0];
if (!/^[a-f0-9]{40}$/.test(revision)) throw new Error('Invalid upstream Rawler revision');

const originalManifest = readFileSync(manifest, 'utf8');
const patched = 'rawler = { path = "rawler" }';
const dependency =
  /^rawler = \{ (?:path = "rawler"|git = "https:\/\/github\.com\/dnglab\/dnglab\.git", rev = "[a-f0-9]{40}") \}$/m;
if (!dependency.test(originalManifest)) throw new Error('Unexpected Rawler dependency in Cargo.toml');

if (!removePatch) {
  const staging = mkdtempDisposableSync(join(tmpdir(), 'slowraw-rawler-'));
  const git = (...params) => execFileSync('git', ['-c', 'core.autocrlf=false', ...params], { cwd: staging.path });
  try {
    git('init', '--quiet');
    git('fetch', '--quiet', '--depth=1', upstream, revision);
    git('checkout', '--quiet', '--detach', 'FETCH_HEAD');
    git('apply', '--check', join(adapter, 'nikon-he.patch'));
  } finally {
    staging.remove();
  }
}

const originalLock = readFileSync(lockfile);
const originalPin = readFileSync(pin);
const next = removePatch ? `rawler = { git = "${upstream}", rev = "${revision}" }` : patched;
try {
  if (!removePatch) writeFileSync(pin, `${revision}\n`);
  if (originalManifest.replace(dependency, next) !== originalManifest) {
    writeFileSync(manifest, originalManifest.replace(dependency, next));
    execFileSync('cargo', ['metadata', '--manifest-path', manifest, '--format-version', '1'], {
      cwd: root,
      stdio: ['ignore', 'ignore', 'inherit'],
    });
  }
} catch (error) {
  writeFileSync(manifest, originalManifest);
  writeFileSync(lockfile, originalLock);
  writeFileSync(pin, originalPin);
  throw error;
}
console.log(`Pinned Rawler to ${revision}${removePatch ? ' (upstream)' : ' (Nikon HE/HE* patch)'}.`);
