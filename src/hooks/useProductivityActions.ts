import { useCallback } from 'react';
import { invoke } from '../utils/jobInvoke';
import { useUIStore } from '../store/useUIStore';
import { Invokes } from '../components/ui/AppProperties';

const imageMergeCommands = {
  panorama: {
    stateKey: 'panoramaModalState',
    start: Invokes.StitchPanorama,
    save: Invokes.SavePanorama,
    progress: 'Starting panorama process...',
    name: 'panorama',
  },
  hdr: {
    stateKey: 'hdrModalState',
    start: Invokes.MergeHdr,
    save: Invokes.SaveHdr,
    progress: 'Starting HDR process...',
    name: 'HDR',
  },
  focusStack: {
    stateKey: 'focusStackModalState',
    start: Invokes.StitchFocusStack,
    save: Invokes.SaveFocusStack,
    progress: 'Starting focus stacking process...',
    name: 'focus stack',
  },
} as const;

export type ImageMergeKind = keyof typeof imageMergeCommands;

export function useProductivityActions(refreshImageList: () => Promise<void>) {
  const setUI = useUIStore((state) => state.setUI);

  const handleStartImageMerge = useCallback(
    (kind: ImageMergeKind, paths: string[]) => {
      const { stateKey, start, progress } = imageMergeCommands[kind];
      setUI((state) => ({
        [stateKey]: {
          ...state[stateKey],
          isProcessing: true,
          error: null,
          finalImageBase64: null,
          progressMessage: progress,
          ...(kind === 'focusStack' ? { depthMapBase64: null } : {}),
        },
      }));
      invoke(start, { paths }).catch((error) => {
        setUI((state) => ({
          [stateKey]: { ...state[stateKey], isProcessing: false, error: String(error) },
        }));
      });
    },
    [setUI],
  );

  const handleSaveImageMerge = useCallback(
    async (kind: ImageMergeKind): Promise<string> => {
      const { stateKey, save, name } = imageMergeCommands[kind];
      const modal = useUIStore.getState()[stateKey];
      const paths = 'sourcePaths' in modal ? modal.sourcePaths : modal.stitchingSourcePaths;
      if (paths.length === 0) {
        const message = `Source paths for ${name} not found.`;
        setUI((state) => ({ [stateKey]: { ...state[stateKey], error: message } }));
        throw new Error(message);
      }
      try {
        const savedPath = await invoke<string>(save, { firstPathStr: paths[0] });
        await refreshImageList();
        return savedPath;
      } catch (error) {
        console.error(`Failed to save ${kind === 'hdr' ? 'HDR image' : name}:`, error);
        setUI((state) => ({ [stateKey]: { ...state[stateKey], error: String(error) } }));
        throw error;
      }
    },
    [refreshImageList, setUI],
  );

  const handleApplyDenoise = useCallback(
    async (intensity: number) => {
      const { denoiseModalState } = useUIStore.getState();
      if (denoiseModalState.targetPaths.length === 0) return;

      setUI((state) => ({
        denoiseModalState: {
          ...state.denoiseModalState,
          isProcessing: true,
          error: null,
          progressMessage: 'Starting engine...',
        },
      }));

      try {
        await invoke(Invokes.ApplyDenoising, {
          path: denoiseModalState.targetPaths[0],
          intensity: intensity,
        });
      } catch (err) {
        setUI((state) => ({
          denoiseModalState: { ...state.denoiseModalState, isProcessing: false, error: String(err) },
        }));
      }
    },
    [setUI],
  );

  const handleBatchDenoise = useCallback(
    async (intensity: number, paths: string[]) => {
      try {
        const savedPaths: string[] = await invoke('batch_denoise_images', { paths, intensity });
        await refreshImageList();
        return savedPaths;
      } catch (err) {
        setUI((state) => ({ denoiseModalState: { ...state.denoiseModalState, error: String(err) } }));
        throw err;
      }
    },
    [refreshImageList, setUI],
  );

  const handleSaveDenoisedImage = useCallback(async (): Promise<string> => {
    const { denoiseModalState } = useUIStore.getState();
    if (denoiseModalState.targetPaths.length === 0) throw new Error('No target path');
    const savedPath = await invoke<string>(Invokes.SaveDenoisedImage, {
      originalPathStr: denoiseModalState.targetPaths[0],
    });
    await refreshImageList();
    return savedPath;
  }, [refreshImageList]);

  const handleSaveCollage = useCallback(
    async (base64Data: string, firstPath: string): Promise<string> => {
      try {
        const savedPath: string = await invoke(Invokes.SaveCollage, { base64Data, firstPathStr: firstPath });
        await refreshImageList();
        return savedPath;
      } catch (err) {
        console.error('Failed to save collage:', err);
        throw err;
      }
    },
    [refreshImageList],
  );

  return {
    handleStartImageMerge,
    handleSaveImageMerge,
    handleApplyDenoise,
    handleBatchDenoise,
    handleSaveDenoisedImage,
    handleSaveCollage,
  };
}
