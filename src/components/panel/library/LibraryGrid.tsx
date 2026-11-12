import React, { useState, useEffect, useLayoutEffect, useRef, useMemo } from 'react';
import { ChevronUp, ChevronDown } from 'lucide-react';
import debounce from 'lodash.debounce';
import { toast } from 'react-toastify';
import { useTranslation } from 'react-i18next';
import { Row } from './LibraryItems';
import { useShallow } from 'zustand/react/shallow';
import { useLibraryStore } from '../../../store/useLibraryStore';
import { ImageFile, SortDirection, LibraryDisplayMode } from '../../ui/AppProperties';
import Text from '../../ui/Text';
import { TextColors, TextVariants, TextWeights, TEXT_COLOR_KEYS } from '../../../types/typography';
import { useProcessStore } from '../../../store/useProcessStore';
import { ExifOverlay } from '../../ui/AppProperties';
import { useSettingsStore } from '../../../store/useSettingsStore';
import { loadCatalogWindow } from '../../../utils/catalog';

function ListHeader({ widths, setWidths, containerRef, sortCriteria, onSortChange }: any) {
  const { t } = useTranslation();
  const exifOverlay = useSettingsStore((s) => s.appSettings?.exifOverlay || ExifOverlay.Off);
  const showExifCols = exifOverlay !== ExifOverlay.Off;
  const totalRawWidth =
    widths.thumbnail +
    widths.name +
    widths.date +
    widths.rating +
    widths.color +
    (showExifCols ? widths.shutter + widths.aperture + widths.iso + widths.focal : 0);

  const handleResize = (e: React.MouseEvent, leftCol: string, rightCol: string) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startLeftWidth = widths[leftCol];
    const startRightWidth = widths[rightCol];
    const containerWidth = containerRef.current?.clientWidth || 1000;

    const onMouseMove = (moveEvent: MouseEvent) => {
      const deltaX = moveEvent.clientX - startX;
      const deltaPercent = (deltaX / containerWidth) * 100;

      let newLeft = startLeftWidth + deltaPercent;
      let newRight = startRightWidth - deltaPercent;

      if (newLeft < 1) {
        newRight -= 1 - newLeft;
        newLeft = 1;
      }
      if (newRight < 1) {
        newLeft -= 1 - newRight;
        newRight = 1;
      }

      setWidths((prev: any) => ({
        ...prev,
        [leftCol]: newLeft,
        [rightCol]: newRight,
      }));
    };

    const onMouseUp = () => {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    };

    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  };

  const Column = ({ title, widthKey, nextKey, sortKey }: any) => {
    const isSorted = sortCriteria.key === sortKey;
    const isAsc = sortCriteria.order === SortDirection.Ascending;
    const actualWidth = `${(widths[widthKey] / totalRawWidth) * 100}%`;

    return (
      <div
        style={{ width: actualWidth }}
        className={`relative flex items-center px-3 h-full select-none ${
          sortKey ? 'cursor-pointer hover:bg-bg-primary/50 transition-colors' : ''
        }`}
        onClick={() => sortKey && onSortChange(sortKey)}
      >
        <Text
          variant={TextVariants.small}
          weight={TextWeights.semibold}
          color={isSorted ? TextColors.primary : TextColors.secondary}
          className="uppercase tracking-wider text-[11px]"
        >
          {title}
        </Text>
        {isSorted && (
          <span className={`ml-1 flex items-center ${TEXT_COLOR_KEYS[TextColors.primary]}`}>
            {isAsc ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          </span>
        )}
        {nextKey && (
          <div
            className="absolute right-[-3px] top-1.5 bottom-1.5 w-[6px] cursor-col-resize z-10 group flex items-center justify-center"
            onMouseDown={(e) => handleResize(e, widthKey, nextKey)}
          >
            <div className="w-px h-full bg-border-color/40 group-hover:bg-accent transition-colors" />
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="flex items-center w-full h-9 bg-bg-secondary/80 backdrop-blur-sm border-b border-border-color/50 shrink-0">
      <Column title="" widthKey="thumbnail" nextKey="name" />
      <Column title={t('library.grid.columns.name')} widthKey="name" nextKey="date" sortKey="name" />
      <Column title={t('library.grid.columns.modified')} widthKey="date" nextKey="rating" sortKey="date" />
      <Column title={t('library.grid.columns.rating')} widthKey="rating" nextKey="color" sortKey="rating" />
      {showExifCols ? (
        <>
          <Column title={t('library.grid.columns.label')} widthKey="color" nextKey="shutter" />
          <Column
            title={t('library.grid.columns.shutter')}
            widthKey="shutter"
            nextKey="aperture"
            sortKey="shutter_speed"
          />
          <Column title={t('library.grid.columns.aperture')} widthKey="aperture" nextKey="iso" sortKey="aperture" />
          <Column title={t('library.grid.columns.iso')} widthKey="iso" nextKey="focal" sortKey="iso" />
          <Column title={t('library.grid.columns.focal')} widthKey="focal" sortKey="focal_length" />
        </>
      ) : (
        <Column title={t('library.grid.columns.label')} widthKey="color" />
      )}
    </div>
  );
}

export default function LibraryGrid(props: any) {
  const { t } = useTranslation();
  const {
    imageList,
    thumbnailSize,
    libraryDisplayMode,
    currentFolderPath,
    activePath,
    multiSelectedPaths,
    onRequestThumbnails,
    thumbnailSizeOptions,
    onThumbnailSizeChange,
    catalogRange,
  } = props;
  const catalogOffset: number = catalogRange?.offset ?? 0;
  const catalogTotal: number | undefined = catalogRange?.total;
  const { listColumnWidths, setLibrary, sortCriteria, setSortCriteria } = useLibraryStore(
    useShallow((state) => ({
      listColumnWidths: state.listColumnWidths,
      setLibrary: state.setLibrary,
      sortCriteria: state.sortCriteria,
      setSortCriteria: state.setSortCriteria,
    })),
  );
  const exifOverlay = useSettingsStore((s) => s.appSettings?.exifOverlay || ExifOverlay.Off);
  const containerRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const [scrollTop, setScrollTop] = useState(0);

  useLayoutEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const measure = () => {
      const { clientWidth: width, clientHeight: height } = element;
      setViewport((old) => (old.width === width && old.height === height ? old : { width, height }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const handleWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      const index = thumbnailSizeOptions.findIndex((option: { id: string }) => option.id === thumbnailSize);
      if (index < 0) return;
      const next = Math.min(thumbnailSizeOptions.length - 1, Math.max(0, index + (event.deltaY < 0 ? 1 : -1)));
      if (index !== next) onThumbnailSizeChange(thumbnailSizeOptions[next].id);
    };
    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [thumbnailSize, thumbnailSizeOptions, onThumbnailSizeChange]);

  const saveScroll = useMemo(() => debounce((top: number) => setLibrary({ libraryScrollTop: top }), 200), [setLibrary]);
  useEffect(() => () => saveScroll.cancel(), [saveScroll]);

  const queueThumbnailRequest = useMemo(() => {
    const paths = new Set<string>();
    const flush = debounce(
      () => {
        onRequestThumbnails?.([...paths]);
        paths.clear();
      },
      50,
      { maxWait: 100 },
    );
    return {
      add(path: string) {
        if (!onRequestThumbnails || useProcessStore.getState().thumbnails[path]) return;
        paths.add(path);
        flush();
      },
      cancel: flush.cancel,
    };
  }, [onRequestThumbnails]);
  useEffect(() => () => queueThumbnailRequest.cancel(), [queueThumbnailRequest]);

  const isListView = libraryDisplayMode === LibraryDisplayMode.List;
  const padding = isListView ? 0 : 12;
  const gap = isListView ? 0 : 12;
  const availableWidth = Math.max(1, viewport.width - 2 * padding);
  const minThumbWidth = thumbnailSizeOptions.find((option: { id: string }) => option.id === thumbnailSize)?.size || 240;
  const columns = isListView ? 1 : Math.max(1, Math.floor((availableWidth + gap) / (minThumbWidth + gap)));
  const itemWidth = (availableWidth - gap * (columns - 1)) / columns;
  const { thumbnail, name, date, rating, color, shutter, aperture, iso, focal } = listColumnWidths;
  const totalWidth =
    thumbnail + name + date + rating + color + (exifOverlay !== ExifOverlay.Off ? shutter + aperture + iso + focal : 0);
  const itemHeight = isListView ? Math.max(36, Math.min(300, (availableWidth * thumbnail) / totalWidth)) : itemWidth;
  const rowHeight = itemHeight + gap;
  const imageCount = catalogTotal ?? imageList.length;
  const rowCount = Math.ceil(imageCount / columns);
  const fullHeight = rowCount * rowHeight + (isListView ? 24 : padding);
  const scrollHeight = Math.min(fullHeight, 16_000_000);
  const scale = Math.max(1, fullHeight - viewport.height) / Math.max(1, scrollHeight - viewport.height);
  const logicalTop = scrollTop * scale;
  const first = Math.min(rowCount - 1, Math.max(0, Math.floor((logicalTop - padding) / rowHeight)));
  const last = Math.min(rowCount - 1, Math.floor((logicalTop + viewport.height - padding) / rowHeight));
  const ready = viewport.width > 0 && viewport.height > 0;

  useLayoutEffect(() => {
    const element = viewportRef.current;
    if (!element || !ready) return;
    element.scrollTop = useLibraryStore.getState().libraryScrollTop;
    setScrollTop(element.scrollTop);
  }, [currentFolderPath, ready]);

  useEffect(() => {
    if (catalogTotal === undefined || !ready || last < first) return;
    const index = Math.floor((first + last) / 2) * columns;
    loadCatalogWindow(index).catch((error) => toast.error(t('catalog.loadError', { error: String(error) })));
  }, [catalogTotal, columns, first, last, ready, t]);

  useEffect(() => {
    setLibrary({ libraryColumnCount: columns });
  }, [columns, setLibrary]);

  const previousActive = useRef({ path: null as string | null, mode: libraryDisplayMode, ready: false });
  useEffect(() => {
    const previous = previousActive.current;
    previousActive.current = { path: activePath, mode: libraryDisplayMode, ready };
    const element = viewportRef.current;
    if (!element || !ready || multiSelectedPaths.length > 1 || !activePath) return;
    if (previous.ready && previous.path === activePath && previous.mode === libraryDisplayMode) return;
    const index = imageList.findIndex((image: ImageFile) => image.path === activePath);
    if (index < 0) return;
    const top = Math.floor((catalogOffset + index) / columns) * rowHeight + padding;
    const visibleTop = element.scrollTop * scale;
    if (!previous.ready || previous.mode !== libraryDisplayMode) {
      element.scrollTop = Math.max(0, top - (element.clientHeight - rowHeight) / 2) / scale;
    } else if (top < visibleTop) {
      element.scrollTo({ top: Math.max(0, top - 120) / scale, behavior: 'smooth' });
    } else if (top + rowHeight > visibleTop + element.clientHeight) {
      element.scrollTo({ top: (top + rowHeight - element.clientHeight + 120) / scale, behavior: 'smooth' });
    }
  }, [
    activePath,
    catalogOffset,
    columns,
    imageList,
    libraryDisplayMode,
    multiSelectedPaths.length,
    padding,
    ready,
    rowHeight,
    scale,
  ]);

  const getImages = useMemo(() => {
    const rows = new Map<number, ImageFile[]>();
    for (let row = Math.ceil(catalogOffset / columns); row < rowCount; row++) {
      const start = row * columns;
      const end = Math.min(start + columns, imageCount) - catalogOffset;
      if (end > imageList.length) break;
      rows.set(row, imageList.slice(start - catalogOffset, end));
    }
    return (index: number) => rows.get(index);
  }, [catalogOffset, columns, imageCount, imageList, rowCount]);
  const selected = useMemo(() => new Set<string>(multiSelectedPaths), [multiSelectedPaths]);
  const renderedRows = [];
  const startRow = Math.max(0, first - 2);
  const endRow = Math.min(rowCount - 1, last + 2);
  for (let index = startRow; ready && index <= endRow; index++) {
    if (!getImages(index)) continue;
    renderedRows.push(
      <Row
        key={index}
        index={index}
        style={{
          position: 'absolute',
          top: padding + index * rowHeight + scrollTop - logicalTop,
          height: rowHeight,
        }}
        getImages={getImages}
        activePath={activePath}
        multiSelectedSet={selected}
        onContextMenu={props.onContextMenu}
        onImageClick={props.onImageClick}
        onImageDoubleClick={props.onImageDoubleClick}
        thumbnailAspectRatio={props.thumbnailAspectRatio}
        imageRatings={props.imageRatings}
        itemWidth={itemWidth}
        itemHeight={itemHeight}
        outerPadding={padding}
        gap={gap}
        isListView={isListView}
        columnWidths={listColumnWidths}
        queueThumbnailRequest={queueThumbnailRequest.add}
        groupBadgeInfo={props.groupBadgeInfo}
        rowCount={rowCount}
      />,
    );
  }

  const handleHeaderSort = (key: string) => {
    props.onClearSelection();
    setSortCriteria((previous) => {
      if (previous.key !== key) return { key, order: SortDirection.Ascending };
      if (previous.order === SortDirection.Ascending) return { key, order: SortDirection.Descending };
      return { key: 'name', order: SortDirection.Ascending };
    });
  };

  return (
    <div
      ref={containerRef}
      className="flex flex-col flex-1 min-h-0 min-w-0 w-full overflow-hidden"
      onClick={props.onClearSelection}
      onContextMenu={props.onEmptyAreaContextMenu}
    >
      {isListView && (
        <ListHeader
          widths={listColumnWidths}
          setWidths={(update: any) => setLibrary({ listColumnWidths: update(listColumnWidths) })}
          containerRef={containerRef}
          sortCriteria={sortCriteria}
          onSortChange={handleHeaderSort}
        />
      )}
      <div
        ref={viewportRef}
        role="list"
        className="relative flex-1 min-h-0 overflow-y-auto overflow-x-hidden custom-scrollbar [overflow-anchor:none] [scrollbar-gutter:stable]"
        onScroll={(event) => {
          const top = event.currentTarget.scrollTop;
          setScrollTop(top);
          saveScroll(top);
        }}
      >
        <div className="relative overflow-clip" style={{ height: scrollHeight }}>
          {renderedRows}
        </div>
      </div>
    </div>
  );
}
