import { useShallow } from 'zustand/react/shallow';

import PanelVisibilityMenu from '../panel/PanelVisibilityMenu';
import MainLibrary from '../panel/MainLibrary';
import BottomBar from '../panel/BottomBar';

import { useUIStore } from '../../store/useUIStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import { useSettingsStore } from '../../store/useSettingsStore';

import { ImageFile, ThumbnailAspectRatio, ThumbnailSize } from '../ui/AppProperties';
import { GroupBadgeInfo, GroupId } from '../../utils/imageGrouping';
import CatalogFilterPanel from '../panel/library/CatalogFilterPanel';

interface LibraryViewProps {
  sortedImageList: ImageFile[];
  groupBadgeInfo: Map<GroupId, GroupBadgeInfo> | null;
  thumbnailSize: ThumbnailSize;
  thumbnailAspectRatio: ThumbnailAspectRatio;
  setThumbnailSize: (size: ThumbnailSize) => void;
  setThumbnailAspectRatio: (ratio: ThumbnailAspectRatio) => void;
  handleClearSelection: () => void;
  handleLibraryImageSingleClick: (...args: any) => void;
  handleImageSelect: (...args: any) => void;
  handleRate: (...args: any) => void;
  handleThumbnailContextMenu: (...args: any) => void;
  handleMainLibraryContextMenu: (...args: any) => void;
  handleOpenFolder: (...args: any) => void;
  handleImportClick: (path: string) => void;
  handleCopyAdjustments: () => void;
  handlePasteAdjustments: () => void;
  handleResetAdjustments: () => void;
  requestThumbnails: any;
}

export default function LibraryView({
  sortedImageList,
  groupBadgeInfo,
  thumbnailSize,
  thumbnailAspectRatio,
  setThumbnailSize,
  setThumbnailAspectRatio,
  handleClearSelection,
  handleLibraryImageSingleClick,
  handleImageSelect,
  handleRate,
  handleThumbnailContextMenu,
  handleMainLibraryContextMenu,
  handleOpenFolder,
  handleImportClick,
  handleCopyAdjustments,
  handlePasteAdjustments,
  handleResetAdjustments,
  requestThumbnails,
}: LibraryViewProps) {
  const { setUI, uiVisibility } = useUIStore(
    useShallow((state) => ({
      setUI: state.setUI,
      uiVisibility: state.uiVisibility,
    })),
  );

  const {
    rootPaths,
    currentFolderPath,
    libraryActivePath,
    multiSelectedPaths,
    catalogTotal,
    imageRatings,
    isCatalogMode,
    isViewLoading,
  } = useLibraryStore(
    useShallow((state) => ({
      rootPaths: state.rootPaths,
      currentFolderPath: state.currentFolderPath,
      libraryActivePath: state.libraryActivePath,
      multiSelectedPaths: state.multiSelectedPaths,
      catalogTotal: state.catalogTotal,
      imageRatings: state.imageRatings,
      isCatalogMode: state.isCatalogMode,
      isViewLoading: state.isViewLoading,
    })),
  );

  const { appSettings, handleSettingsChange } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      handleSettingsChange: state.handleSettingsChange,
    })),
  );

  const { isCopied, isPasted } = useProcessStore(
    useShallow((state) => ({
      isCopied: state.isCopied,
      isPasted: state.isPasted,
    })),
  );

  return (
    <div className="flex flex-row grow h-full min-h-0">
      <div className="relative flex-1 flex flex-col min-w-0 min-h-0 gap-2">
        <PanelVisibilityMenu />
        <MainLibrary
          activePath={libraryActivePath}
          appSettings={appSettings}
          currentFolderPath={currentFolderPath}
          groupBadgeInfo={groupBadgeInfo}
          imageList={sortedImageList}
          imageRatings={imageRatings}
          isLoading={isViewLoading}
          multiSelectedPaths={multiSelectedPaths}
          onClearSelection={handleClearSelection}
          onContextMenu={handleThumbnailContextMenu}
          onEmptyAreaContextMenu={handleMainLibraryContextMenu}
          onImageClick={handleLibraryImageSingleClick}
          onImageDoubleClick={handleImageSelect}
          onImportClick={() => handleImportClick(currentFolderPath as string)}
          onOpenFolder={handleOpenFolder}
          onSettingsChange={handleSettingsChange}
          onThumbnailSizeChange={setThumbnailSize}
          onThumbnailAspectRatioChange={setThumbnailAspectRatio}
          onRequestThumbnails={requestThumbnails}
          thumbnailAspectRatio={thumbnailAspectRatio}
          thumbnailSize={thumbnailSize}
        />
        {uiVisibility.bottomPanel && ((rootPaths && rootPaths.length > 0) || currentFolderPath || isCatalogMode) && (
          <BottomBar
            isCopied={isCopied}
            isCopyDisabled={multiSelectedPaths.length !== 1}
            isExportDisabled={multiSelectedPaths.length === 0}
            isImportDisabled={false}
            isLibraryView={true}
            isPasted={isPasted}
            isPasteDisabled={useEditorStore.getState().copiedAdjustments === null || multiSelectedPaths.length === 0}
            isRatingDisabled={multiSelectedPaths.length === 0}
            isResetDisabled={multiSelectedPaths.length === 0}
            multiSelectedPaths={multiSelectedPaths}
            onCopy={handleCopyAdjustments}
            onExportClick={() =>
              setUI((state) => ({ isLibraryExportPanelVisible: !state.isLibraryExportPanelVisible }))
            }
            onImportClick={() => handleImportClick(currentFolderPath ?? '')}
            onImportFolderClick={handleOpenFolder}
            onOpenCopyPasteSettings={() => setUI({ isCopyPasteSettingsModalOpen: true })}
            onPaste={() => handlePasteAdjustments()}
            onRate={handleRate}
            onReset={() => handleResetAdjustments()}
            rating={imageRatings[libraryActivePath || ''] || 0}
            thumbnailAspectRatio={thumbnailAspectRatio}
            totalImages={isCatalogMode ? catalogTotal : sortedImageList.length}
          />
        )}
      </div>
      {uiVisibility.rightPanel && <CatalogFilterPanel />}
    </div>
  );
}
