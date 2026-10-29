import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const root = mkdtempSync(path.join(os.tmpdir(), 'slowraw-clean-'));
try {
  const resources = path.join(root, 'src-tauri', 'resources');
  mkdirSync(path.join(root, 'scripts'), { recursive: true });
  mkdirSync(path.join(resources, 'licenses'), { recursive: true });
  writeFileSync(path.join(root, 'package.json'), '{"name":"slowraw"}');
  writeFileSync(path.join(root, 'src-tauri', 'Cargo.toml'), '');
  writeFileSync(path.join(resources, 'exiftool.config'), 'keep configuration');
  writeFileSync(path.join(resources, 'licenses', 'notice.txt'), 'keep license');
  for (const directory of ['.exiftool-staging-interrupted', '.exiftool-staging-download', 'exiftool']) {
    mkdirSync(path.join(resources, directory));
    writeFileSync(path.join(resources, directory, 'file'), 'generated');
  }
  const script = path.join(root, 'scripts', 'clean.mjs');
  cpSync(new URL('./clean.mjs', import.meta.url), script);
  const run = (...args) => execFileSync(process.execPath, [script, ...args], { encoding: 'utf8', windowsHide: true });
  const dryRun = run('--dependencies', '--dry-run');
  assert.ok(dryRun.includes('.exiftool-staging-interrupted'));
  assert.ok(existsSync(path.join(resources, '.exiftool-staging-interrupted', 'file')));
  run();
  assert.ok(!existsSync(path.join(resources, '.exiftool-staging-interrupted')));
  assert.ok(!existsSync(path.join(resources, '.exiftool-staging-download')));
  assert.ok(existsSync(path.join(resources, 'exiftool', 'file')));
  run('--dependencies');
  assert.ok(!existsSync(path.join(resources, 'exiftool')));
  assert.equal(readFileSync(path.join(resources, 'exiftool.config'), 'utf8'), 'keep configuration');
  assert.equal(readFileSync(path.join(resources, 'licenses', 'notice.txt'), 'utf8'), 'keep license');
  console.log('Clean: interrupted downloads removed; runtime kept by clean and removed by clean:deep; configuration and licenses preserved.');
} finally {
  assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
  assert.ok(path.basename(root).startsWith('slowraw-clean-'));
  rmSync(root, { recursive: true, force: true });
}
