import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';

const result = execFileSync('git', ['ls-remote', 'https://github.com/CarVac/librtprocess.git', 'HEAD'], {
  encoding: 'utf8',
});
const revision = result.split(/\s+/)[0];
if (!/^[a-f0-9]{40}$/.test(revision)) throw new Error('Invalid upstream revision');
writeFileSync(new URL('../src-tauri/rcd/upstream-revision', import.meta.url), revision + '\n');
console.log(`Pinned librtprocess to ${revision}. Rebuild SlowRAW and run the RCD tests.`);
