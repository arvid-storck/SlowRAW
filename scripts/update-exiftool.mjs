import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { access, chmod, cp, mkdir, mkdtemp, readFile, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const run = promisify(execFile);
const workspace = fileURLToPath(new URL('../', import.meta.url));
export const VERSION = '13.60';
export const DISTRIBUTIONS = {
  windows: {
    file: `exiftool-${VERSION}_64.zip`,
    sha256: '548b1f4e0b713cd2a26824b34080c35db4e2d2edd166468eb38b1d55ada90c2e',
  },
  unix: {
    file: `Image-ExifTool-${VERSION}.tar.gz`,
    sha256: 'c8e3c42576f46d862e4cd9c192989776e7363931e890c95056bcd6293e83ea36',
  },
};
const exists = async (file) =>
  access(file).then(
    () => true,
    () => false,
  );
const isFile = async (file) =>
  stat(file).then(
    (entry) => entry.isFile(),
    () => false,
  );

export async function stageExiftool({
  platform = process.platform,
  destination = path.join(workspace, 'src-tauri/resources/exiftool'),
  archive,
  ensure = false,
} = {}) {
  if (!['win32', 'windows', 'darwin', 'macos', 'linux'].includes(platform))
    throw new Error(`Unsupported ExifTool platform: ${platform}`);
  const windows = platform === 'windows' || platform === 'win32';
  const distribution = DISTRIBUTIONS[windows ? 'windows' : 'unix'];
  destination = path.resolve(destination);
  const launcher = windows ? 'exiftool.exe' : 'exiftool';
  const library = windows ? 'exiftool_files' : 'lib';
  const complete =
    (await isFile(path.join(destination, launcher))) &&
    (await isFile(path.join(destination, library, windows ? 'lib/Image/ExifTool.pm' : 'Image/ExifTool.pm')));
  if (complete && ensure) return { destination, reused: true };
  if (await exists(destination))
    throw new Error(
      `ExifTool destination already exists or is incomplete: ${destination}. Preserve or move it before staging a replacement.`,
    );
  const parent = path.dirname(destination);
  await mkdir(parent, { recursive: true });
  const staging = await mkdtemp(path.join(parent, '.exiftool-staging-'));
  try {
    const download = archive ? path.resolve(archive) : path.join(staging, distribution.file);
    const url = `https://downloads.sourceforge.net/project/exiftool/${distribution.file}`;
    if (!archive) {
      console.log(`Downloading official ExifTool ${VERSION} (${platform})`);
      await run(
        process.platform === 'win32' ? 'curl.exe' : 'curl',
        [
          '--fail',
          '--location',
          '--retry',
          '2',
          '--connect-timeout',
          '20',
          '--max-time',
          '180',
          '--output',
          download,
          url,
        ],
        { windowsHide: true },
      );
    }
    const digest = createHash('sha256')
      .update(await readFile(download))
      .digest('hex');
    if (digest !== distribution.sha256) throw new Error('Official ExifTool archive checksum mismatch');
    const extracted = path.join(staging, 'archive');
    await mkdir(extracted);
    if (windows && process.platform === 'win32') {
      const quote = (value) => `'${value.replaceAll("'", "''")}'`;
      await run(
        'powershell.exe',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `Expand-Archive -LiteralPath ${quote(download)} -DestinationPath ${quote(extracted)}`,
        ],
        { windowsHide: true },
      );
    } else {
      await run(
        windows ? 'unzip' : 'tar',
        windows ? ['-q', download, '-d', extracted] : ['-xzf', download, '-C', extracted],
        { windowsHide: true },
      );
    }
    const candidates = [
      extracted,
      ...(await readdir(extracted, { withFileTypes: true }))
        .filter((entry) => entry.isDirectory())
        .map((entry) => path.join(extracted, entry.name)),
    ];
    const sourceLauncher = windows ? 'exiftool(-k).exe' : 'exiftool';
    let source;
    for (const candidate of candidates)
      if (await exists(path.join(candidate, sourceLauncher))) {
        source = candidate;
        break;
      }
    if (!source) throw new Error('Official ExifTool archive lacks its launcher');
    const prepared = path.join(staging, 'runtime');
    await mkdir(prepared);
    await cp(path.join(source, sourceLauncher), path.join(prepared, launcher));
    await cp(path.join(source, library), path.join(prepared, library), { recursive: true });
    for (const entry of await readdir(source)) {
      if (/^(readme|licen[cs]e|copying|artistic)/i.test(entry))
        await cp(path.join(source, entry), path.join(prepared, entry), { recursive: true });
    }
    if (!windows) await chmod(path.join(prepared, launcher), 0o755);
    // Check native distributions before publishing. Cross-staging still verifies
    // the pinned archive and library version, without executing a foreign binary.
    const module = await readFile(
      path.join(prepared, library, windows ? 'lib/Image/ExifTool.pm' : 'Image/ExifTool.pm'),
      'utf8',
    );
    if (!module.includes(`$VERSION = '${VERSION}'`) && !module.includes(`$VERSION = "${VERSION}"`))
      throw new Error('ExifTool library version mismatch');
    if (windows === (process.platform === 'win32')) {
      const { stdout } = await run(
        windows ? path.join(prepared, launcher) : 'perl',
        windows ? ['-ver'] : [path.join(prepared, launcher), '-ver'],
        { windowsHide: true },
      );
      if (stdout.trim() !== VERSION) throw new Error('ExifTool version check failed');
    }
    await writeFile(path.join(prepared, 'SOURCE.txt'), `Official ExifTool ${VERSION}\n${url}\nSHA256: ${digest}\n`);
    await rename(prepared, destination);
    return { destination, reused: false };
  } finally {
    if (path.dirname(staging) !== parent || !path.basename(staging).startsWith('.exiftool-staging-'))
      throw new Error('Unexpected staging cleanup path');
    await rm(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = {};
  for (let i = 2; i < process.argv.length; i++) {
    const argument = process.argv[i];
    if (argument === '--ensure') options.ensure = true;
    else if (['--platform', '--destination', '--archive'].includes(argument) && process.argv[i + 1])
      options[argument.slice(2)] = process.argv[++i];
    else throw new Error(`Unknown or incomplete ExifTool option: ${argument}`);
  }
  try {
    const result = await stageExiftool(options);
    console.log(`${result.reused ? 'Reusing' : 'Staged'} official ExifTool at ${result.destination}`);
  } catch (error) {
    console.error(`ExifTool preparation failed: ${error.message}`);
    process.exitCode = 1;
  }
}
