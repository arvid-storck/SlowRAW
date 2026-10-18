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
const generatedFiles = [];
if (process.argv.includes('--dependencies')) {
  generatedDirectories.push('node_modules');
  generatedFiles.push(
    join('src-tauri', 'resources', 'onnxruntime.dll'),
    join('src-tauri', 'resources', 'libonnxruntime.so'),
    join('src-tauri', 'resources', 'libonnxruntime.dylib'),
  );
}

for (const relativePath of [...generatedDirectories, ...generatedFiles]) {
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
  const isDirectory = generatedDirectories.includes(relativePath);
  if (!isDirectory && !lstatSync(target).isFile()) {
    throw new Error(`Refusing to clean unexpected non-file: ${target}`);
  }
  console.log(`${dryRun ? 'Would remove' : 'Removing'} ${relativePath}`);
  if (!dryRun) rmSync(target, { recursive: isDirectory, force: true });
}

console.log(dryRun ? 'Dry run complete.' : 'Clean complete.');
