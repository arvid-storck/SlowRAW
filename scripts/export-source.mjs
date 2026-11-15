import { execFileSync } from 'node:child_process';
import { constants, copyFileSync, lstatSync, mkdirSync, mkdtempDisposableSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const files = [
  '.gitignore',
  '.prettierignore',
  '.prettierrc',
  'eslint.config.js',
  'i18next.config.ts',
  'index.html',
  'LICENSE',
  'package-lock.json',
  'package.json',
  'README.md',
  'tsconfig.json',
  'vite.config.mjs',
  'src-tauri/.gitignore',
  'src-tauri/.taurignore',
  'src-tauri/build.rs',
  'src-tauri/Cargo.lock',
  'src-tauri/Cargo.toml',
  'src-tauri/rust-toolchain.toml',
  'src-tauri/tauri.conf.json',
  'src-tauri/tauri.linux.conf.json',
  'src-tauri/tauri.macos.conf.json',
  'src-tauri/rawler/build.rs',
  'src-tauri/rawler/Cargo.toml',
  'src-tauri/rawler/LICENSE',
  'src-tauri/rawler/nikon-he.patch',
  'src-tauri/rawler/upstream-revision',
  'src-tauri/rcd/bridge.cpp',
  'src-tauri/rcd/build.rs',
  'src-tauri/rcd/Cargo.toml',
  'src-tauri/rcd/upstream-revision',
];

const sourceDirectories = {
  '.github': ['.md', '.yml', '.yaml'],
  data: ['.desktop', '.xml'],
  docs: ['.md'],
  packaging: ['.json', '.yml', '.yaml'],
  scripts: ['.mjs', '.ps1'],
  src: ['.css', '.json', '.mjs', '.ts', '.tsx'],
  'src-tauri/capabilities': ['.json', '.toml'],
  'src-tauri/icc': ['.icc', '.md'],
  'src-tauri/icons': ['.icns', '.ico', '.png'],
  'src-tauri/lensfun_db': ['.dtd', '.txt', '.xml', '.xsd'],
  'src-tauri/rawler/src': ['.rs'],
  'src-tauri/rcd/src': ['.rs'],
  'src-tauri/resources/licenses': ['.txt'],
  'src-tauri/src': ['.rs', '.wgsl'],
};

function collect(directory, extensions) {
  for (const entry of readdirSync(join(root, directory), { withFileTypes: true })) {
    if (entry.name.startsWith('.')) continue;
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) collect(path, extensions);
    else if (entry.isFile() && extensions.includes(extname(entry.name))) files.push(path);
  }
}

for (const file of files) {
  if (!lstatSync(join(root, file), { throwIfNoEntry: false })?.isFile()) {
    throw new Error(`Missing source file: ${file}`);
  }
}
if (lstatSync(join(root, 'icon.png'), { throwIfNoEntry: false })?.isFile()) files.push('icon.png');
for (const [directory, extensions] of Object.entries(sourceDirectories)) {
  const entry = lstatSync(join(root, directory), { throwIfNoEntry: false });
  if (!entry && directory === '.github') continue;
  if (!entry?.isDirectory()) throw new Error(`Missing source directory: ${directory}`);
  collect(directory, extensions);
}

if (files.some((path) => /[\r\n]/.test(path))) throw new Error('Archive paths must fit on one line.');
const input = `${files.sort().join('\n')}\n`;
const outputDirectory = join(root, 'artifacts-source');
const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
const name = `SlowRAW-source-${timestamp}.zip`;
const staging = mkdtempDisposableSync(join(tmpdir(), 'slowraw-source-'));
try {
  const archive = join(staging.path, name);
  const [program, args] =
    process.platform === 'win32' || process.platform === 'darwin'
      ? ['tar', ['--format', 'zip', '-cf', archive, '-T', '-']]
      : ['zip', ['-q', '-X', archive, '-@']];
  execFileSync(program, args, { cwd: root, input, stdio: ['pipe', 'inherit', 'inherit'] });
  mkdirSync(outputDirectory, { recursive: true });
  if (!lstatSync(outputDirectory).isDirectory()) throw new Error('Archive output must be a local directory.');
  const output = join(outputDirectory, name);
  copyFileSync(archive, output, constants.COPYFILE_EXCL);
  console.log(`${output} (${files.length} files)`);
} finally {
  staging.remove();
}
