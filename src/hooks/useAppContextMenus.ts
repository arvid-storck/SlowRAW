import { t as translate } from 'i18next';
import { normalizeCatalogFilter } from '../utils/catalog';
import { useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Aperture,
  Check,
  ClipboardPaste,
  Copy,
  CopyPlus,
  Edit,
  FileEdit,
  FileInput,
  Folder,
  FolderInput,
  FolderPlus,
  Images,
  LayoutTemplate,
  LayersArrowDown,
  Redo,
  RefreshCw,
  RotateCcw,
  Star,
  SquaresUnite,
  Palette,
  Tag,
  Trash2,
  Undo,
  X,
  Users,
  Gauge,
  Layers,
  Grip,
  Film,
  Home,
  Plane,
  Mountain,
  Sun,
  Camera,
  Map,
  Heart,
  Car,
  Briefcase,
  User,
  Album as AlbumIcon,
  PencilSparkles,
} from 'lucide-react';
import { toast } from 'react-toastify';
import { useTranslation } from 'react-i18next';
import { useContextMenu } from '../context/ContextMenuContext';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useProcessStore } from '../store/useProcessStore';
import { useUIStore } from '../store/useUIStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { Invokes, Option, OPTION_SEPARATOR, Panel, AlbumItem, Album, AlbumGroup } from '../components/ui/AppProperties';
import { Color, COLOR_LABELS, INITIAL_ADJUSTMENTS, normalizeLoadedAdjustments } from '../utils/adjustments';
import TaggingSubMenu from '../context/TaggingSubMenu';
import { useEditorActions } from './useEditorActions';
import { useLibraryActions } from './useLibraryActions';
import { globalImageCache } from '../utils/ImageLRUCache';

export interface UseAppContextMenusProps {
  handleImageSelect: (path: string) => void;
  handleBackToLibrary: () => void;
  handleRenameFiles: (paths: string[]) => void;
  handleImportClick: (path: string) => void;
  handleLibraryRefresh: () => Promise<void>;
  refreshImageList: () => Promise<void>;
  executeDelete: (paths: string[], options: any) => Promise<void>;
}

export function useAppContextMenus(props: UseAppContextMenusProps) {
  const { t } = useTranslation();
  const { showContextMenu } = useContextMenu();

  const {
    handleAutoAdjustments,
    handleAutoLensCorrection,
    handleResetAdjustments,
    handleCopyAdjustments,
    handlePasteAdjustments,
  } = useEditorActions();
  const { handleRate, handleSetColorLabel, handleTagsChanged } = useLibraryActions();

  const albumIcons = useMemo(
    () => [
      { label: t('contextMenus.albumIcons.default'), value: undefined, icon: Folder },
      { label: t('contextMenus.albumIcons.travel'), value: 'plane', icon: Plane },
      { label: t('contextMenus.albumIcons.nature'), value: 'mountain', icon: Mountain },
      { label: t('contextMenus.albumIcons.summer'), value: 'sun', icon: Sun },
      { label: t('contextMenus.albumIcons.photography'), value: 'camera', icon: Camera },
      { label: t('contextMenus.albumIcons.locations'), value: 'map', icon: Map },
      { label: t('contextMenus.albumIcons.favorites'), value: 'heart', icon: Heart },
      { label: t('contextMenus.albumIcons.featured'), value: 'star', icon: Star },
      { label: t('contextMenus.albumIcons.people'), value: 'users', icon: Users },
      { label: t('contextMenus.albumIcons.person'), value: 'user', icon: User },
      { label: t('contextMenus.albumIcons.automotive'), value: 'car', icon: Car },
      { label: t('contextMenus.albumIcons.portfolio'), value: 'briefcase', icon: Briefcase },
    ],
    [t],
  );

  const getCommonTags = useCallback((paths: string[]): { tag: string; isUser: boolean }[] => {
    const { imageList } = useLibraryStore.getState();
    if (paths.length === 0) return [];
    const imageFiles = imageList.filter((img) => paths.includes(img.path));
    if (imageFiles.length === 0) return [];

    const allTagsSets = imageFiles.map((img) => {
      const tagsWithPrefix = (img.tags || []).filter((t: string) => !t.startsWith('color:'));
      return new Set(tagsWithPrefix);
    });

    if (allTagsSets.length === 0) return [];

    const commonTagsWithPrefix = allTagsSets.reduce((intersection, currentSet) => {
      return new Set([...intersection].filter((tag) => currentSet.has(tag)));
    });

    return Array.from(commonTagsWithPrefix)
      .map((tag: string) => ({
        tag: tag.startsWith('user:') ? tag.substring(5) : tag,
        isUser: tag.startsWith('user:'),
      }))
      .sort((a, b) => a.tag.localeCompare(b.tag));
  }, []);

  const buildAddToAlbumMenu = useCallback(
    (items: AlbumItem[], pathsToAdd: string[]): Option[] => {
      return items.flatMap((item) => {
        if (item.type === 'smartGroup') return [];
        const customIconDef = item.icon ? albumIcons.find((i) => i.value === item.icon) : null;
        const ResolvedIcon = customIconDef?.icon || (item.type === 'group' ? Folder : AlbumIcon);

        if (item.type === 'group') {
          return {
            label: item.name,
            icon: ResolvedIcon,
            submenu:
              (item as AlbumGroup).children.length > 0
                ? buildAddToAlbumMenu((item as AlbumGroup).children, pathsToAdd)
                : [{ label: t('contextMenus.album.emptyGroup'), disabled: true }],
          };
        } else {
          return {
            label: item.name,
            icon: ResolvedIcon,
            onClick: () => {
              invoke(Invokes.AddToAlbum, { albumId: item.id, paths: pathsToAdd })
                .then(() => {
                  console.log(`Added image(s) to ${item.name}`);
                  invoke(Invokes.GetAlbums).then((res: any) =>
                    useLibraryStore.getState().setLibrary({ albumTree: res }),
                  );
                })
                .catch((err) => toast.error(t('contextMenus.toasts.failedAddToAlbum', { err })));
            },
          };
        }
      });
    },
    [albumIcons, t],
  );

  const handleEditorContextMenu = useCallback(
    (event: any) => {
      event.preventDefault();
      event.stopPropagation();

      const { selectedImage, history, historyIndex, undo, redo, resetHistory, copiedAdjustments, setEditor } =
        useEditorStore.getState();
      const { appSettings } = useSettingsStore.getState();
      const { setPanel, setUI } = useUIStore.getState();

      if (!selectedImage) return;

      const canUndo = historyIndex > 0;
      const canRedo = historyIndex < history.length - 1;
      const commonTags = getCommonTags([selectedImage.path]);

      const options: Array<Option> = [
        {
          label: t('contextMenus.editor.exportImage'),
          icon: FileInput,
          onClick: () => setPanel(Panel.Export),
        },
        { type: OPTION_SEPARATOR },
        { label: t('contextMenus.editor.undo'), icon: Undo, onClick: undo, disabled: !canUndo },
        { label: t('contextMenus.editor.redo'), icon: Redo, onClick: redo, disabled: !canRedo },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.editor.copyAdjustments'),
          icon: Copy,
          onClick: () => handleCopyAdjustments(),
        },
        {
          label: t('contextMenus.editor.pasteAdjustments'),
          icon: ClipboardPaste,
          onClick: () => handlePasteAdjustments(),
          disabled: copiedAdjustments === null,
        },
        {
          label: t('contextMenus.editor.productivity'),
          icon: Gauge,
          submenu: [
            {
              label: t('contextMenus.editor.autoAdjust'),
              icon: PencilSparkles,
              onClick: handleAutoAdjustments,
              disabled: !selectedImage?.isReady,
            },
            {
              label: t('contextMenus.editor.autoLensCorrection'),
              icon: Aperture,
              onClick: () => handleAutoLensCorrection([selectedImage.path]),
              disabled: !selectedImage?.isReady,
            },
            {
              label: t('contextMenus.editor.denoise'),
              icon: Grip,
              onClick: () => {
                setUI({
                  denoiseModalState: {
                    isOpen: true,
                    isProcessing: false,
                    previewBase64: null,
                    error: null,
                    targetPaths: [selectedImage.path],
                    progressMessage: null,
                  },
                });
              },
            },
            {
              label: t('contextMenus.editor.convertNegative'),
              icon: Film,
              onClick: () => {
                if (selectedImage) {
                  setUI({ negativeModalState: { isOpen: true, targetPaths: [selectedImage.path] } });
                }
              },
            },
            {
              icon: LayoutTemplate,
              label: t('contextMenus.editor.frameImage'),
              onClick: () => {
                setUI({ collageModalState: { isOpen: true, sourceImages: [selectedImage] } });
              },
            },
            { label: t('contextMenus.editor.cullImage'), icon: Users, disabled: true },
          ],
        },
        {
          label: t('contextMenus.merge.title'),
          icon: LayersArrowDown,
          submenu: [
            { disabled: true, icon: SquaresUnite, label: t('contextMenus.editor.stitchPanorama') },
            { disabled: true, icon: Images, label: t('contextMenus.editor.mergeHdr') },
            { disabled: true, icon: Layers, label: t('contextMenus.merge.focusStack') },
          ],
        },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.editor.rating'),
          icon: Star,
          submenu: [0, 1, 2, 3, 4, 5].map((rating: number) => ({
            label:
              rating === 0
                ? t('contextMenus.editor.noRating')
                : t('contextMenus.editor.ratingLabel', { count: rating }),
            onClick: () => handleRate(rating),
          })),
        },
        {
          label: t('contextMenus.editor.colorLabel'),
          icon: Palette,
          submenu: [
            { label: t('contextMenus.editor.noLabel'), onClick: () => handleSetColorLabel(null) },
            ...COLOR_LABELS.map((label: Color) => ({
              label: String(t(`contextMenus.colors.${label.name}` as any)),
              color: label.color,
              onClick: () => handleSetColorLabel(label.name),
            })),
          ],
        },
        {
          label: t('contextMenus.editor.tagging'),
          icon: Tag,
          submenu: [
            {
              customComponent: TaggingSubMenu,
              customProps: {
                paths: [selectedImage.path],
                initialTags: commonTags,
                onTagsChanged: handleTagsChanged,
                appSettings,
              },
            },
          ],
        },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.editor.resetAdjustments'),
          icon: RotateCcw,
          submenu: [
            { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
            {
              label: t('contextMenus.editor.confirmReset'),
              icon: Check,
              isDestructive: true,
              onClick: () => {
                const originalAspectRatio =
                  selectedImage.width && selectedImage.height ? selectedImage.width / selectedImage.height : null;
                resetHistory({
                  ...INITIAL_ADJUSTMENTS,
                  aspectRatio: originalAspectRatio,
                });
                setEditor({ adjustments: { ...INITIAL_ADJUSTMENTS, aspectRatio: originalAspectRatio } });
              },
            },
          ],
        },
      ];
      showContextMenu(event.clientX, event.clientY, options);
    },
    [
      getCommonTags,
      handleCopyAdjustments,
      handlePasteAdjustments,
      handleAutoAdjustments,
      handleRate,
      handleSetColorLabel,
      handleTagsChanged,
      showContextMenu,
      t,
    ],
  );

  const handleThumbnailContextMenu = useCallback(
    (event: any, path: string, forceSingleSelection: boolean = false) => {
      event.preventDefault();
      event.stopPropagation();

      const { selectedImage, copiedAdjustments, setEditor } = useEditorStore.getState();
      const {
        multiSelectedPaths,
        imageList,
        libraryActivePath,
        albumTree,
        activeAlbumId,
        currentFolderPath,
        isCatalogMode,
        setLibrary,
      } = useLibraryStore.getState();
      const { appSettings } = useSettingsStore.getState();
      const { activeView, setUI, setPanel } = useUIStore.getState();
      const { setProcess } = useProcessStore.getState();

      const isTargetInSelection = multiSelectedPaths.includes(path);
      const isSmartGroupView = currentFolderPath?.startsWith('Smart group: ') ?? false;
      let finalSelection: string[];

      if (forceSingleSelection) {
        finalSelection = [path];
      } else if (!isTargetInSelection) {
        finalSelection = [path];
        setLibrary({ multiSelectedPaths: [path] });
        if (!selectedImage) {
          setLibrary({ libraryActivePath: path });
        }
      } else {
        finalSelection = multiSelectedPaths;
      }

      const commonTags = getCommonTags(finalSelection);

      const selectionCount = finalSelection.length;
      const isSingleSelection = selectionCount === 1;
      const isEditingThisImage = activeView === 'editor' && selectedImage?.path === path;
      const deleteLabel = t('contextMenus.thumbnail.deleteImage', { count: selectionCount });
      const exportLabel = t('contextMenus.thumbnail.exportImage', { count: selectionCount });

      const selectionHasVirtualCopies =
        isSingleSelection &&
        !finalSelection[0].includes('?vc=') &&
        imageList.some((image) => image.path.startsWith(`${finalSelection[0]}?vc=`));

      const hasAssociatedFiles = finalSelection.some((selectedPath) => {
        const image = imageList.find((img) => img.path === selectedPath);
        if (image?.group_id != null) return true;

        const getBasePath = (p: string) => {
          const qMark = p.indexOf('?');
          const clean = qMark === -1 ? p : p.substring(0, qMark);
          const dot = clean.lastIndexOf('.');
          return dot === -1 ? clean : clean.substring(0, dot);
        };
        const basePath = getBasePath(selectedPath);

        return imageList.some((img) => img.path !== selectedPath && getBasePath(img.path) === basePath);
      });

      let deleteSubmenu;
      if (selectionHasVirtualCopies) {
        deleteSubmenu = [
          { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
          {
            label: t('contextMenus.thumbnail.confirmDeleteVc'),
            icon: Check,
            isDestructive: true,
            onClick: () => props.executeDelete(finalSelection, { includeAssociated: false }),
          },
        ];
      } else if (hasAssociatedFiles) {
        deleteSubmenu = [
          { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
          {
            label: t('contextMenus.thumbnail.deleteSelected'),
            icon: Check,
            isDestructive: true,
            onClick: () => props.executeDelete(finalSelection, { includeAssociated: false }),
          },
          {
            label: t('contextMenus.thumbnail.deleteAssociated'),
            icon: Check,
            isDestructive: true,
            onClick: () => props.executeDelete(finalSelection, { includeAssociated: true }),
          },
        ];
      } else {
        deleteSubmenu = [
          { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
          {
            label: t('contextMenus.thumbnail.confirmDelete'),
            icon: Check,
            isDestructive: true,
            onClick: () => props.executeDelete(finalSelection, { includeAssociated: false }),
          },
        ];
      }

      const pasteLabel = t('contextMenus.thumbnail.pasteAdjustments', { count: selectionCount });
      const resetLabel = t('contextMenus.thumbnail.resetAdjustments', { count: selectionCount });
      const copyLabel = t('contextMenus.thumbnail.copyImage', { count: selectionCount });
      const autoAdjustLabel = t('contextMenus.thumbnail.autoAdjust', { count: selectionCount });
      const renameLabel = t('contextMenus.thumbnail.renameImage', { count: selectionCount });
      const cullLabel = t('contextMenus.thumbnail.cullImage', { count: selectionCount });
      const collageLabel = t('contextMenus.thumbnail.collage', { count: selectionCount });
      const stitchLabel = t('contextMenus.editor.stitchPanorama');
      const conversionLabel = t('contextMenus.thumbnail.convertNegative', { count: selectionCount });
      const denoiseLabel = t('contextMenus.thumbnail.denoise', { count: selectionCount });
      const mergeLabel = t('contextMenus.editor.mergeHdr');

      const handleCreateVirtualCopy = async (sourcePath: string) => {
        try {
          await invoke(Invokes.CreateVirtualCopy, {
            sourceVirtualPath: sourcePath,
            targetAlbumId: isSmartGroupView ? null : activeAlbumId || null,
          });

          if (activeAlbumId && !isSmartGroupView) {
            const sortedTree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
            setLibrary({ albumTree: sortedTree });
          }
          await props.refreshImageList();
        } catch (err) {
          toast.error(t('contextMenus.toasts.failedCreateVirtualCopy', { err }));
        }
      };

      const handleApplyAutoAdjustmentsToSelection = () => {
        if (finalSelection.length === 0) return;
        finalSelection.forEach((p) => globalImageCache.delete(p));

        invoke(Invokes.ApplyAutoAdjustmentsToPaths, { paths: finalSelection })
          .then(async () => {
            if (selectedImage && finalSelection.includes(selectedImage.path)) {
              const metadata: any = await invoke(Invokes.LoadMetadata, { path: selectedImage.path });
              if (metadata.adjustments && !metadata.adjustments.is_null) {
                const normalized = normalizeLoadedAdjustments(metadata.adjustments);
                setEditor({ adjustments: normalized });
                useEditorStore.getState().resetHistory(normalized);
              }
            }
            if (libraryActivePath && finalSelection.includes(libraryActivePath)) {
              const metadata: any = await invoke(Invokes.LoadMetadata, { path: libraryActivePath });
              if (metadata.adjustments && !metadata.adjustments.is_null) {
                const normalized = normalizeLoadedAdjustments(metadata.adjustments);
                setLibrary({ libraryActiveAdjustments: normalized });
              }
            }
          })
          .catch((err) => {
            console.error('Failed to apply auto adjustments to paths:', err);
            toast.error(t('contextMenus.toasts.failedApplyAuto', { err }));
          });
      };

      const onExportClick = () => {
        setLibrary({ multiSelectedPaths: finalSelection });
        if (activeView === 'editor' && selectedImage && selectedImage.path !== path) {
          props.handleImageSelect(path);
        }
        setPanel(Panel.Export);
      };

      const handleRemoveFromAlbum = async () => {
        if (!activeAlbumId) return;
        const newTree = JSON.parse(JSON.stringify(albumTree));

        const removeImages = (nodes: AlbumItem[]): boolean => {
          for (const n of nodes) {
            if (n.id === activeAlbumId && n.type === 'album') {
              (n as Album).images = (n as Album).images.filter((p) => !finalSelection.includes(p));
              return true;
            } else if (n.type === 'group') {
              if (removeImages(n.children)) return true;
            }
          }
          return false;
        };

        if (removeImages(newTree)) {
          try {
            await invoke(Invokes.SaveAlbums, { tree: newTree });
            const sortedTree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
            setLibrary({ albumTree: sortedTree });

            const albumObj = sortedTree.reduce((acc: any, cur: any) => {
              const find = (n: any): any =>
                n.id === activeAlbumId
                  ? n
                  : n.type === 'group'
                    ? n.children.reduce((a: any, c: any) => a || find(c), null)
                    : null;
              return acc || find(cur);
            }, null) as Album;

            if (albumObj) {
              setLibrary({ imageList: imageList.filter((i) => albumObj.images.includes(i.path)) });
            }
          } catch (e) {
            toast.error(t('contextMenus.toasts.failedRemoveImages', { err: e }));
          }
        }
      };

      const handleRemoveFromCatalog = async () => {
        try {
          const catalogStatus = await invoke(Invokes.CatalogRemoveImages, { paths: finalSelection });
          const catalogFacets = await invoke(Invokes.CatalogFacets);
          setLibrary((state) => ({
            catalogStatus: catalogStatus as any,
            catalogFacets: catalogFacets as any,
            imageList: state.imageList.filter((image) => !finalSelection.includes(image.path)),
            multiSelectedPaths: [],
            libraryActivePath: null,
          }));
          toast.success(
            selectionCount === 1 ? 'Image removed from catalog' : `${selectionCount} images removed from catalog`,
          );
        } catch (error) {
          toast.error(
            translate('messages.useAppContextMenus.could_not_remove_from_catalog', { value1: String(error) }),
          );
        }
      };

      const options = [
        ...(!isEditingThisImage
          ? [
              {
                disabled: !isSingleSelection,
                icon: Edit,
                label: t('contextMenus.editor.editImage'),
                onClick: () => props.handleImageSelect(finalSelection[0]),
              },
              { icon: FileInput, label: exportLabel, onClick: onExportClick },
              { type: OPTION_SEPARATOR },
            ]
          : [{ icon: FileInput, label: exportLabel, onClick: onExportClick }, { type: OPTION_SEPARATOR }]),
        {
          disabled: !isSingleSelection,
          icon: Copy,
          label: t('contextMenus.editor.copyAdjustments'),
          onClick: () => handleCopyAdjustments(finalSelection[0]),
        },
        {
          disabled: copiedAdjustments === null,
          icon: ClipboardPaste,
          label: pasteLabel,
          onClick: () => handlePasteAdjustments(finalSelection),
        },
        {
          label: t('contextMenus.editor.productivity'),
          icon: Gauge,
          submenu: [
            { label: autoAdjustLabel, icon: PencilSparkles, onClick: handleApplyAutoAdjustmentsToSelection },
            {
              label: t('contextMenus.thumbnail.autoLensCorrection', { count: selectionCount }),
              icon: Aperture,
              onClick: () => handleAutoLensCorrection(finalSelection),
            },
            {
              label: denoiseLabel,
              icon: Grip,
              disabled: finalSelection.length === 0,
              onClick: () => {
                setUI({
                  denoiseModalState: {
                    isOpen: true,
                    isProcessing: false,
                    previewBase64: null,
                    error: null,
                    targetPaths: finalSelection,
                    progressMessage: null,
                  },
                });
              },
            },
            {
              label: conversionLabel,
              icon: Film,
              disabled: selectionCount === 0,
              onClick: () => {
                setUI({ negativeModalState: { isOpen: true, targetPaths: finalSelection } });
              },
            },
            {
              icon: LayoutTemplate,
              label: collageLabel,
              onClick: () => {
                const imagesForCollage = imageList.filter((img) => finalSelection.includes(img.path));
                setUI({ collageModalState: { isOpen: true, sourceImages: imagesForCollage } });
              },
              disabled: selectionCount === 0 || selectionCount > 9,
            },
            {
              label: cullLabel,
              icon: Users,
              onClick: () =>
                setUI({
                  cullingModalState: {
                    isOpen: true,
                    progress: null,
                    suggestions: null,
                    error: null,
                    pathsToCull: finalSelection,
                  },
                }),
              disabled: selectionCount < 2,
            },
          ],
        },
        {
          label: t('contextMenus.merge.title'),
          icon: LayersArrowDown,
          submenu: [
            {
              disabled: selectionCount < 2 || selectionCount > 30,
              icon: SquaresUnite,
              label: stitchLabel,
              onClick: () => {
                setUI({
                  panoramaModalState: {
                    error: null,
                    finalImageBase64: null,
                    isOpen: true,
                    isProcessing: false,
                    progressMessage: null,
                    stitchingSourcePaths: finalSelection,
                  },
                });
              },
            },
            {
              disabled: selectionCount < 2 || selectionCount > 9,
              icon: Images,
              label: mergeLabel,
              onClick: () => {
                setUI({
                  hdrModalState: {
                    error: null,
                    finalImageBase64: null,
                    isOpen: true,
                    isProcessing: false,
                    progressMessage: null,
                    stitchingSourcePaths: finalSelection,
                  },
                });
              },
            },
            {
              disabled: selectionCount < 2,
              icon: Layers,
              label: t('contextMenus.merge.focusStack'),
              onClick: () => {
                setUI({
                  focusStackModalState: {
                    error: null,
                    finalImageBase64: null,
                    depthMapBase64: null,
                    isOpen: true,
                    isProcessing: false,
                    progressMessage: null,
                    sourcePaths: finalSelection,
                  },
                });
              },
            },
          ],
        },
        { type: OPTION_SEPARATOR },
        {
          label: copyLabel,
          icon: Copy,
          onClick: () => {
            setProcess({ copiedFilePaths: finalSelection, isCopied: true });
          },
        },
        {
          icon: CopyPlus,
          label: t('contextMenus.thumbnail.duplicateImage'),
          submenu: [
            {
              label: t('contextMenus.thumbnail.physicalCopy'),
              icon: Copy,
              disabled: !isSingleSelection,
              onClick: async () => {
                try {
                  await invoke(Invokes.DuplicateFile, {
                    path: finalSelection[0],
                    targetAlbumId: isSmartGroupView ? null : activeAlbumId || null,
                  });
                  if (activeAlbumId && !isSmartGroupView) {
                    const sortedTree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
                    setLibrary({ albumTree: sortedTree });
                  }
                  await props.refreshImageList();
                } catch (err) {
                  console.error('Failed to duplicate file:', err);
                  toast.error(t('contextMenus.toasts.failedDuplicate', { err }));
                }
              },
            },
            {
              label: t('contextMenus.thumbnail.virtualCopy'),
              icon: CopyPlus,
              disabled: !isSingleSelection,
              onClick: () => handleCreateVirtualCopy(finalSelection[0]),
            },
          ],
        },
        { icon: FileEdit, label: renameLabel, onClick: () => props.handleRenameFiles(finalSelection) },
        { type: OPTION_SEPARATOR },
        {
          icon: Star,
          label: t('contextMenus.editor.rating'),
          submenu: [0, 1, 2, 3, 4, 5].map((rating: number) => ({
            label:
              rating === 0
                ? t('contextMenus.editor.noRating')
                : t('contextMenus.editor.ratingLabel', { count: rating }),
            onClick: () => handleRate(rating, finalSelection),
          })),
        },
        {
          label: t('contextMenus.editor.colorLabel'),
          icon: Palette,
          submenu: [
            { label: t('contextMenus.editor.noLabel'), onClick: () => handleSetColorLabel(null, finalSelection) },
            ...COLOR_LABELS.map((label: Color) => ({
              label: String(t(`contextMenus.colors.${label.name}` as any)),
              color: label.color,
              onClick: () => handleSetColorLabel(label.name, finalSelection),
            })),
          ],
        },
        {
          label: t('contextMenus.editor.tagging'),
          icon: Tag,
          submenu: [
            {
              customComponent: TaggingSubMenu,
              customProps: {
                paths: finalSelection,
                initialTags: commonTags,
                onTagsChanged: handleTagsChanged,
                appSettings,
              },
            },
          ],
        },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.thumbnail.addToAlbum'),
          icon: FolderPlus,
          submenu:
            albumTree.length > 0
              ? buildAddToAlbumMenu(albumTree, finalSelection)
              : [{ label: t('contextMenus.thumbnail.noAlbums'), disabled: true }],
        },
        ...(activeAlbumId && !isSmartGroupView
          ? [
              {
                label: t('contextMenus.thumbnail.removeFromAlbum', { count: selectionCount }),
                icon: Trash2,
                isDestructive: true,
                onClick: handleRemoveFromAlbum,
              },
            ]
          : []),
        ...(isCatalogMode
          ? [
              {
                label: selectionCount === 1 ? 'Remove from catalog' : `Remove ${selectionCount} images from catalog`,
                icon: FolderInput,
                submenu: [
                  { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
                  {
                    label: 'Confirm removal from catalog',
                    icon: Check,
                    isDestructive: true,
                    onClick: handleRemoveFromCatalog,
                  },
                ],
              },
            ]
          : []),
        { type: OPTION_SEPARATOR },
        {
          disabled: !isSingleSelection,
          icon: Folder,
          label: t('contextMenus.thumbnail.showExplorer'),
          onClick: () => {
            invoke(Invokes.ShowInFinder, { path: finalSelection[0] }).catch((err) =>
              toast.error(t('contextMenus.toasts.couldNotShowExplorer', { err })),
            );
          },
        },
        {
          label: resetLabel,
          icon: RotateCcw,
          submenu: [
            { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
            {
              label: t('contextMenus.editor.confirmReset'),
              icon: Check,
              isDestructive: true,
              onClick: () => handleResetAdjustments(finalSelection),
            },
          ],
        },
        {
          label: deleteLabel,
          icon: Trash2,
          isDestructive: true,
          submenu: deleteSubmenu,
        },
      ];
      showContextMenu(event.clientX, event.clientY, options);
    },
    [
      getCommonTags,
      buildAddToAlbumMenu,
      handleCopyAdjustments,
      handlePasteAdjustments,
      handleRate,
      handleSetColorLabel,
      handleTagsChanged,
      handleResetAdjustments,
      showContextMenu,
      props,
      t,
    ],
  );

  const handleAlbumTreeContextMenu = useCallback(
    (event: any, item: AlbumItem | null) => {
      event.preventDefault();
      event.stopPropagation();

      const { setUI } = useUIStore.getState();
      const { albumTree, setLibrary } = useLibraryStore.getState();

      const findParentId = (
        nodes: AlbumItem[],
        childId: string,
        parentId: string | null = null,
      ): string | null | undefined => {
        for (const n of nodes) {
          if (n.id === childId) return parentId;
          if (n.type === 'group') {
            const found = findParentId((n as AlbumGroup).children, childId, n.id);
            if (found !== undefined) return found;
          }
        }
        return undefined;
      };

      const currentParentId = item ? findParentId(albumTree, item.id) : undefined;

      const handleMove = (targetId: string | null) => {
        if (!item) return;
        const newTree = structuredClone(albumTree);
        let extractedItem: AlbumItem | null = null;

        const removeAndGet = (nodes: AlbumItem[], id: string): AlbumItem | null => {
          for (let i = 0; i < nodes.length; i++) {
            if (nodes[i].id === id) return nodes.splice(i, 1)[0];
            if (nodes[i].type === 'group') {
              const res = removeAndGet((nodes[i] as AlbumGroup).children, id);
              if (res) return res;
            }
          }
          return null;
        };

        extractedItem = removeAndGet(newTree, item.id);
        if (!extractedItem) return;

        if (!targetId) {
          newTree.push(extractedItem);
        } else {
          let inserted = false;

          const insert = (nodes: AlbumItem[]) => {
            for (const n of nodes) {
              if (n.id === targetId && n.type === 'group') {
                n.children.push(extractedItem!);
                inserted = true;
                return;
              } else if (n.type === 'group') {
                insert(n.children);
                if (inserted) return;
              }
            }
          };

          insert(newTree);

          if (!inserted) {
            toast.error(t('contextMenus.toasts.failedMoveInvalid'));
            return;
          }
        }

        invoke(Invokes.SaveAlbums, { tree: newTree })
          .then(() => invoke(Invokes.GetAlbums))
          .then((sortedTree: any) => setLibrary({ albumTree: sortedTree }))
          .catch((err) => toast.error(t('contextMenus.toasts.failedMoveError', { err })));
      };

      const buildMoveSubmenu = (nodes: AlbumItem[]): Option[] => {
        const opts: Option[] = [];
        nodes.forEach((n) => {
          if (n.type === 'group' && n.id !== item?.id) {
            const isCurrentParent = n.id === currentParentId;
            const subOpts = buildMoveSubmenu((n as AlbumGroup).children);

            const customIconDef = n.icon ? albumIcons.find((i) => i.value === n.icon) : null;
            const ResolvedIcon = customIconDef?.icon || Folder;

            if (subOpts.length > 0) {
              opts.push({
                label: n.name,
                icon: ResolvedIcon,
                submenu: [
                  {
                    label: isCurrentParent ? t('contextMenus.albums.alreadyHere') : t('contextMenus.albums.moveHere'),
                    icon: Check,
                    disabled: isCurrentParent,
                    onClick: isCurrentParent ? undefined : () => handleMove(n.id),
                  },
                  { type: OPTION_SEPARATOR },
                  ...subOpts,
                ],
              });
            } else {
              opts.push({
                label: isCurrentParent ? `${n.name} (Current)` : n.name,
                icon: ResolvedIcon,
                disabled: isCurrentParent,
                onClick: isCurrentParent ? undefined : () => handleMove(n.id),
              });
            }
          }
        });
        return opts;
      };

      const moveOptions = buildMoveSubmenu(albumTree);
      const isAtRoot = currentParentId === null;
      const isMoveDisabled = moveOptions.length === 0 && isAtRoot;

      const options: Option[] = [
        {
          label: t('contextMenus.albums.newAlbum'),
          icon: Images,
          onClick: () => setUI({ albumActionTarget: item?.id || null, isCreateAlbumModalOpen: true }),
        },
        {
          label: t('contextMenus.albums.newGroup'),
          icon: FolderPlus,
          onClick: () => setUI({ albumActionTarget: item?.id || null, isCreateAlbumGroupModalOpen: true }),
        },
        ...(item?.type === 'smartGroup'
          ? [
              {
                label: t('catalog.editFilters'),
                icon: FileEdit,
                onClick: () => {
                  setLibrary({
                    editingSmartGroupId: item.id,
                    activeAlbumId: item.id,
                    isCatalogMode: true,
                    catalogFilter: normalizeCatalogFilter(item.filter),
                    catalogDateFilter: item.filter.date ?? '',
                    catalogPage: 0,
                  });
                  setUI((state) => ({
                    activeView: 'library',
                    uiVisibility: { ...state.uiVisibility, rightPanel: true },
                  }));
                },
              },
            ]
          : []),
        ...(item
          ? [
              { type: OPTION_SEPARATOR },
              {
                label:
                  item.type === 'group' ? t('contextMenus.albums.renameGroup') : t('contextMenus.albums.renameAlbum'),
                icon: FileEdit,
                onClick: () => setUI({ albumActionTarget: item.id, isRenameAlbumModalOpen: true }),
              },
              {
                label: t('contextMenus.folders.changeIcon'),
                icon: Palette,
                submenu: albumIcons.map((iconDef) => ({
                  label: iconDef.label,
                  icon: iconDef.icon,
                  onClick: () => {
                    const newTree = structuredClone(albumTree);
                    const updateIcon = (nodes: AlbumItem[]) => {
                      for (const n of nodes) {
                        if (n.id === item.id) {
                          n.icon = iconDef.value;
                          return true;
                        }
                        if (n.type === 'group' && updateIcon((n as AlbumGroup).children)) return true;
                      }
                      return false;
                    };

                    if (updateIcon(newTree)) {
                      invoke(Invokes.SaveAlbums, { tree: newTree })
                        .then(() => invoke(Invokes.GetAlbums))
                        .then((sorted: any) => setLibrary({ albumTree: sorted }))
                        .catch((err) => toast.error(t('contextMenus.toasts.failedChangeIcon', { err })));
                    }
                  },
                })),
              },
              {
                label: t('contextMenus.albums.moveTo'),
                icon: FolderInput,
                disabled: isMoveDisabled,
                submenu: isMoveDisabled
                  ? []
                  : [
                      {
                        label: isAtRoot ? t('contextMenus.albums.alreadyAtRoot') : t('contextMenus.albums.rootDir'),
                        icon: Home,
                        disabled: isAtRoot,
                        onClick: isAtRoot ? undefined : () => handleMove(null),
                      },
                      ...(moveOptions.length > 0 ? [{ type: OPTION_SEPARATOR }, ...moveOptions] : []),
                    ],
              },
              { type: OPTION_SEPARATOR },
              {
                label:
                  item.type === 'group' ? t('contextMenus.albums.deleteGroup') : t('contextMenus.albums.deleteAlbum'),
                icon: Trash2,
                isDestructive: true,
                submenu: [
                  { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
                  {
                    label:
                      item.type === 'album' || item.type === 'smartGroup'
                        ? t('contextMenus.albums.confirmDeleteAlbum')
                        : (item as AlbumGroup).children.length > 0
                          ? t('contextMenus.albums.confirmDeleteGroupNested')
                          : t('contextMenus.albums.confirmDeleteGroupEmpty'),
                    icon: Check,
                    isDestructive: true,
                    onClick: () => {
                      const newTree = structuredClone(albumTree);
                      const del = (nodes: AlbumItem[]) => {
                        const idx = nodes.findIndex((n) => n.id === item.id);
                        if (idx !== -1) nodes.splice(idx, 1);
                        else
                          nodes.forEach((n) => {
                            if (n.type === 'group') del((n as AlbumGroup).children);
                          });
                      };
                      del(newTree);
                      invoke(Invokes.SaveAlbums, { tree: newTree })
                        .then(() => invoke(Invokes.GetAlbums))
                        .then((sorted: any) => setLibrary({ albumTree: sorted }))
                        .catch((err) => toast.error(t('contextMenus.toasts.failedDelete', { err })));
                    },
                  },
                ],
              },
            ]
          : []),
      ];

      showContextMenu(event.clientX, event.clientY, options);
    },
    [showContextMenu, albumIcons, t],
  );

  const handleMainLibraryContextMenu = useCallback(
    (event: any) => {
      event.preventDefault();
      event.stopPropagation();

      const { copiedFilePaths } = useProcessStore.getState();
      const { currentFolderPath, activeAlbumId, isCatalogMode, setLibrary } = useLibraryStore.getState();
      const isAlbumView = Boolean(activeAlbumId) && !isCatalogMode;
      const pasteOption = {
        label: t('contextMenus.library.addCopiedToAlbum', { count: copiedFilePaths.length }),
        icon: ClipboardPaste,
        disabled: copiedFilePaths.length === 0,
        onClick: async () => {
          try {
            await invoke(Invokes.AddToAlbum, { albumId: activeAlbumId, paths: copiedFilePaths });
            setLibrary({ albumTree: await invoke<AlbumItem[]>(Invokes.GetAlbums) });
            await props.refreshImageList();
          } catch (err) {
            toast.error(t('contextMenus.toasts.failedAddToAlbum', { err }));
          }
        },
      };

      const options = [
        { label: t('contextMenus.library.refreshView'), icon: RefreshCw, onClick: props.handleLibraryRefresh },
        { type: OPTION_SEPARATOR },
        ...(isAlbumView ? [pasteOption] : []),
        {
          icon: FolderInput,
          label: t('contextMenus.folders.importImages'),
          onClick: () => props.handleImportClick(currentFolderPath as string),
          disabled: !currentFolderPath || isAlbumView,
        },
      ];

      showContextMenu(event.clientX, event.clientY, options);
    },
    [props, showContextMenu, t],
  );

  return {
    handleEditorContextMenu,
    handleThumbnailContextMenu,
    handleAlbumTreeContextMenu,
    handleMainLibraryContextMenu,
  };
}
