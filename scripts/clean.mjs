import { existsSync, lstatSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const packageJson = JSON.parse(readFileSync(join(projectRoot, 'package.json'), 'utf8'));

if (packageJson.name !== 'slowraw' || !existsSync(join(projectRoot, 'src-tauri', 'Cargo.toml'))) {
  throw new Error('Refusing to clean outside the SlowRAW project.');
}

const dryRun = process.argv.includes('--dry-run');
const generatedDirectories = ['dist', 'dist-ssr', join('src-tauri', 'target'), join('src-tauri', 'rcd', 'target')];
if (process.argv.includes('--dependencies')) {
  generatedDirectories.push('node_modules', join('src-tauri', 'resources', 'macos'));
}

for (const relativePath of generatedDirectories) {
  const target = resolve(projectRoot, relativePath);
  if (!target.startsWith(projectRoot + sep)) {
    throw new Error(`Refusing to clean path outside the project: ${target}`);
  }
  if (!existsSync(target)) continue;
  if (lstatSync(target).isSymbolicLink()) {
    throw new Error(`Refusing to clean symbolic link: ${target}`);
  }
  if (!realpathSync(target).startsWith(realpathSync(projectRoot) + sep)) {
    throw new Error(`Refusing to clean resolved path outside the project: ${target}`);
  }
  console.log(`${dryRun ? 'Would remove' : 'Removing'} ${relativePath}`);
  if (!dryRun) rmSync(target, { recursive: true, force: true });
}

console.log(dryRun ? 'Dry run complete.' : 'Clean complete.');
