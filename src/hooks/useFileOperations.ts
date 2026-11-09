import { t as translate } from 'i18next';
import { useCallback } from 'react';
import { invoke } from '../utils/jobInvoke';
import { open } from '@tauri-apps/plugin-dialog';
import { toast } from 'react-toastify';
import { useLibraryStore } from '../store/useLibraryStore';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { Invokes } from '../components/ui/AppProperties';
import { Status } from '../components/ui/ExportImportProperties';

export function useFileOperations(
  refreshImageList: () => Promise<void>,
  refreshCatalogIndex: () => Promise<void>,
  handleImageSelect: (path: string) => void,
  handleBackToLibrary: () => void,
  sortedImageList: any[],
) {
  const executeDelete = useCallback(
    async (pathsToDelete: Array<string>, options = { includeAssociated: false }) => {
      if (!pathsToDelete || pathsToDelete.length === 0) return;

      const { libraryActivePath, isCatalogMode, setLibrary } = useLibraryStore.getState();
      const { selectedImage } = useEditorStore.getState();
      const { activeView } = useUIStore.getState(); // <-- Check active view

      // Determine the active path based on the current view
      const activePath = selectedImage && activeView === 'editor' ? selectedImage.path : libraryActivePath;
      let nextImagePath: string | null = null;

      if (activePath) {
        const physicalPath = activePath.split('?vc=')[0];
        const isActiveImageDeleted = pathsToDelete.some((p) => p === activePath || p === physicalPath);

        if (isActiveImageDeleted) {
          const currentIndex = sortedImageList.findIndex((img) => img.path === activePath);
          if (currentIndex !== -1) {
            const nextCandidate = sortedImageList
              .slice(currentIndex + 1)
              .find((img) => !pathsToDelete.includes(img.path));

            if (nextCandidate) {
              nextImagePath = nextCandidate.path;
            } else {
              const prevCandidate = sortedImageList
                .slice(0, currentIndex)
                .reverse()
                .find((img) => !pathsToDelete.includes(img.path));

              if (prevCandidate) {
                nextImagePath = prevCandidate.path;
              }
            }
          }
        } else {
          nextImagePath = activePath;
        }
      }

      try {
        const command = options.includeAssociated ? 'delete_files_with_associated' : 'delete_files_from_disk';
        await invoke(command, { paths: pathsToDelete });
        if (isCatalogMode) {
          await invoke(Invokes.CatalogRemoveImages, { paths: pathsToDelete });
          setLibrary((state) => ({
            imageList: state.imageList.filter((image) => !pathsToDelete.includes(image.path)),
          }));
          await refreshCatalogIndex();
        } else {
          await refreshImageList();
        }

        if (selectedImage && activeView === 'editor') {
          const physicalPath = selectedImage.path.split('?vc=')[0];
          const isFileBeingEditedDeleted = pathsToDelete.some((p) => p === selectedImage.path || p === physicalPath);

          if (isFileBeingEditedDeleted) {
            if (nextImagePath) {
              handleImageSelect(nextImagePath);
            } else {
              handleBackToLibrary();
            }
          }
        } else {
          if (nextImagePath) {
            setLibrary({ multiSelectedPaths: [nextImagePath], libraryActivePath: nextImagePath });
          } else {
            setLibrary({ multiSelectedPaths: [], libraryActivePath: null });
          }

          if (selectedImage) {
            const physicalPath = selectedImage.path.split('?vc=')[0];
            if (pathsToDelete.some((p) => p === selectedImage.path || p === physicalPath)) {
              useEditorStore.getState().setEditor({ selectedImage: null });
            }
          }
        }
      } catch (err) {
        console.error('Failed to delete files:', err);
        toast.error(translate('messages.useFileOperations.failed_to_delete_files', { value1: String(err) }));
      }
    },
    [refreshImageList, refreshCatalogIndex, handleBackToLibrary, sortedImageList, handleImageSelect],
  );

  const handleDeleteSelected = useCallback(() => {
    const { multiSelectedPaths, imageList } = useLibraryStore.getState();
    const { setUI } = useUIStore.getState();

    const pathsToDelete = multiSelectedPaths;
    if (pathsToDelete.length === 0) {
      return;
    }

    const isSingle = pathsToDelete.length === 1;

    const selectionHasVirtualCopies =
      isSingle &&
      !pathsToDelete[0].includes('?vc=') &&
      imageList.some((image) => image.path.startsWith(`${pathsToDelete[0]}?vc=`));

    let modalTitle = 'Confirm Delete';
    let modalMessage: string;
    let confirmText: string;

    if (selectionHasVirtualCopies) {
      modalTitle = 'Delete Image and All Virtual Copies?';
      modalMessage = `Are you sure you want to permanently delete this image and all of its virtual copies? This action cannot be undone.`;
      confirmText = 'Delete All';
    } else if (isSingle) {
      modalMessage = `Are you sure you want to permanently delete this image? This action cannot be undone. Right-click for more options (e.g., deleting associated files).`;
      confirmText = 'Delete Selected Only';
    } else {
      modalMessage = `Are you sure you want to permanently delete these ${pathsToDelete.length} images? This action cannot be undone. Right-click for more options (e.g., deleting associated files).`;
      confirmText = 'Delete Selected Only';
    }

    setUI({
      confirmModalState: {
        confirmText,
        confirmVariant: 'destructive',
        isOpen: true,
        message: modalMessage,
        onConfirm: () => executeDelete(pathsToDelete, { includeAssociated: false }),
        title: modalTitle,
      },
    });
  }, [executeDelete]);

  const handleSaveRename = useCallback(
    async (nameTemplate: string) => {
      const { renameTargetPaths, setUI } = useUIStore.getState();
      const { selectedImage } = useEditorStore.getState();
      const { libraryActivePath, setLibrary } = useLibraryStore.getState();

      if (renameTargetPaths.length > 0 && nameTemplate) {
        try {
          const newPaths: Array<string> = await invoke(Invokes.RenameFiles, {
            nameTemplate,
            paths: renameTargetPaths,
          });

          await refreshImageList();

          if (selectedImage && renameTargetPaths.includes(selectedImage.path)) {
            const oldPathIndex = renameTargetPaths.indexOf(selectedImage.path);
            if (newPaths[oldPathIndex]) {
              handleImageSelect(newPaths[oldPathIndex]);
            } else {
              handleBackToLibrary();
            }
          }

          if (libraryActivePath && renameTargetPaths.includes(libraryActivePath)) {
            const oldPathIndex = renameTargetPaths.indexOf(libraryActivePath);
            if (newPaths[oldPathIndex]) {
              setLibrary({ libraryActivePath: newPaths[oldPathIndex] });
            } else {
              setLibrary({ libraryActivePath: null });
            }
          }

          setLibrary({ multiSelectedPaths: newPaths });
        } catch (err) {
          toast.error(translate('messages.useFileOperations.failed_to_rename_files', { value1: String(err) }));
        }
      }
      setUI({ renameTargetPaths: [] });
    },
    [refreshImageList, handleImageSelect, handleBackToLibrary],
  );

  const handleRenameFiles = useCallback((paths: Array<string>) => {
    if (paths && paths.length > 0) {
      useUIStore.getState().setUI({ renameTargetPaths: paths, isRenameFileModalOpen: true });
    }
  }, []);

  const startImportFiles = useCallback(async (sourcePaths: string[], destinationFolder: string, settings: any) => {
    if (sourcePaths.length === 0 || !destinationFolder) return;

    try {
      await invoke(Invokes.ImportFiles, { destinationFolder, settings, sourcePaths });
    } catch (err) {
      console.error('Failed to start import:', err);
      useProcessStore
        .getState()
        .setImportState({ status: Status.Error, errorMessage: `Failed to start import: ${err}` });
    }
  }, []);

  const handleStartImport = useCallback(
    async (settings: any) => {
      const { importTargetFolder, importSourcePaths } = useUIStore.getState();
      let destination = importTargetFolder;
      if (settings.importMode === 'inPlace') {
        destination = destination || 'Catalog'; // Ignored by the in-place backend.
      } else if (
        !destination ||
        destination.startsWith('Catalog') ||
        destination.startsWith('Album: ') ||
        destination.startsWith('Smart group: ')
      ) {
        const selected = await open({ directory: true, multiple: false, title: 'Choose import destination' });
        if (typeof selected !== 'string') return;
        destination = selected;
      }
      await startImportFiles(importSourcePaths, destination, settings);
    },
    [startImportFiles],
  );

  const handleImportClick = useCallback(async (targetPath: string) => {
    const { supportedTypes } = useSettingsStore.getState();
    const { setUI } = useUIStore.getState();

    try {
      const nonRaw = supportedTypes?.nonRaw || [];
      const raw = supportedTypes?.raw || [];
      const video = supportedTypes?.video || [];

      const expandExtensions = (exts: string[]) => {
        return Array.from(new Set(exts.flatMap((ext) => [ext.toLowerCase(), ext.toUpperCase()])));
      };

      const processedNonRaw = expandExtensions(nonRaw);
      const processedRaw = expandExtensions(raw);
      const processedVideo = expandExtensions(video);
      const allMediaExtensions = [...processedNonRaw, ...processedRaw, ...processedVideo];

      const typeFilters = [
        { name: 'All Supported Media', extensions: allMediaExtensions },
        { name: 'RAW Images', extensions: processedRaw },
        { name: 'Standard Images (JPEG, PNG, etc.)', extensions: processedNonRaw },
        { name: 'Videos', extensions: processedVideo },
        { name: 'All Files', extensions: ['*'] },
      ];

      const selected = await open({
        filters: typeFilters,
        multiple: true,
        title: 'Select files to import',
      });

      if (Array.isArray(selected) && selected.length > 0) {
        const invalidExtensions = new Set<string>();
        const allowedExtensions = new Set(allMediaExtensions.map((e) => e.toLowerCase()));

        const validFiles = selected.filter((path) => {
          const ext = path.split('.').pop()?.toLowerCase() || 'unknown';

          if (!allowedExtensions.has(ext)) {
            invalidExtensions.add(`.${ext}`);
            return false;
          }
          return true;
        });

        if (invalidExtensions.size > 0) {
          const extList = Array.from(invalidExtensions).join(', ');
          toast.error(
            translate('messages.useFileOperations.unsupported_file_format_s_detected', { value1: String(extList) }),
          );
          return;
        }

        setUI({ importSourcePaths: validFiles, importTargetFolder: targetPath, isImportModalOpen: true });
      }
    } catch (err) {
      console.error('Failed to open file dialog for import:', err);
    }
  }, []);

  const handlePasteFiles = useCallback(
    async (mode = 'copy') => {
      const { copiedFilePaths, setProcess } = useProcessStore.getState();
      const { currentFolderPath, setLibrary } = useLibraryStore.getState();

      if (copiedFilePaths.length === 0 || !currentFolderPath) return;

      try {
        if (mode === 'copy') {
          await invoke(Invokes.CopyFiles, { sourcePaths: copiedFilePaths, destinationFolder: currentFolderPath });
        } else {
          await invoke(Invokes.MoveFiles, { sourcePaths: copiedFilePaths, destinationFolder: currentFolderPath });
          setProcess({ copiedFilePaths: [] });
          setLibrary({ multiSelectedPaths: [] });
          await refreshCatalogIndex();
        }
        await refreshImageList();
      } catch (err) {
        toast.error(
          translate('messages.useFileOperations.failed_to_files', { value1: String(mode), value2: String(err) }),
        );
      }
    },
    [refreshImageList, refreshCatalogIndex],
  );

  return {
    executeDelete,
    handleDeleteSelected,
    handleSaveRename,
    handleRenameFiles,
    handleStartImport,
    startImportFiles,
    handleImportClick,
    handlePasteFiles,
  };
}
