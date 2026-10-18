import { useState, useEffect, useRef } from 'react';
import { Star, Copy, ClipboardPaste, Check, Settings, FileOutput } from 'lucide-react';
import clsx from 'clsx';
import { motion, AnimatePresence } from 'framer-motion';
import { useShallow } from 'zustand/react/shallow';
import { useTranslation } from 'react-i18next';

import Filmstrip from './Filmstrip';
import FlagToggles from '../ui/FlagToggles';
import { GLOBAL_KEYS, ImageFile, SelectedImage, ThumbnailAspectRatio } from '../ui/AppProperties';
import Text from '../ui/Text';
import { useEditorStore } from '../../store/useEditorStore';
import { useUIStore } from '../../store/useUIStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import ImportChoiceMenu from './library/ImportChoiceMenu';
import { getZoomImageDimensions } from '../../utils/zoom';

interface BottomBarProps {
  filmstripHeight?: number;
  imageList?: Array<ImageFile>;
  imageRatings?: Record<string, number> | null;
  isCopied: boolean;
  isCopyDisabled: boolean;
  isExportDisabled?: boolean;
  isImportDisabled?: boolean;
  isFilmstripVisible?: boolean;
  isLibraryView?: boolean;
  isLoading?: boolean;
  isPasted: boolean;
  isPasteDisabled: boolean;
  isRatingDisabled?: boolean;
  isResetDisabled?: boolean;
  isResizing?: boolean;
  multiSelectedPaths?: Array<string>;
  onClearSelection?(): void;
  onContextMenu?(event: any, path: string): void;
  onEmptyAreaContextMenu?(event: any): void;
  onCopy(): void;
  onExportClick?(): void;
  onImportClick?(): void;
  onImportFolderClick?(): void;
  onImageSelect?(path: string, event: any): void;
  onOpenCopyPasteSettings?(): void;
  onRequestThumbnails?(paths: string[]): void;
  onPaste(): void;
  onRate(rate: number): void;
  onReset?(): void;
  onZoomChange?(zoomValue: number, fitToWindow?: boolean): void;
  rating: number;
  selectedImage?: SelectedImage;
  showFilmstrip?: boolean;
  showZoomControls?: boolean;
  thumbnailAspectRatio: ThumbnailAspectRatio;
  totalImages?: number;
}

interface StarRatingProps {
  disabled: boolean;
  onRate(rate: number): void;
  rating: number;
}

const StarRating = ({ rating, onRate, disabled }: StarRatingProps) => {
  const { t } = useTranslation();

  return (
    <div className={clsx('flex items-center gap-1', disabled && 'cursor-not-allowed')}>
      {[...Array(5)].map((_, index: number) => {
        const starValue = index + 1;
        return (
          <button
            className="disabled:cursor-not-allowed"
            disabled={disabled}
            key={starValue}
            onClick={() => !disabled && onRate(starValue === rating ? 0 : starValue)}
            data-tooltip={
              disabled
                ? t('ui.bottomBar.tooltips.selectToRate')
                : t('ui.bottomBar.tooltips.rateStars', { count: starValue })
            }
          >
            <Star
              size={18}
              className={clsx(
                'transition-colors duration-150',
                disabled
                  ? 'text-text-secondary opacity-40'
                  : starValue <= rating
                    ? 'fill-accent text-accent'
                    : 'text-text-secondary hover:text-accent',
              )}
            />
          </button>
        );
      })}
    </div>
  );
};

interface PanelToggleButtonProps {
  onClick: () => void;
  Icon: React.ElementType;
  tooltip: string;
  disabled?: boolean;
}

const PanelToggleButton = ({ onClick, Icon, tooltip, disabled = false }: PanelToggleButtonProps) => (
  <button
    className={clsx(
      'p-1.5 rounded-md transition-colors',
      disabled
        ? 'text-text-secondary opacity-40 cursor-not-allowed'
        : 'text-text-secondary hover:bg-surface hover:text-text-primary',
    )}
    onClick={() => !disabled && onClick()}
    disabled={disabled}
    data-tooltip={tooltip}
  >
    <Icon size={18} />
  </button>
);

export default function BottomBar({
  filmstripHeight,
  imageList = [],
  imageRatings,
  isCopied,
  isCopyDisabled,
  isExportDisabled = false,
  isImportDisabled = false,
  isFilmstripVisible,
  isLibraryView = false,
  isLoading = false,
  isPasted,
  isPasteDisabled,
  isRatingDisabled = false,
  isResizing,
  multiSelectedPaths = [],
  onClearSelection,
  onContextMenu,
  onEmptyAreaContextMenu,
  onCopy,
  onExportClick,
  onImportClick,
  onImportFolderClick,
  onImageSelect,
  onOpenCopyPasteSettings,
  onRequestThumbnails,
  onPaste,
  onRate,
  onZoomChange = () => {},
  rating,
  selectedImage,
  showFilmstrip = true,
  showZoomControls = true,
  thumbnailAspectRatio,
  totalImages,
}: BottomBarProps) {
  const { t } = useTranslation();
  const appSettings = useSettingsStore((state) => state.appSettings);
  const maxZoomPercent = Math.max(100, appSettings?.maxZoomPercent ?? 100);
  const maxZoom = maxZoomPercent / 100;

  const { isInstantTransition } = useUIStore(
    useShallow((state) => ({
      isInstantTransition: state.isInstantTransition,
    })),
  );

  const { displaySize, originalSize, adjustments } = useEditorStore(
    useShallow((state) => ({
      displaySize: state.displaySize,
      originalSize: state.originalSize,
      adjustments: state.adjustments,
    })),
  );

  const [isEditingPercent, setIsEditingPercent] = useState(false);
  const [percentInputValue, setPercentInputValue] = useState('');
  const isDraggingSlider = useRef(false);
  const [isZoomActive, setIsZoomActive] = useState(false);

  const percentInputRef = useRef<HTMLInputElement>(null);
  const [isZoomLabelHovered, setIsZoomLabelHovered] = useState(false);
  const isZoomReady = !isLoading && originalSize && originalSize.width > 0 && displaySize && displaySize.width > 0;

  const zoomImageDimensions = getZoomImageDimensions(originalSize, adjustments.crop, adjustments.orientationSteps);
  const currentOriginalPercent =
    isZoomReady && zoomImageDimensions.width > 0
      ? (displaySize.width * (typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1)) /
        zoomImageDimensions.width
      : 1.0;

  const [latchedSliderValue, setLatchedSliderValue] = useState(1.0);
  const [latchedDisplayPercent, setLatchedDisplayPercent] = useState(100);

  const numSelected = multiSelectedPaths.length;
  const total = totalImages ?? 0;
  const showSelectionCounter = numSelected > 1;

  const currentHeight = filmstripHeight ?? 120;
  const isCollapsed = !isFilmstripVisible;
  const effectiveHeight = isFilmstripVisible ? currentHeight : 0;
  const shouldAnimate = !isInstantTransition && (!isResizing || isCollapsed);

  useEffect(() => {
    if (isZoomReady && !isDraggingSlider.current) {
      setLatchedSliderValue(currentOriginalPercent);
      setLatchedDisplayPercent(Math.round(currentOriginalPercent * 100));
    }
  }, [currentOriginalPercent, isZoomReady]);

  useEffect(() => {
    const handleDragEndGlobal = () => {
      if (isZoomActive) {
        setIsZoomActive(false);
        isDraggingSlider.current = false;
        if (isZoomReady) {
          setLatchedDisplayPercent(Math.round(currentOriginalPercent * 100));
        }
      }
    };

    if (isZoomActive) {
      window.addEventListener('mouseup', handleDragEndGlobal);
      window.addEventListener('touchend', handleDragEndGlobal);
    }

    return () => {
      window.removeEventListener('mouseup', handleDragEndGlobal);
      window.removeEventListener('touchend', handleDragEndGlobal);
    };
  }, [isZoomActive, isZoomReady, currentOriginalPercent]);

  const handleSliderChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newZoom = parseFloat(e.target.value);
    setLatchedSliderValue(newZoom);
    setLatchedDisplayPercent(Math.round(newZoom * 100));
    onZoomChange(newZoom);
  };

  const handleMouseDown = () => {
    isDraggingSlider.current = true;
    setIsZoomActive(true);
  };

  const handleMouseUp = () => {
    isDraggingSlider.current = false;
    setIsZoomActive(false);
    if (isZoomReady) {
      setLatchedDisplayPercent(Math.round(currentOriginalPercent * 100));
    }
  };

  const handleZoomKeyDown = (e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && ['z', 'y'].includes(e.key.toLowerCase())) {
      (e.target as HTMLElement).blur();
      return;
    }
    if (GLOBAL_KEYS.includes(e.key)) {
      (e.target as HTMLElement).blur();
    }
  };

  const handleResetZoom = () => {
    onZoomChange(0, true);
  };

  const handlePercentClick = () => {
    if (!isZoomReady) return;
    setIsEditingPercent(true);
    setPercentInputValue(latchedDisplayPercent.toString());
    setTimeout(() => {
      percentInputRef.current?.focus();
      percentInputRef.current?.select();
    }, 0);
  };

  const handlePercentSubmit = () => {
    const value = parseFloat(percentInputValue);
    if (!isNaN(value)) {
      const originalPercent = value / 100;
      const clampedPercent = Math.max(0.1, Math.min(maxZoom, originalPercent));
      onZoomChange(clampedPercent);
    }
    setIsEditingPercent(false);
    setPercentInputValue('');
  };

  const handlePercentKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') handlePercentSubmit();
    else if (e.key === 'Escape') {
      setIsEditingPercent(false);
      setPercentInputValue('');
    }
    e.stopPropagation();
  };

  return (
    <div className="relative z-10 shrink-0 bg-bg-secondary rounded-lg flex flex-col">
      {!isLibraryView && showFilmstrip && (
        <div
          className={clsx(
            'overflow-hidden shrink-0 relative',
            shouldAnimate && 'transition-all duration-300 ease-in-out',
          )}
          style={{ height: `${effectiveHeight}px` }}
        >
          <div
            className={clsx(
              'w-full p-2 duration-300 ease-in-out',
              shouldAnimate ? 'transition-all' : 'transition-opacity',
              isCollapsed ? 'opacity-0 pointer-events-none' : 'opacity-100 pointer-events-auto',
            )}
            style={{ height: `${currentHeight}px` }}
          >
            <Filmstrip
              imageList={imageList}
              imageRatings={imageRatings}
              isLoading={isLoading}
              multiSelectedPaths={multiSelectedPaths}
              onClearSelection={onClearSelection}
              onContextMenu={onContextMenu}
              onEmptyAreaContextMenu={onEmptyAreaContextMenu}
              onImageSelect={onImageSelect}
              onRequestThumbnails={onRequestThumbnails}
              selectedImage={selectedImage}
              thumbnailAspectRatio={thumbnailAspectRatio}
            />
          </div>
        </div>
      )}

      <div
        className={clsx(
          'shrink-0 h-12 flex items-center justify-between pl-3 pr-12',
          !isLibraryView && 'border-t transition-colors duration-300',
          !isLibraryView && showFilmstrip && isFilmstripVisible ? 'border-surface' : 'border-transparent',
        )}
      >
        <div className="flex items-center gap-4">
          <StarRating rating={rating} onRate={onRate} disabled={isRatingDisabled} />
          <FlagToggles />
          <div className="h-5 w-px bg-surface"></div>
          <div className="flex items-center gap-2">
            <button
              className="relative w-8 h-8 flex items-center justify-center rounded-md text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-40 disabled:hover:bg-transparent disabled:cursor-not-allowed"
              disabled={isCopyDisabled}
              onClick={onCopy}
              data-tooltip={t('ui.bottomBar.tooltips.copySettings')}
            >
              <AnimatePresence mode="wait" initial={false}>
                {isCopied ? (
                  <motion.div
                    key="copied"
                    initial={{ opacity: 0, scale: 0.5 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.5 }}
                    transition={{ duration: 0.15 }}
                    className="absolute"
                  >
                    <Check size={18} className="text-green-500" />
                  </motion.div>
                ) : (
                  <motion.div
                    key="copy"
                    initial={{ opacity: 0, scale: 0.5 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.5 }}
                    transition={{ duration: 0.15 }}
                    className="absolute"
                  >
                    <Copy size={18} />
                  </motion.div>
                )}
              </AnimatePresence>
            </button>

            <button
              className="relative w-8 h-8 flex items-center justify-center rounded-md text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-40 disabled:hover:bg-transparent disabled:cursor-not-allowed"
              disabled={isPasteDisabled}
              onClick={onPaste}
              data-tooltip={t('ui.bottomBar.tooltips.pasteSettings')}
            >
              <AnimatePresence mode="wait" initial={false}>
                {isPasted ? (
                  <motion.div
                    key="pasted"
                    initial={{ opacity: 0, scale: 0.5 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.5 }}
                    transition={{ duration: 0.15 }}
                    className="absolute"
                  >
                    <Check size={18} className="text-green-500" />
                  </motion.div>
                ) : (
                  <motion.div
                    key="paste"
                    initial={{ opacity: 0, scale: 0.5 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.5 }}
                    transition={{ duration: 0.15 }}
                    className="absolute"
                  >
                    <ClipboardPaste size={18} />
                  </motion.div>
                )}
              </AnimatePresence>
            </button>

            <button
              className="w-8 h-8 flex items-center justify-center rounded-md text-text-secondary hover:bg-surface hover:text-text-primary transition-colors"
              onClick={onOpenCopyPasteSettings}
              data-tooltip={t('ui.bottomBar.tooltips.copyPasteSettings')}
            >
              <Settings size={18} />
            </button>
          </div>

          <div className="h-5 w-px bg-surface"></div>

          <div
            className={clsx(
              'flex items-center transition-all duration-300 ease-out overflow-hidden',
              showSelectionCounter ? 'max-w-xs opacity-100' : 'max-w-0 opacity-0',
            )}
          >
            <div className="h-5 w-px bg-surface mr-4"></div>
            <Text as="span" className="whitespace-nowrap">
              {t('ui.bottomBar.imagesSelected', { current: numSelected, total })}
            </Text>
          </div>
        </div>

        <div className="grow" />

        <div className="flex items-center gap-4">
          {isLibraryView && (
            <div className="flex items-center gap-1">
              <ImportChoiceMenu
                onFiles={() => onImportClick?.()}
                onFolder={() => onImportFolderClick?.()}
                iconOnly
                disabled={isImportDisabled}
              />
              <PanelToggleButton
                onClick={() => onExportClick?.()}
                Icon={FileOutput}
                tooltip="Export"
                disabled={isExportDisabled}
              />
            </div>
          )}
          {!isLibraryView && showZoomControls && (
            <>
              <div className="flex items-center gap-2 w-56">
                <div
                  className="relative w-12 h-full flex items-center justify-end cursor-pointer"
                  onClick={handleResetZoom}
                  onMouseEnter={() => setIsZoomLabelHovered(true)}
                  onMouseLeave={() => setIsZoomLabelHovered(false)}
                  data-tooltip={t('ui.bottomBar.tooltips.resetZoom')}
                >
                  <span className="absolute right-0 text-xs text-text-secondary select-none text-right w-max transition-colors hover:text-text-primary">
                    {isZoomLabelHovered ? t('ui.bottomBar.zoomLabelReset') : t('ui.bottomBar.zoomLabel')}
                  </span>
                </div>

                <div className="relative flex-1 h-5">
                  <div className="absolute top-1/2 left-0 w-full h-1.5 -translate-y-1/2 bg-surface rounded-full pointer-events-none" />
                  <input
                    type="range"
                    min={0.1}
                    max={maxZoom}
                    step="0.05"
                    value={latchedSliderValue}
                    onChange={handleSliderChange}
                    onKeyDown={handleZoomKeyDown}
                    onMouseDown={handleMouseDown}
                    onMouseUp={handleMouseUp}
                    onTouchStart={handleMouseDown}
                    onTouchEnd={handleMouseUp}
                    className={`absolute top-1/2 left-0 w-full h-1.5 mt-[-1.5px] appearance-none bg-transparent cursor-pointer p-0 slider-input z-10 ${
                      isZoomActive ? 'slider-thumb-active' : ''
                    }`}
                  />
                </div>

                <div className="relative text-xs text-text-secondary w-6 text-right flex items-center justify-end h-5 gap-1">
                  {isEditingPercent ? (
                    <input
                      ref={percentInputRef}
                      type="text"
                      value={percentInputValue}
                      onChange={(e) => setPercentInputValue(e.target.value)}
                      onKeyDown={handlePercentKeyDown}
                      onBlur={handlePercentSubmit}
                      className="w-full text-xs text-text-primary bg-bg-primary border border-border-color rounded-sm px-1 text-right"
                      style={{ fontSize: '12px', height: '18px' }}
                    />
                  ) : (
                    <span
                      onClick={handlePercentClick}
                      className="cursor-pointer hover:text-text-primary transition-colors select-none"
                      data-tooltip={t('ui.bottomBar.tooltips.customZoom')}
                    >
                      {latchedDisplayPercent}%
                    </span>
                  )}
                </div>
              </div>

              <div className="h-5 w-px bg-surface"></div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
