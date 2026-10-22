import { t as translate } from 'i18next';
import { useCatalog } from './hooks/useCatalog';
import { refreshCatalog, showAllCatalog } from './utils/catalog';
import { type PointerEvent as ReactPointerEvent, useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ToastContainer, toast, Slide } from 'react-toastify';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  Modifier,
  MeasuringStrategy,
  pointerWithin,
} from '@dnd-kit/core';
import clsx from 'clsx';

import TitleBar from './window/TitleBar';
import CatalogPanel from './components/panel/library/CatalogPanel';
import SettingsPanel from './components/panel/SettingsPanel';
import ExportPanel from './components/panel/right/ExportPanel';
import GlobalTooltip from './components/ui/GlobalTooltip';
import AppModals from './components/modals/AppModals';

import SidePanelArea from './components/panel/SidePanelArea';
import { PANEL_ICONS } from './components/panel/PanelSwitcher';
import Controls from './components/panel/right/ControlsPanel';
import MetadataPanel from './components/panel/right/MetadataPanel';
import CropPanel from './components/panel/right/CropPanel';
import MasksPanel from './components/panel/right/MasksPanel';

import EditorView from './components/views/EditorView';
import LibraryView from './components/views/LibraryView';

import { ContextMenuProvider } from './context/ContextMenuContext';
import { useSettingsStore } from './store/useSettingsStore';
import { DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_PANEL_WIDTH, useUIStore } from './store/useUIStore';
import { useLibraryStore } from './store/useLibraryStore';
import { useEditorStore } from './store/useEditorStore';
import { useProcessStore } from './store/useProcessStore';
import { useShallow } from 'zustand/react/shallow';

import { useThumbnails } from './hooks/useThumbnails';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { useTauriListeners } from './hooks/useTauriListeners';
import { useFileOperations } from './hooks/useFileOperations';
import { useAppContextMenus } from './hooks/useAppContextMenus';
import { useSortedLibrary } from './hooks/useSortedLibrary';
import { useAppNavigation } from './hooks/useAppNavigation';
import { useExternalEditSession } from './hooks/useExternalEditSession';
import ExternalEditBar from './components/ui/ExternalEditBar';
import { Status } from './components/ui/ExportImportProperties';

import { useEditorActions } from './hooks/useEditorActions';
import { useLibraryActions } from './hooks/useLibraryActions';
import { useProductivityActions } from './hooks/useProductivityActions';

import { useAppInitialization } from './hooks/useAppInitialization';
import './i18n';

import {
  Invokes,
  ImageFile,
  LibraryViewMode,
  Panel,
  PanelRegion,
  Theme,
  ThumbnailSize,
  ThumbnailAspectRatio,
} from './components/ui/AppProperties';

import ImageProcessingManager from './components/managers/ImageProcessingManager';
import ImageLoaderManager from './components/managers/ImageLoaderManager';

const imageDragModifier: Modifier = ({ active, activatorEvent, activeNodeRect, transform }) => {
  if (active?.data?.current?.type === 'library-image' && activatorEvent && activeNodeRect) {
    const event = activatorEvent as any;
    const startX = event.clientX ?? event.touches?.[0]?.clientX ?? 0;
    const startY = event.clientY ?? event.touches?.[0]?.clientY ?? 0;

    if (startX === 0 && startY === 0) return transform;

    const offsetX = startX - activeNodeRect.left - 48;
    const offsetY = startY - activeNodeRect.top - 48;

    return {
      ...transform,
      x: transform.x + offsetX,
      y: transform.y + offsetY,
    };
  }
  return transform;
};

function ImageDragOverlayNode({ activeItem }: { activeItem: { path: string; paths: string[] } }) {
  const url = useProcessStore.getState().thumbnails[activeItem.path];
  const count = activeItem.paths.length;

  return (
    <div className="w-24 h-24 rounded-lg shadow-2xl border-2 border-accent relative bg-surface overflow-hidden flex items-center justify-center">
      {url && <img src={url} className="w-full h-full object-cover" />}
      {count > 1 && (
        <div className="absolute top-1 right-1 bg-accent text-button-text text-xs font-bold px-2 py-0.5 rounded-full shadow-md z-10">
          {count}
        </div>
      )}
    </div>
  );
}

function App() {
  useCatalog();
  const [activeImageDragItem, setActiveImageDragItem] = useState<{ path: string; paths: string[] } | null>(null);

  const { appSettings, theme, osPlatform, handleSettingsChange } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      theme: state.theme,
      osPlatform: state.osPlatform,
      handleSettingsChange: state.handleSettingsChange,
    })),
  );

  const {
    activeView,
    isFullScreen,
    isWindowFullScreen,
    isInstantTransition,
    isLayoutReady,
    uiVisibility,
    leftPanelWidth,
    rightPanelWidth,
    activePanel,
    activeLayoutDragItem,
    isSettingsOpen,
    isLibraryExportPanelVisible,
    setUI,
    setLayoutDragItem,
    movePanel,
  } = useUIStore(
    useShallow((state) => ({
      activeView: state.activeView,
      isFullScreen: state.isFullScreen,
      isWindowFullScreen: state.isWindowFullScreen,
      isInstantTransition: state.isInstantTransition,
      isLayoutReady: state.isLayoutReady,
      uiVisibility: state.uiVisibility,
      leftPanelWidth: state.leftPanelWidth,
      rightPanelWidth: state.rightPanelWidth,
      activePanel: state.activePanel,
      activeLayoutDragItem: state.activeLayoutDragItem,
      isSettingsOpen: state.isSettingsOpen,
      isLibraryExportPanelVisible: state.isLibraryExportPanelVisible,
      setUI: state.setUI,
      setLayoutDragItem: state.setLayoutDragItem,
      movePanel: state.movePanel,
    })),
  );

  const { rootPaths, currentFolderPath, multiSelectedPaths, isCatalogMode } = useLibraryStore(
    useShallow((state) => ({
      rootPaths: state.rootPaths,
      currentFolderPath: state.currentFolderPath,
      multiSelectedPaths: state.multiSelectedPaths,
      isCatalogMode: state.isCatalogMode,
      setLibrary: state.setLibrary,
    })),
  );

  const { selectedImage, activeMaskContainerId, hasRenderedFirstFrame, setEditor } = useEditorStore(
    useShallow((state) => ({
      selectedImage: state.selectedImage,
      activeMaskContainerId: state.activeMaskContainerId,
      hasRenderedFirstFrame: state.hasRenderedFirstFrame,
      setEditor: state.setEditor,
    })),
  );

  const { exportState, setExportState } = useProcessStore(
    useShallow((state) => ({
      exportState: state.exportState,
      setExportState: state.setExportState,
    })),
  );

  const defaultThumbnailSize = ThumbnailSize.Medium;
  const defaultLibraryViewMode = LibraryViewMode.Flat;

  const selectedImagePathRef = useRef<string | null>(null);
  useEffect(() => {
    selectedImagePathRef.current = selectedImage?.path ?? null;
  }, [selectedImage?.path]);

  const prevAdjustmentsRef = useRef<any>(null);

  const isBackendReadyRef = useRef(true);
  const previewJobIdRef = useRef<number>(0);
  const latestRenderedJobIdRef = useRef<number>(0);
  const currentResRef = useRef<number>(1280);
  const cachedEditStateRef = useRef<any | null>(null);

  const [libraryViewMode, setLibraryViewMode] = useState<LibraryViewMode>(defaultLibraryViewMode);
  const [isResizing, setIsResizing] = useState(false);
  const [thumbnailSize, setThumbnailSize] = useState(defaultThumbnailSize);
  const [thumbnailAspectRatio, setThumbnailAspectRatio] = useState(ThumbnailAspectRatio.Contain);

  const { requestThumbnails, clearThumbnailQueue, markGenerated } = useThumbnails();

  const transformWrapperRef = useRef<any>(null);
  const preloadedDataRef = useRef<{
    trees?: Promise<any>;
    images?: Promise<ImageFile[]>;
    rootPaths?: string[];
    currentPath?: string;
  }>({});

  useAppInitialization({
    preloadedDataRef,
    thumbnailSize,
    setThumbnailSize,
    thumbnailAspectRatio,
    setThumbnailAspectRatio,
    libraryViewMode,
    setLibraryViewMode,
  });

  const { handleCopyAdjustments, handlePasteAdjustments, handleResetAdjustments, handleZoomChange } =
    useEditorActions();

  const navigationRefs = {
    transformWrapperRef,
    preloadedDataRef,
    cachedEditStateRef,
    selectedImagePathRef,
    isBackendReadyRef,
    latestRenderedJobIdRef,
    previewJobIdRef,
    currentResRef,
    prevAdjustmentsRef,
  };

  const { handleGoHome, handleBackToLibrary, handleImageSelect, handleSelectAlbum, handleOpenFolder } =
    useAppNavigation({
      clearThumbnailQueue,
      refs: navigationRefs,
    });

  const {
    externalEditSession,
    isFinishing: isExternalEditFinishing,
    finishExternalEdit,
  } = useExternalEditSession(handleImageSelect);

  const {
    handleRate,
    handleClearSelection,
    handleLibraryImageSingleClick,
    handleImageClick,
    handleSetColorLabel,
    refreshCatalogIndex,
    handleCreateAlbumItem,
    handleRenameAlbumItem,
  } = useLibraryActions(handleImageSelect);

  const { displayList: sortedImageList, badges: groupBadgeInfo } = useSortedLibrary();

  const handleLibraryRefresh = useCallback(async () => {
    if (useLibraryStore.getState().isCatalogMode) {
      await refreshCatalog();
      return;
    }
    if (currentFolderPath) {
      if (currentFolderPath.startsWith('Album: ') || currentFolderPath.startsWith('Smart group: ')) {
        const { activeAlbumId, albumTree } = useLibraryStore.getState();
        if (activeAlbumId) {
          const findObj = (nodes: any[]): any => {
            for (const n of nodes) {
              if (n.id === activeAlbumId) return n;
              if (n.type === 'group') {
                const f = findObj(n.children);
                if (f) return f;
              }
            }
            return null;
          };
          const album = findObj(albumTree);
          if (album) await handleSelectAlbum(album.id, album.name, album.images || [], true, album.filter);
        }
      } else {
        showAllCatalog();
        await refreshCatalog();
      }
    }
  }, [currentFolderPath, handleSelectAlbum]);

  const {
    executeDelete,
    handleDeleteSelected,
    handleSaveRename,
    handleRenameFiles,
    handleStartImport,
    handleImportClick,
    handlePasteFiles,
  } = useFileOperations(
    handleLibraryRefresh,
    refreshCatalogIndex,
    handleImageSelect,
    handleBackToLibrary,
    sortedImageList,
  );

  const {
    handleStartPanorama,
    handleSavePanorama,
    handleStartHdr,
    handleSaveHdr,
    handleApplyDenoise,
    handleBatchDenoise,
    handleSaveDenoisedImage,
    handleSaveCollage,
    handleStartFocusStack,
    handleSaveFocusStack,
  } = useProductivityActions(handleLibraryRefresh);

  const {
    handleEditorContextMenu,
    handleThumbnailContextMenu,
    handleAlbumTreeContextMenu,
    handleMainLibraryContextMenu,
  } = useAppContextMenus({
    handleImageSelect,
    handleBackToLibrary,
    handleLibraryRefresh,
    handleRenameFiles,
    handleImportClick,
    refreshImageList: handleLibraryRefresh,
    executeDelete,
  });

  useTauriListeners({
    refreshImageList: handleLibraryRefresh,
    markGenerated,
  });

  const handleToggleFullScreen = useCallback(() => {
    const { zoom, selectedImage } = useEditorStore.getState();
    const currentlyZoomed = zoom > 1.01;
    setUI({ isInstantTransition: currentlyZoomed });

    if (isFullScreen) {
      setUI({ isFullScreen: false });
    } else {
      if (!selectedImage) return;
      setUI({ isFullScreen: true });
    }

    if (currentlyZoomed) {
      setTimeout(() => setUI({ isInstantTransition: false }), 100);
    }
  }, [isFullScreen, setUI]);

  useKeyboardShortcuts({
    sortedImageList,
    handleBackToLibrary,
    handleDeleteSelected,
    handleGoHome,
    handleImageSelect,
    handlePasteFiles,
    handleToggleFullScreen,
    handleZoomChange,
  });

  useEffect(() => {
    const handleGlobalContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };
    window.addEventListener('contextmenu', handleGlobalContextMenu);
    return () => window.removeEventListener('contextmenu', handleGlobalContextMenu);
  }, []);

  const isLightTheme = useMemo(() => [Theme.Light, Theme.Snow, Theme.Arctic].includes(theme as Theme), [theme]);

  useEffect(() => {
    if (activePanel !== Panel.Masks || !activeMaskContainerId) {
      setEditor({ isMaskControlHovered: false });
    }
  }, [activePanel, activeMaskContainerId, setEditor]);

  useEffect(() => {
    setEditor({
      isWbPickerActive: false,
      isStraightenActive: false,
      isGuidedPerspectiveActive: false,
      activeMaskId: null,
    });
  }, [activePanel, activeView, setEditor]);

  const createResizeHandler = (stateKey: string, startSize: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    setIsResizing(true);

    const pointerId = e.pointerId;
    const target = e.currentTarget;
    const startX = e.clientX;
    const startY = e.clientY;

    const previousTouchAction = document.documentElement.style.touchAction;
    const previousUserSelect = document.documentElement.style.userSelect;

    target.setPointerCapture?.(pointerId);
    document.documentElement.style.touchAction = 'none';
    document.documentElement.style.userSelect = 'none';

    const doDrag = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      moveEvent.preventDefault();

      if (stateKey === 'left') {
        let w = startSize + (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, leftPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            leftPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, leftPanel: true },
          }));
        }
      } else if (stateKey === 'right') {
        let w = startSize - (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, rightPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            rightPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, rightPanel: true },
          }));
        }
      } else if (stateKey === 'bottom') {
        const newHeight = startSize - (moveEvent.clientY - startY);
        if (newHeight < 100) {
          setUI((state) => ({
            uiVisibility: { ...state.uiVisibility, filmstrip: false },
          }));
        } else {
          setUI((state) => ({
            bottomPanelHeight: Math.round(Math.min(newHeight, 400)),
            uiVisibility: { ...state.uiVisibility, filmstrip: true },
          }));
        }
      }
    };

    const stopDrag = (upEvent: PointerEvent) => {
      if (upEvent.pointerId !== pointerId) return;
      if (target.hasPointerCapture?.(pointerId)) target.releasePointerCapture(pointerId);

      document.documentElement.style.cursor = '';
      document.documentElement.style.touchAction = previousTouchAction;
      document.documentElement.style.userSelect = previousUserSelect;

      window.removeEventListener('pointermove', doDrag);
      window.removeEventListener('pointerup', stopDrag);
      window.removeEventListener('pointercancel', stopDrag);
      setIsResizing(false);
    };

    document.documentElement.style.cursor = stateKey === 'bottom' ? 'row-resize' : 'col-resize';

    window.addEventListener('pointermove', doDrag, { passive: false });
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
  };

  const createResizeResetHandler = (stateKey: string) => () => {
    if (stateKey === 'left') {
      setUI((state) => ({
        leftPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, leftPanel: true },
      }));
    } else if (stateKey === 'right') {
      setUI((state) => ({
        rightPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, rightPanel: true },
      }));
    } else if (stateKey === 'bottom') {
      setUI((state) => ({
        bottomPanelHeight: DEFAULT_BOTTOM_PANEL_HEIGHT,
        uiVisibility: { ...state.uiVisibility, filmstrip: true },
      }));
    }
  };

  useEffect(() => {
    const appWindow = getCurrentWindow();
    const checkFullscreen = async () => {
      setUI({ isWindowFullScreen: await appWindow.isFullscreen() });
    };
    checkFullscreen();
    const unlistenPromise = appWindow.onResized(checkFullscreen);
    return () => {
      unlistenPromise.then((unlisten: any) => unlisten());
    };
  }, [setUI]);

  const renderAppPanel = useCallback(
    (panelId: Panel) => {
      switch (panelId) {
        case Panel.Catalog:
          return (
            <CatalogPanel
              isResizing={isResizing}
              onAlbumContextMenu={handleAlbumTreeContextMenu}
              onSelectAlbum={handleSelectAlbum}
              style={{ width: '100%', height: '100%' }}
            />
          );
        case Panel.Export:
          return (
            <ExportPanel
              exportState={exportState}
              multiSelectedPaths={multiSelectedPaths}
              selectedImage={selectedImage}
              setExportState={setExportState}
              appSettings={appSettings}
              onSettingsChange={handleSettingsChange}
              rootPaths={rootPaths}
              isVisible={true}
              onClose={() => setUI({ isLibraryExportPanelVisible: false })}
            />
          );
        case Panel.Adjustments:
          return <Controls />;
        case Panel.Metadata:
          return <MetadataPanel />;
        case Panel.Crop:
          return <CropPanel />;
        case Panel.Masks:
          return <MasksPanel />;
        default:
          return null;
      }
    },
    [
      isResizing,
      handleAlbumTreeContextMenu,
      handleSelectAlbum,
      handleOpenFolder,
      setUI,
      isInstantTransition,
      exportState,
      multiSelectedPaths,
      selectedImage,
      setExportState,
      appSettings,
      handleSettingsChange,
      rootPaths,
    ],
  );

  const hasRoots = rootPaths && rootPaths.length > 0;
  const hasMainContent =
    hasRoots || !!currentFolderPath || isCatalogMode || (activeView === 'editor' && !!selectedImage);

  const isWgpuActive = activeView === 'editor' && selectedImage?.isReady && hasRenderedFirstFrame;
  const useMacWindowShell = osPlatform === 'macos' && !appSettings?.decorations && !isWindowFullScreen && !isFullScreen;

  const layoutSensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));
  const handleDragStart = (e: any) => {
    if (e.active.data.current?.type === 'layout-tab') {
      setLayoutDragItem(e.active.data.current.panel as Panel);
    } else if (e.active.data.current?.type === 'library-image') {
      const path = e.active.data.current.path;
      const multiSelected = useLibraryStore.getState().multiSelectedPaths;
      const paths = multiSelected.includes(path) ? multiSelected : [path];
      setActiveImageDragItem({ path, paths });
    }
  };

  const handleDragEnd = (e: any) => {
    setLayoutDragItem(null);
    setActiveImageDragItem(null);
    const { active, over } = e;

    if (active.data.current?.type === 'layout-tab' && over?.data.current?.type === 'layout-region') {
      movePanel(active.data.current.panel as Panel, over.data.current.region as PanelRegion);
    }

    if (active.data.current?.type === 'library-image' && over?.data.current?.type === 'album') {
      const targetAlbumId = over.data.current.id;
      const sourcePaths = activeImageDragItem?.paths || [active.data.current.path];

      invoke(Invokes.AddToAlbum, { albumId: targetAlbumId, paths: sourcePaths })
        .then(() => invoke(Invokes.GetAlbums))
        .then((updatedTree: any) => {
          useLibraryStore.getState().setLibrary({ albumTree: updatedTree, multiSelectedPaths: [] });
          handleLibraryRefresh();
        })
        .catch((err) => {
          toast.error(translate('messages.App.failed_to_add_to_album', { value1: String(err) }));
        });
    }
  };

  const ActiveOverlayIcon = activeLayoutDragItem ? PANEL_ICONS[activeLayoutDragItem] : null;
  const effectiveLeftWidth = uiVisibility.leftPanel ? leftPanelWidth : 48;
  const effectiveRightWidth = uiVisibility.rightPanel ? rightPanelWidth : 48;

  return (
    <>
      <ImageProcessingManager
        transformWrapperRef={transformWrapperRef}
        prevAdjustmentsRef={prevAdjustmentsRef}
        previewJobIdRef={previewJobIdRef}
        latestRenderedJobIdRef={latestRenderedJobIdRef}
        currentResRef={currentResRef}
      />
      <ImageLoaderManager cachedEditStateRef={cachedEditStateRef} />
      <div
        className={clsx(
          'flex flex-col h-screen font-sans text-text-primary overflow-hidden select-none',
          useMacWindowShell && 'macos-window-shell',
          isWgpuActive ? 'bg-transparent' : 'bg-bg-primary',
        )}
      >
        <div
          className={clsx(
            'shrink-0 overflow-hidden z-50',
            !isInstantTransition && 'transition-all duration-300 ease-in-out',
            isFullScreen ? 'max-h-0 opacity-0 pointer-events-none' : 'max-h-15 opacity-100',
          )}
        >
          {appSettings?.decorations || (!isWindowFullScreen && <TitleBar />)}
        </div>
        <div
          className={clsx(
            'flex-1 flex flex-col min-h-0',
            isLayoutReady && hasMainContent && !isInstantTransition && 'transition-all duration-300 ease-in-out',
            [hasMainContent && (isFullScreen ? 'p-0 gap-0' : 'p-2 gap-2')],
          )}
        >
          <DndContext
            sensors={layoutSensors}
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
            collisionDetection={pointerWithin}
            measuring={{
              droppable: {
                strategy: MeasuringStrategy.Always,
              },
            }}
          >
            <div className="flex flex-row grow h-full min-h-0">
              {activeView !== 'editor' && uiVisibility.leftPanel && (
                <div className="w-64 shrink-0 min-h-0 overflow-hidden rounded-lg bg-bg-secondary">
                  {renderAppPanel(Panel.Catalog)}
                </div>
              )}
              {activeView === 'editor' && uiVisibility.leftPanel && hasMainContent && (
                <SidePanelArea
                  side="left"
                  width={effectiveLeftWidth}
                  topRegion="leftTop"
                  bottomRegion="leftBottom"
                  renderPanel={renderAppPanel}
                  onWidthChange={createResizeHandler('left', effectiveLeftWidth)}
                  onWidthReset={createResizeResetHandler('left')}
                  isResizing={isResizing}
                />
              )}
              <div className="relative flex-1 flex flex-col min-w-0 min-h-0">
                {selectedImage && externalEditSession && (
                  <ExternalEditBar
                    session={externalEditSession}
                    isFinishing={isExternalEditFinishing}
                    errorMessage={exportState.status === Status.Error ? exportState.errorMessage : ''}
                    onDone={finishExternalEdit}
                  />
                )}
                <div
                  className={clsx(
                    'flex-1 flex flex-col min-w-0 min-h-0 h-full',
                    activeView === 'editor' && selectedImage ? 'flex' : 'hidden',
                  )}
                >
                  {selectedImage && (
                    <EditorView
                      transformWrapperRef={transformWrapperRef}
                      isResizing={isResizing}
                      thumbnailAspectRatio={thumbnailAspectRatio}
                      sortedImageList={sortedImageList}
                      createResizeHandler={createResizeHandler}
                      createResizeResetHandler={createResizeResetHandler}
                      handleBackToLibrary={handleBackToLibrary}
                      handleEditorContextMenu={handleEditorContextMenu}
                      handleThumbnailContextMenu={handleThumbnailContextMenu}
                      handleMainLibraryContextMenu={handleMainLibraryContextMenu}
                      handleImageClick={handleImageClick}
                      handleClearSelection={handleClearSelection}
                      handleCopyAdjustments={handleCopyAdjustments}
                      handlePasteAdjustments={handlePasteAdjustments}
                      handleRate={handleRate}
                      handleZoomChange={handleZoomChange}
                      requestThumbnails={requestThumbnails}
                    />
                  )}
                </div>
                <div
                  className={clsx(
                    'flex-1 flex flex-col min-w-0 min-h-0 h-full',
                    activeView === 'editor' && selectedImage ? 'hidden' : 'flex',
                  )}
                >
                  <LibraryView
                    sortedImageList={sortedImageList}
                    groupBadgeInfo={groupBadgeInfo}
                    thumbnailSize={thumbnailSize}
                    thumbnailAspectRatio={thumbnailAspectRatio}
                    setThumbnailSize={setThumbnailSize}
                    setThumbnailAspectRatio={setThumbnailAspectRatio}
                    handleClearSelection={handleClearSelection}
                    handleLibraryImageSingleClick={handleLibraryImageSingleClick}
                    handleImageSelect={handleImageSelect}
                    handleRate={handleRate}
                    handleThumbnailContextMenu={handleThumbnailContextMenu}
                    handleMainLibraryContextMenu={handleMainLibraryContextMenu}
                    handleOpenFolder={handleOpenFolder}
                    handleImportClick={handleImportClick}
                    handleCopyAdjustments={handleCopyAdjustments}
                    handlePasteAdjustments={handlePasteAdjustments}
                    handleResetAdjustments={handleResetAdjustments}
                    requestThumbnails={requestThumbnails}
                  />
                </div>
                {isSettingsOpen && appSettings && (
                  <div className="absolute inset-0 z-50 flex bg-bg-secondary rounded-lg">
                    <div className="w-full h-full flex flex-col p-4 lg:p-8 overflow-y-auto custom-scrollbar">
                      <SettingsPanel
                        appSettings={appSettings}
                        onBack={() => setUI({ isSettingsOpen: false })}
                        onLibraryRefresh={handleLibraryRefresh}
                        onSettingsChange={handleSettingsChange}
                        rootPaths={rootPaths}
                      />
                    </div>
                  </div>
                )}
              </div>
              {activeView !== 'editor' && uiVisibility.rightPanel && isLibraryExportPanelVisible && (
                <div className="w-80 shrink-0 min-h-0 overflow-y-auto rounded-lg bg-bg-secondary">
                  {renderAppPanel(Panel.Export)}
                </div>
              )}
              {activeView === 'editor' && uiVisibility.rightPanel && hasMainContent && (
                <SidePanelArea
                  side="right"
                  width={effectiveRightWidth}
                  topRegion="rightTop"
                  bottomRegion="rightBottom"
                  renderPanel={renderAppPanel}
                  onWidthChange={createResizeHandler('right', effectiveRightWidth)}
                  onWidthReset={createResizeResetHandler('right')}
                  isResizing={isResizing}
                />
              )}
            </div>
            <DragOverlay modifiers={activeImageDragItem ? [imageDragModifier] : undefined} dropAnimation={null}>
              {activeLayoutDragItem && ActiveOverlayIcon ? (
                <div className="w-10 h-10 bg-surface shadow-2xl rounded-md flex items-center justify-center text-text-primary ring-1 ring-border-color">
                  <ActiveOverlayIcon size={20} />
                </div>
              ) : null}
              {activeImageDragItem ? <ImageDragOverlayNode activeItem={activeImageDragItem} /> : null}
            </DragOverlay>
          </DndContext>
        </div>
        <AppModals
          handleImageSelect={handleImageSelect}
          handleSavePanorama={handleSavePanorama}
          handleStartPanorama={handleStartPanorama}
          handleStartFocusStack={handleStartFocusStack}
          handleSaveFocusStack={handleSaveFocusStack}
          handleSaveHdr={handleSaveHdr}
          handleStartHdr={handleStartHdr}
          refreshImageList={handleLibraryRefresh}
          handleApplyDenoise={handleApplyDenoise}
          handleBatchDenoise={handleBatchDenoise}
          handleSaveDenoisedImage={handleSaveDenoisedImage}
          handleSaveRename={handleSaveRename}
          handleStartImport={handleStartImport}
          handleSetColorLabel={handleSetColorLabel}
          handleRate={handleRate}
          executeDelete={executeDelete}
          handleSaveCollage={handleSaveCollage}
          handleCreateAlbumItem={handleCreateAlbumItem}
          handleRenameAlbumItem={handleRenameAlbumItem}
        />
        <ToastContainer
          position="bottom-right"
          autoClose={5000}
          hideProgressBar={false}
          newestOnTop
          closeOnClick
          rtl={false}
          pauseOnFocusLoss
          draggable={false}
          pauseOnHover
          theme={isLightTheme ? 'light' : 'dark'}
          transition={Slide}
          toastClassName={() =>
            clsx(
              'relative flex min-h-16 p-4 rounded-lg justify-between overflow-hidden cursor-pointer mb-4',
              'bg-surface! text-text-primary! border! border-border-color! shadow-2xl! max-w-[420px]!',
            )
          }
        />
      </div>
    </>
  );
}

const AppWrapper = () => (
  <ContextMenuProvider>
    <App />
    <GlobalTooltip />
  </ContextMenuProvider>
);

export default AppWrapper;
