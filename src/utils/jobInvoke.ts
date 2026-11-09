import { invoke as nativeInvoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { enqueueJob, updateJob, type JobKind } from '../store/useJobStore';

const commands: Record<string, { kind: JobKind; done?: string; error?: string; progress?: string }> = {
  export_images: { kind: 'export', done: 'export-complete', error: 'export-error', progress: 'batch-export-progress' },
  import_files: { kind: 'import', done: 'import-complete', progress: 'import-progress' },
  copy_files: { kind: 'copy' },
  move_files: { kind: 'move' },
  apply_auto_adjustments_to_paths: { kind: 'auto' },
  catalog_add_folder: { kind: 'catalog', progress: 'indexing-progress' },
  catalog_rebuild: { kind: 'catalog', progress: 'indexing-progress' },
  batch_denoise_images: { kind: 'denoise' },
  apply_denoising: { kind: 'denoise', done: 'denoise-complete', error: 'denoise-error' },
  stitch_panorama: { kind: 'panorama', done: 'panorama-complete', error: 'panorama-error' },
  merge_hdr: { kind: 'hdr', done: 'hdr-complete', error: 'hdr-error' },
  stitch_focus_stack: { kind: 'focus', done: 'focus-stack-complete', error: 'focus-stack-error' },
};

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const config = commands[command];
  if (!config) return nativeInvoke<T>(command, args);
  const output = args?.outputFolderOrFile === 'originalFolderDummy' ? undefined : args?.outputFolderOrFile;
  const sources = args?.paths as string[] | undefined;
  const path = String(output || args?.destinationFolder || args?.path || sources?.[0] || '');
  return enqueueJob<T>(
    config.kind,
    async (id) => {
      const listeners: UnlistenFn[] = [];
      let finish!: (value?: unknown) => void;
      let fail!: (error: unknown) => void;
      const completion = new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      });
      void completion.catch(() => {});
      try {
        if (config.done)
          listeners.push(
            await listen(config.done, (event) => {
              const result = event.payload as { failed?: number } | undefined;
              if (result?.failed) fail(new Error(`${result.failed} files failed`));
              else finish();
            }),
          );
        if (config.error) listeners.push(await listen(config.error, (event) => fail(event.payload)));
        if (config.progress)
          listeners.push(
            await listen<{ current: number; total: number }>(config.progress, (event) => {
              updateJob(id, { progress: event.payload });
            }),
          );
        if (config.kind === 'export')
          listeners.push(
            await listen('export-cancelled', () => {
              updateJob(id, { status: 'cancelled', cancellable: false });
              finish();
            }),
          );
        const result = await nativeInvoke<T>(command, args);
        if (config.done) await completion;
        return result;
      } finally {
        listeners.forEach((unlisten) => unlisten());
      }
    },
    path,
    config.kind === 'export' ? () => nativeInvoke('cancel_export') : undefined,
  );
}
