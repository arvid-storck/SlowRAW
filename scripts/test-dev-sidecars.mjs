import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createServer, normalizePath } from 'vite';
import tailwindcss from '@tailwindcss/vite';

const root = await mkdtemp(path.join(os.tmpdir(), 'slowraw-sidecar-hmr-'));
let server;
try {
  await mkdir(path.join(root, 'src'));
  await mkdir(path.join(root, 'photos'));
  const sidecar = path.join(root, 'photos', 'photo.DNG.rrdata');
  await writeFile(sidecar, '{"adjustments":{"exposure":0}}');
  await writeFile(path.join(root, 'src', 'App.tsx'), '<div className="flex p-4" />');
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');
  const imports = styles
    .slice(0, styles.indexOf('@theme'))
    .replace("'tailwindcss'", JSON.stringify(normalizePath(path.resolve('node_modules/tailwindcss/index.css'))));
  await writeFile(path.join(root, 'src', 'styles.css'), imports);
  server = await createServer({
    root,
    configFile: false,
    plugins: [tailwindcss()],
    logLevel: 'silent',
    server: { middlewareMode: true, hmr: false, watch: null },
  });
  const environment = server.environments.client;
  const css = await environment.transformRequest('/src/styles.css');
  assert.ok(css.code.includes('.flex') && css.code.includes('.p-4'), 'UI source classes must still be generated');
  const modules = [...(environment.moduleGraph.getModulesByFile(normalizePath(sidecar)) || [])];
  const messages = [];
  environment.hot.send = (message) => messages.push(message);
  const plugin = server.config.plugins.find((plugin) => plugin.name === '@tailwindcss/vite:generate:serve');
  assert.ok(plugin?.hotUpdate, 'Use the installed Tailwind development update handler');
  await plugin.hotUpdate.call(
    { environment },
    {
      file: normalizePath(sidecar),
      modules,
      timestamp: Date.now(),
      server,
      type: 'update',
      read: () => Promise.resolve('{"adjustments":{"exposure":1}}'),
    },
  );
  assert.ok(
    !messages.some((message) => message.type === 'full-reload'),
    `Saving a photo sidecar must not reload the editor (${modules.length} watched asset modules)`,
  );
  console.log('Saving photo metadata does not trigger a Tailwind/Vite page reload.');
} finally {
  await server?.close();
  const resolved = path.resolve(root);
  assert.equal(path.dirname(resolved), path.resolve(os.tmpdir()));
  assert.ok(path.basename(resolved).startsWith('slowraw-sidecar-hmr-'));
  await rm(resolved, { recursive: true, force: true });
}
