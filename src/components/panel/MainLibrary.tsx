import { Settings, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import Button from '../ui/Button';
import { AppSettings, ImageFile, ThumbnailSize, ThumbnailAspectRatio, LibraryDisplayMode } from '../ui/AppProperties';
import { GroupBadgeInfo, GroupId } from '../../utils/imageGrouping';
import { useUIStore } from '../../store/useUIStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import LibraryGrid from './library/LibraryGrid';
import ImportChoiceMenu from './library/ImportChoiceMenu';

const LIBRARY_SELECT_CLASS = 'bg-surface text-text-primary rounded p-1';

interface MainLibraryProps {
  activePath: string | null;
  appSettings: AppSettings | null;
  currentFolderPath: string | null;
  groupBadgeInfo: Map<GroupId, GroupBadgeInfo> | null;
  imageList: Array<ImageFile>;
  imageRatings: Record<string, number>;
  isLoading: boolean;
  multiSelectedPaths: Array<string>;
  onClearSelection(): void;
  onContextMenu(event: any, path: string): void;
  onEmptyAreaContextMenu(event: any): void;
  onImageClick(path: string, event: any): void;
  onImageDoubleClick(path: string): void;
  onImportClick(): void;
  onOpenFolder(): void;
  onSettingsChange(settings: AppSettings): Promise<void>;
  onThumbnailSizeChange(size: ThumbnailSize): void;
  onThumbnailAspectRatioChange(ratio: ThumbnailAspectRatio): void;
  onRequestThumbnails?(paths: string[]): void;
  thumbnailAspectRatio: ThumbnailAspectRatio;
  thumbnailSize: ThumbnailSize;
}

export interface ColumnWidths {
  thumbnail: number;
  name: number;
  date: number;
  rating: number;
  color: number;
  shutter: number;
  aperture: number;
  iso: number;
  focal: number;
}

export default function MainLibrary(props: MainLibraryProps) {
  const { t } = useTranslation();
  const setUI = useUIStore((s) => s.setUI);
  const showTopPanel = useUIStore((s) => s.uiVisibility.topPanel);
  const { catalogStatus, catalogOffset, catalogTotal, isCatalogMode, sortCriteria, setSortCriteria } =
    useLibraryStore();
  const total = isCatalogMode ? catalogTotal : props.imageList.length;
  const mode =
    props.appSettings?.libraryDisplayMode === LibraryDisplayMode.List
      ? LibraryDisplayMode.List
      : LibraryDisplayMode.Grid;
  const sizes = [
    { id: ThumbnailSize.Small, size: 160, label: t('catalog.small') },
    { id: ThumbnailSize.Medium, size: 240, label: t('catalog.medium') },
    { id: ThumbnailSize.Large, size: 320, label: t('catalog.large') },
  ];
  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden bg-bg-secondary rounded-lg">
      {showTopPanel && (
        <header className="shrink-0 flex items-center gap-2 p-3 border-b border-surface">
          <span className="text-sm text-text-secondary grow">
            {isCatalogMode && catalogStatus && total !== catalogStatus.imageCount
              ? t(total === 1 ? 'catalog.filteredTotalOne' : 'catalog.filteredTotal', { images: total })
              : t('catalog.total', { count: total })}
          </span>
          {props.isLoading && <Loader2 className="animate-spin" size={18} />}
          <select
            aria-label={t('catalog.sort')}
            className={LIBRARY_SELECT_CLASS}
            value={sortCriteria.key}
            onChange={(e) => setSortCriteria({ key: e.target.value })}
          >
            <option value="name">{t('catalog.name')}</option>
            <option value="date_taken">{t('catalog.captureDate')}</option>
            <option value="date">{t('catalog.modifiedDate')}</option>
            <option value="rating">{t('catalog.rating')}</option>
            <option value="shutter_speed">{t('library.grid.columns.shutter')}</option>
          </select>
          <button
            aria-label={t('catalog.reverse')}
            onClick={() => setSortCriteria({ order: sortCriteria.order === 'asc' ? 'desc' : 'asc' })}
          >
            {sortCriteria.order === 'asc' ? '↑' : '↓'}
          </button>
          <select
            aria-label={t('catalog.display')}
            className={LIBRARY_SELECT_CLASS}
            value={mode}
            onChange={(e) =>
              props.appSettings &&
              props.onSettingsChange({ ...props.appSettings, libraryDisplayMode: e.target.value as LibraryDisplayMode })
            }
          >
            <option value={LibraryDisplayMode.Grid}>{t('catalog.grid')}</option>
            <option value={LibraryDisplayMode.List}>{t('catalog.list')}</option>
          </select>
          <select
            aria-label={t('catalog.thumbnailSize')}
            className={LIBRARY_SELECT_CLASS}
            value={props.thumbnailSize}
            onChange={(e) => props.onThumbnailSizeChange(e.target.value as ThumbnailSize)}
          >
            {sizes.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
          <select
            aria-label={t('catalog.thumbnailLayout')}
            className={LIBRARY_SELECT_CLASS}
            value={props.thumbnailAspectRatio}
            onChange={(e) => props.onThumbnailAspectRatioChange(e.target.value as ThumbnailAspectRatio)}
          >
            <option value={ThumbnailAspectRatio.Contain}>{t('catalog.originalRatio')}</option>
            <option value={ThumbnailAspectRatio.Cover}>{t('catalog.fillSquare')}</option>
          </select>
          <Button onClick={() => setUI({ isSettingsOpen: true })} data-tooltip={t('settings.title')}>
            <Settings size={18} />
          </Button>
        </header>
      )}
      {total > 0 ? (
        <LibraryGrid
          {...props}
          catalogRange={isCatalogMode ? { offset: catalogOffset, total } : undefined}
          libraryDisplayMode={mode}
          thumbnailSizeOptions={sizes}
        />
      ) : (
        <div className="flex flex-col items-center justify-center flex-1 gap-3 text-text-secondary">
          <p>
            {props.isLoading
              ? t('catalog.loading')
              : catalogStatus?.imageCount
                ? t('catalog.noMatches')
                : t('catalog.getStarted')}
          </p>
          {!catalogStatus?.imageCount && (
            <ImportChoiceMenu onFiles={props.onImportClick} onFolder={props.onOpenFolder} />
          )}
        </div>
      )}
    </div>
  );
}
