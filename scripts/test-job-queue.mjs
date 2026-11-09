import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { create } from 'zustand';

function load(path, dependencies) {
  const source = fs.readFileSync(new URL(path, import.meta.url), 'utf8');
  const exports = {};
  vm.runInNewContext(
    ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } })
      .outputText,
    {
      exports,
      require: (name) => {
        assert.ok(name in dependencies, name);
        return dependencies[name];
      },
    },
  );
  return exports;
}
const jobs = load('../src/store/useJobStore.ts', { zustand: { create } });
const tick = () => new Promise((resolve) => setImmediate(resolve));
const order = [];
let release;
const first = jobs.enqueueJob(
  'copy',
  () =>
    new Promise((resolve) => {
      release = resolve;
      order.push(1);
    }),
);
const cancelled = jobs.enqueueJob('move', async () => {
  throw new Error('Cancelled job executed');
});
const rejected = assert.rejects(cancelled, /cancelled/);
const cancelledId = jobs.useJobStore.getState().jobs.at(-1).id;
const failed = jobs.enqueueJob('auto', async () => {
  order.push(2);
  throw new Error('fixture failure');
});
const failedResult = assert.rejects(failed, /fixture failure/);
const last = jobs.enqueueJob('copy', async () => {
  order.push(3);
  return 42;
});
await jobs.cancelJob(cancelledId);
await rejected;
assert.deepEqual(order, [1]);
jobs.clearFinishedJobs();
assert.equal(jobs.useJobStore.getState().jobs.length, 3);
release();
await first;
await failedResult;
assert.equal(await last, 42);
assert.deepEqual(order, [1, 2, 3]);

const listeners = new Map();
const calls = [];
const emit = (name, payload) => {
  for (const callback of [...(listeners.get(name) || [])]) callback({ payload });
};
const nativeInvoke = async (command) => {
  calls.push(command);
};
const api = load('../src/utils/jobInvoke.ts', {
  '@tauri-apps/api/core': { invoke: nativeInvoke },
  '@tauri-apps/api/event': {
    listen: async (name, callback) => {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(callback);
      return () => listeners.get(name).delete(callback);
    },
  },
  '../store/useJobStore': jobs,
});
const exported = api.invoke('export_images');
const copied = api.invoke('copy_files');
await tick();
assert.deepEqual(calls, ['export_images']);
emit('batch-export-progress', { current: 2, total: 4 });
assert.equal(jobs.useJobStore.getState().jobs.find((job) => job.status === 'running').progress.current, 2);
emit('export-cancelled');
await exported;
await copied;
assert.deepEqual(calls, ['export_images', 'copy_files']);
assert.ok(jobs.useJobStore.getState().jobs.some((job) => job.kind === 'export' && job.status === 'cancelled'));
assert.ok([...listeners.values()].every((set) => set.size === 0));

const imported = api.invoke('import_files');
const importError = assert.rejects(imported, /2 files failed/);
const next = api.invoke('move_files');
await tick();
assert.equal(calls.at(-1), 'import_files');
emit('import-complete', { failed: 2 });
await importError;
await next;
assert.equal(calls.at(-1), 'move_files');
for (let i = 0; i < 60; i++) await jobs.enqueueJob('copy', async () => {});
assert.equal(jobs.useJobStore.getState().jobs.length, 50);
jobs.clearFinishedJobs();
assert.equal(jobs.useJobStore.getState().jobs.length, 0);
console.log(
  'Passed FIFO, pending cancellation, failure continuation, background completion/progress, listener cleanup and bounded history checks.',
);
