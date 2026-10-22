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
import { INITIAL_ADJUSTMENTS, normalizeLoadedAdjustments } from '../utils/adjustments';
import { globalImageCache } from '../utils/ImageLRUCache';
import { debouncedSave, debouncedSetHistory } from './useEditorActions';
import { isVideoPath } from '../utils/mediaTypes';

export interface AppNavigationProps {
  clearThumbnailQueue: () => void;
  refs: {
    transformWrapperRef: React.RefObject<any>;
    cachedEditStateRef: React.RefObject<any>;
    selectedImagePathRef: React.RefObject<string | null>;
    isBackendReadyRef: React.RefObject<boolean>;
    latestRenderedJobIdRef: React.RefObject<number>;
    previewJobIdRef: React.RefObject<number>;
    currentResRef: React.RefObject<number>;
    prevAdjustmentsRef: React.RefObject<any>;
  };
}

export function useAppNavigation({ clearThumbnailQueue, refs }: AppNavigationProps) {
  const {
    transformWrapperRef,
    cachedEditStateRef,
    selectedImagePathRef,
    isBackendReadyRef,
    latestRenderedJobIdRef,
    previewJobIdRef,
    currentResRef,
    prevAdjustmentsRef,
  } = refs;

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
      const { selectedImage, isSliderDragging, resetHistory, setEditor } = useEditorStore.getState();
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
      const isFrontendCached = Boolean(cached?.selectedImage?.isReady);
      const isCachedInBackend = isFrontendCached
        ? await invoke<boolean>('is_image_cached', { path }).catch(() => false)
        : false;

      const hasDifferentResolution =
        cached &&
        (useEditorStore.getState().originalSize.width !== cached.originalSize.width ||
          useEditorStore.getState().originalSize.height !== cached.originalSize.height);

      if (!isCachedInBackend || hasDifferentResolution) {
        setEditor({ hasRenderedFirstFrame: false });
      }

      selectedImagePathRef.current = path;

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

      if (cached?.selectedImage?.isReady) {
        setEditor({
          selectedImage: {
            ...cached.selectedImage,
            thumbnailUrl: cachedMedium || cached.selectedImage.thumbnailUrl,
          },
          originalSize: cached.originalSize,
          previewSize: cached.previewSize,
          histogram: cached.histogram,
          waveform: cached.waveform,
          finalPreviewUrl: cached.finalPreviewUrl,
          uncroppedAdjustedPreviewUrl: cached.uncroppedPreviewUrl,
        });

        setEditor({ adjustments: cached.adjustments });
        resetHistory(cached.adjustments);
        prevAdjustmentsRef.current = { path, adjustments: cached.adjustments };

        setLibrary({ isViewLoading: false });

        latestRenderedJobIdRef.current = previewJobIdRef.current;
        isBackendReadyRef.current = false;
        currentResRef.current = Infinity;

        invoke(Invokes.LoadImage, { path })
          .then((_result: any) => {
            if (selectedImagePathRef.current !== path) return;
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
            setEditor({ originalSize: { width: _result.width, height: _result.height } });
          })
          .catch((err: any) => {
            if (String(err).includes('cancelled')) return;
            console.error('Background load_image failed on cache hit:', err);
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
          });

        invoke(Invokes.LoadMetadata, { path })
          .then((metadata: any) => {
            if (selectedImagePathRef.current !== path) return;
            let freshAdjustments: any;
            if (metadata.adjustments && !metadata.adjustments.is_null) {
              freshAdjustments = normalizeLoadedAdjustments(metadata.adjustments);
            } else {
              freshAdjustments = { ...INITIAL_ADJUSTMENTS };
            }
            if (!isSliderDragging && JSON.stringify(cached.adjustments) !== JSON.stringify(freshAdjustments)) {
              setEditor({ adjustments: freshAdjustments });
              resetHistory(freshAdjustments);
              prevAdjustmentsRef.current = { path, adjustments: freshAdjustments };
              globalImageCache.set(path, { ...cached, adjustments: freshAdjustments });
            }
          })
          .catch((err) => console.error('Failed background metadata sync on cache hit:', err));

        return;
      }

      isBackendReadyRef.current = true;

      const imageFile = useLibraryStore.getState().imageList.find((img) => img.path === path);
      setEditor({
        selectedImage: {
          exif: null,
          group_id: imageFile?.group_id ?? null,
          height: 0,
          isRaw: false,
          isReady: false,
          metadata: null,
          path,
          thumbnailUrl: cachedMedium,
          width: 0,
        },
        originalSize: { width: 0, height: 0 },
        previewSize: { width: 0, height: 0 },
        histogram: null,
        waveform: null,
        uncroppedAdjustedPreviewUrl: null,
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
        catalogPage: 0,
        ...(filter
          ? {
              catalogFilter: normalizeCatalogFilter(filter),
              catalogDateFilter: filter.date ?? '',
            }
          : {}),
        activeAlbumId: albumId,
        editingSmartGroupId: null,
      });

      if (filter) return; // Shared catalog loader owns paged smart-group queries.
      try {
        const files: ImageFile[] = await invoke(Invokes.GetAlbumImages, { paths: imagePaths });

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
        console.error('Failed to load album images:', err);
        toast.error(translate('messages.useAppNavigation.failed_to_load_album', { value1: String(err) }));
      } finally {
        setLibrary({ isViewLoading: false });
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
