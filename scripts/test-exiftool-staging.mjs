import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { stageExiftool } from './update-exiftool.mjs';

const root = await mkdtemp(path.join(os.tmpdir(), 'slowraw-exiftool-staging-'));
try {
  const badArchive = path.join(root, 'invalid.zip');
  await writeFile(badArchive, 'not the official archive');
  for (const platform of ['windows', 'macos', 'linux']) {
    const destination = path.join(root, platform);
    await assert.rejects(stageExiftool({ platform, destination, archive: badArchive }), /checksum mismatch/);
    assert.ok(!(await readdir(root)).includes(platform), 'A rejected download must not publish a runtime');
    assert.ok(
      !(await readdir(root)).some((entry) => entry.startsWith('.exiftool-staging-')),
      'Failed staging must be cleaned',
    );
    const windows = platform === 'windows';
    const library = path.join(destination, windows ? 'exiftool_files/lib/Image' : 'lib/Image');
    await mkdir(library, { recursive: true });
    await writeFile(path.join(destination, windows ? 'exiftool.exe' : 'exiftool'), 'existing executable');
    await writeFile(path.join(library, 'ExifTool.pm'), 'existing library');
    assert.equal((await stageExiftool({ platform, destination, archive: badArchive, ensure: true })).reused, true);
    assert.equal(await readFile(path.join(library, 'ExifTool.pm'), 'utf8'), 'existing library');
  }
  const incomplete = path.join(root, 'incomplete');
  await mkdir(incomplete);
  await writeFile(path.join(incomplete, 'keep.txt'), 'preserve');
  await assert.rejects(stageExiftool({ destination: incomplete, ensure: true }), /already exists or is incomplete/);
  assert.equal(await readFile(path.join(incomplete, 'keep.txt'), 'utf8'), 'preserve');
  console.log('ExifTool staging: all platform layouts, offline reuse, checksum rejection and cleanup passed.');
} finally {
  assert.equal(path.dirname(path.resolve(root)), path.resolve(os.tmpdir()));
  assert.ok(path.basename(root).startsWith('slowraw-exiftool-staging-'));
  await rm(root, { recursive: true, force: true });
}
