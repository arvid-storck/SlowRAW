import { t as translate } from 'i18next';
import { addCatalogFolder, normalizeCatalogFilter, showAllCatalog } from '../utils/catalog';
import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { useLibraryStore } from '../store/useLibraryStore';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { Invokes, ImageFile } from '../components/ui/AppProperties';
import { INITIAL_ADJUSTMENTS } from '../utils/adjustments';
import { globalImageCache } from '../utils/ImageLRUCache';
import { debouncedSave, debouncedSetHistory } from './useEditorActions';
import { isVideoPath } from '../utils/mediaTypes';

export interface AppNavigationProps {
  clearThumbnailQueue: () => void;
  refs: {
    transformWrapperRef: React.RefObject<any>;
    cachedEditStateRef: React.RefObject<any>;
    currentResRef: React.RefObject<number>;
    prevAdjustmentsRef: React.RefObject<any>;
  };
}

export function useAppNavigation({ clearThumbnailQueue, refs }: AppNavigationProps) {
  const { transformWrapperRef, cachedEditStateRef, currentResRef, prevAdjustmentsRef } = refs;

  const handleGoHome = useCallback(() => {
    showAllCatalog();
    useUIStore.getState().setUI({ isLibraryExportPanelVisible: false });
  }, []);

  const handleBackToLibrary = useCallback(() => {
    const { selectedImage } = useEditorStore.getState();
    const { setLibrary } = useLibraryStore.getState();
    const { setUI } = useUIStore.getState();

    if (selectedImage?.path && cachedEditStateRef.current) {
      globalImageCache.set(selectedImage.path, cachedEditStateRef.current);
    }
    if (transformWrapperRef.current) {
      transformWrapperRef.current.resetTransform(0);
    }
    useEditorStore.getState().setEditor({
      zoom: 1,
      showOriginal: false,
      previewOverride: null,
    });

    debouncedSave.flush();
    debouncedSetHistory.cancel();

    const lastActivePath = selectedImage?.path ?? null;

    setLibrary({ libraryActivePath: lastActivePath });
    setUI({ activeView: 'library', slideDirection: 1 });
  }, [refs]);

  const handleImageSelect = useCallback(
    async (path: string, openInEditor: boolean = true) => {
      if (isVideoPath(path)) {
        if (openInEditor) {
          try {
            await invoke('open_video_in_system', { path });
          } catch (error) {
            toast.error(translate('messages.useAppNavigation.could_not_open_video', { value1: String(error) }));
          }
        }
        return;
      }
      const { selectedImage, setEditor } = useEditorStore.getState();
      const { setLibrary, multiSelectedPaths } = useLibraryStore.getState();
      const { setUI } = useUIStore.getState();

      if (openInEditor) {
        setUI({ activeView: 'editor' });
      }

      if (selectedImage?.path === path) return;

      debouncedSave.flush();
      debouncedSetHistory.cancel();

      if (selectedImage?.path && cachedEditStateRef.current) {
        globalImageCache.set(selectedImage.path, cachedEditStateRef.current);
      }

      const cachedThumb = useProcessStore.getState().thumbnails[path];
      const cachedMedium = useProcessStore.getState().mediumThumbnails[path] || cachedThumb;

      const cached = globalImageCache.get(path);
      const newMultiSelectedPaths = multiSelectedPaths.includes(path) ? multiSelectedPaths : [path];

      setLibrary({
        multiSelectedPaths: newMultiSelectedPaths,
        libraryActivePath: path,
        selectionAnchorPath: path,
      });

      setEditor({
        showOriginal: false,
        activeMaskId: null,
        activeMaskContainerId: null,
        isWbPickerActive: false,
        previewOverride: null,
      });

      setUI({
        isLibraryExportPanelVisible: false,
      });

      currentResRef.current = 0;
      prevAdjustmentsRef.current = null;
      const imageFile = useLibraryStore.getState().imageList.find((img) => img.path === path);
      setEditor({
        selectedImage: {
          exif: imageFile?.exif ?? cached?.selectedImage?.exif ?? null,
          catalogCamera: imageFile?.camera ?? cached?.selectedImage?.catalogCamera,
          catalogLens: imageFile?.lens ?? cached?.selectedImage?.catalogLens,
          catalogFocalLength: imageFile?.focal_length ?? cached?.selectedImage?.catalogFocalLength,
          catalogFNumber: imageFile?.f_number ?? cached?.selectedImage?.catalogFNumber,
          group_id: imageFile?.group_id ?? null,
          height: cached?.originalSize.height ?? 0,
          isRaw: imageFile?.is_raw ?? cached?.selectedImage?.isRaw ?? false,
          isReady: false,
          metadata: null,
          path,
          thumbnailUrl: cachedMedium || cached?.selectedImage?.thumbnailUrl,
          width: cached?.originalSize.width ?? 0,
        },
        adjustments: cached?.adjustments ?? { ...INITIAL_ADJUSTMENTS },
        originalSize: cached?.originalSize ?? { width: 0, height: 0 },
        previewSize: cached?.previewSize ?? { width: 0, height: 0 },
        histogram: cached?.histogram ?? null,
        waveform: cached?.waveform ?? null,
        uncroppedAdjustedPreviewUrl: null,
        hasRenderedFirstFrame: false,
        previewError: null,
      });

      setLibrary({ isViewLoading: true });

      setEditor((state) => {
        const prev = state.finalPreviewUrl;
        if (prev?.startsWith('blob:') && !globalImageCache.isProtected(prev)) {
          setTimeout(() => {
            if (!globalImageCache.isProtected(prev)) {
              URL.revokeObjectURL(prev);
            }
          }, 250);
        }
        return { finalPreviewUrl: null };
      });
    },
    [refs],
  );

  const handleSelectAlbum = useCallback(
    async (albumId: string, albumName: string, imagePaths: string[], preserveEditor = false, filter?: any) => {
      const { setLibrary } = useLibraryStore.getState();
      const { setUI } = useUIStore.getState();

      if (!preserveEditor) {
        await invoke('cancel_thumbnail_generation');
        clearThumbnailQueue();
        setLibrary({ libraryScrollTop: 0 });
        globalImageCache.clear();
        setUI({ activeView: 'library' });
      }

      setLibrary({
        isViewLoading: true,
        currentFolderPath: `${filter ? 'Smart group' : 'Album'}: ${albumName}`,
        isCatalogMode: Boolean(filter),
        ...(filter
          ? {
              catalogFilter: normalizeCatalogFilter(filter),
              catalogDateFilter: filter.date ?? '',
            }
          : {}),
        activeAlbumId: albumId,
        editingSmartGroupId: null,
      });

      if (filter) return; // Shared catalog loader owns smart-group queries.
      try {
        const files: ImageFile[] = await invoke(Invokes.GetAlbumImages, { paths: imagePaths });
        const current = useLibraryStore.getState();
        if (current.activeAlbumId !== albumId || current.isCatalogMode) return;

        const initialRatings: Record<string, number> = {};
        files.forEach((f) => {
          if (f.rating !== undefined) initialRatings[f.path] = f.rating;
        });

        setLibrary({
          imageList: files,
          imageRatings: initialRatings,
          ...(preserveEditor ? {} : { multiSelectedPaths: [], libraryActivePath: null }),
        });
      } catch (err) {
        if (useLibraryStore.getState().activeAlbumId !== albumId) return;
        console.error('Failed to load album images:', err);
        toast.error(translate('messages.useAppNavigation.failed_to_load_album', { value1: String(err) }));
      } finally {
        if (useLibraryStore.getState().activeAlbumId === albumId) setLibrary({ isViewLoading: false });
      }
    },
    [clearThumbnailQueue],
  );

  const handleOpenFolder = async () => {
    try {
      await addCatalogFolder();
    } catch (error) {
      toast.error(translate('messages.useAppNavigation.could_not_add_folder', { value1: String(error) }));
    }
  };
  return {
    handleGoHome,
    handleBackToLibrary,
    handleImageSelect,
    handleSelectAlbum,
    handleOpenFolder,
  };
}
