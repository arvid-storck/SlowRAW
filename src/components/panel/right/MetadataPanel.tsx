import { useState, useMemo, useEffect } from 'react';
import { cameraIso } from '../../../utils/metadata';
import { invoke } from '@tauri-apps/api/core';
import { Check, ChevronDown, ChevronRight, Plus, Star, Tag, X, User } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import { Invokes } from '../../ui/AppProperties';
import { COLOR_LABELS, Color } from '../../../utils/adjustments';
import Text from '../../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { IconAperture, IconShutter, IconIso, IconFocalLength, IconLens } from '../editor/ExifIcons';
import { useEditorStore } from '../../../store/useEditorStore';
import { useLibraryStore } from '../../../store/useLibraryStore';
import { useSettingsStore } from '../../../store/useSettingsStore';
import { useProcessStore } from '../../../store/useProcessStore';
import { useLibraryActions } from '../../../hooks/useLibraryActions';
import { expandGroupedPaths } from '../../../utils/imageGrouping';

interface CameraSetting {
  format?(value: number): string | number;
  label: string;
}

interface CameraSettings {
  [index: string]: CameraSetting;
  ExposureTime: CameraSetting;
  FNumber: CameraSetting;
  FocalLengthIn35mmFilm: CameraSetting;
  LensModel: CameraSetting;
  PhotographicSensitivity: CameraSetting;
}

interface MetaDataItemProps {
  label: string;
  value: any;
}

const USER_TAG_PREFIX = 'user:';
const EMPTY_TAGS: string[] = [];

function formatExifTag(str: string) {
  if (!str) return '';
  return str.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/([A-Z])([A-Z][a-z])/g, '$1 $2');
}

const CAMERA_ICONS: Record<string, React.FC> = {
  FNumber: IconAperture,
  ExposureTime: IconShutter,
  PhotographicSensitivity: IconIso,
  FocalLengthIn35mmFilm: IconFocalLength,
  LensModel: IconLens,
};

function MetadataItem({ label, value }: MetaDataItemProps) {
  const strValue = String(value);

  return (
    <div className="flex justify-between items-start gap-4 py-1.5 px-2 rounded-md hover:bg-card-active transition-colors cursor-default">
      <Text
        variant={TextVariants.small}
        color={TextColors.secondary}
        weight={TextWeights.medium}
        className="shrink-0 mt-0.5"
      >
        {label}
      </Text>
      <Text
        variant={TextVariants.small}
        color={TextColors.primary}
        className="break-words min-w-0 text-right line-clamp-3 select-text"
        data-tooltip={strValue.length > 500 ? strValue.slice(0, 500) + '...' : strValue}
      >
        {strValue}
      </Text>
    </div>
  );
}

interface EditableMetadataItemProps {
  label: string;
  value: string;
  onSave: (val: string) => void;
}

function EditableMetadataItem({ label, value, onSave }: EditableMetadataItemProps) {
  const { t } = useTranslation();
  const [isEditing, setIsEditing] = useState(false);
  const [localValue, setLocalValue] = useState(value || '');

  useEffect(() => {
    setLocalValue(value || '');
    setIsEditing(false);
  }, [value]);

  const handleSave = () => {
    setIsEditing(false);
    const trimmedLocal = localValue.trim();
    const trimmedProp = (value || '').trim();
    if (trimmedLocal !== trimmedProp) {
      onSave(trimmedLocal);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') handleSave();
    if (e.key === 'Escape') {
      setLocalValue(value || '');
      setIsEditing(false);
    }
  };

  return (
    <div className="flex justify-between items-center gap-4 py-1 px-2 rounded-md">
      <Text
        variant={TextVariants.small}
        color={TextColors.secondary}
        weight={TextWeights.medium}
        className="shrink-0 truncate"
      >
        {label}
      </Text>

      <div className="w-[55%] shrink-0">
        {isEditing ? (
          <input
            autoFocus
            type="text"
            value={localValue}
            onChange={(e) => setLocalValue(e.target.value)}
            onBlur={handleSave}
            onKeyDown={handleKeyDown}
            className="bg-bg-secondary border border-accent rounded-sm px-2 py-0.5 text-xs text-text-primary text-right outline-hidden w-full shadow-sm focus:ring-1 focus:ring-accent/30"
          />
        ) : (
          <div
            onClick={() => setIsEditing(true)}
            className="text-xs px-2 py-0.5 min-h-[24px] flex items-center justify-end rounded-sm cursor-text border transition-colors text-right truncate w-full text-text-primary bg-bg-secondary/40 border-surface/50 hover:bg-bg-secondary/80 hover:border-text-tertiary/40"
            data-tooltip={value ? t('editor.metadata.clickToEdit') : t('editor.metadata.emptyClickToAdd')}
          >
            {value}
          </div>
        )}
      </div>
    </div>
  );
}

const EDITABLE_FIELDS = [
  { key: 'ImageDescription', label: 'title' },
  { key: 'Artist', label: 'author' },
  { key: 'Copyright', label: 'copyright' },
  { key: 'UserComment', label: 'comments' },
];

const KEY_CAMERA_SETTINGS_MAP: CameraSettings = {
  FNumber: {
    format: (value: number) => {
      const fStr = String(value);
      return fStr.toLowerCase().startsWith('f') ? fStr : `f/${fStr}`;
    },
    label: 'Aperture',
  },
  ExposureTime: {
    format: (value: number) => (String(value).endsWith('s') ? value : `${value}s`),
    label: 'Shutter Speed',
  },
  PhotographicSensitivity: {
    format: (value: number) => `${value}`,
    label: 'ISO',
  },
  FocalLengthIn35mmFilm: {
    format: (value: number) => (String(value).endsWith('mm') ? value : `${value} mm`),
    label: 'Focal Length',
  },
  LensModel: {
    format: (value: number) => String(value).replace(/"/g, ''),
    label: 'Lens',
  },
};

export default function MetadataPanel() {
  const { t } = useTranslation();
  const [isOrganizationExpanded, setIsOrganizationExpanded] = useState(false);
  const [isAuthorExpanded, setIsAuthorExpanded] = useState(false);
  const [tagInputValue, setTagInputValue] = useState('');
  const [isTagInputFocused, setIsTagInputFocused] = useState(false);
  const selectedImage = useEditorStore((s) => s.selectedImage);
  const multiSelectedPaths = useLibraryStore((s) => s.multiSelectedPaths);
  const imageRatings = useLibraryStore((s) => s.imageRatings);
  const appSettings = useSettingsStore((s) => s.appSettings);
  const thumbnails = useProcessStore((s) => s.thumbnails);

  const { handleRate, handleSetColorLabel, handleTagsChanged, handleUpdateExif } = useLibraryActions();

  const rating = selectedImage ? imageRatings[selectedImage.path] || 0 : 0;
  const tags = useLibraryStore((state) => {
    if (!selectedImage) return EMPTY_TAGS;
    return state.imageList.find((img) => img.path === selectedImage.path)?.tags ?? EMPTY_TAGS;
  });
  const liveThumbnailUrl = selectedImage ? thumbnails[selectedImage.path] : undefined;

  const targetPaths = multiSelectedPaths?.length > 0 ? multiSelectedPaths : selectedImage ? [selectedImage.path] : [];
  const getPathsToUpdate = () => {
    const { imageList } = useLibraryStore.getState();
    const groupingMode = useSettingsStore.getState().appSettings?.grouping ?? 'off';
    return expandGroupedPaths(imageList, targetPaths, groupingMode);
  };

  const { cameraGridSettings, lensSetting, otherExifEntries } = useMemo(() => {
    const exif = selectedImage?.exif || {};

    const cameraGridKeys = ['ExposureTime', 'FNumber', 'PhotographicSensitivity', 'FocalLengthIn35mmFilm'];
    const cameraGridSettings = cameraGridKeys.map((key) => {
      const value =
        key === 'PhotographicSensitivity'
          ? cameraIso(exif)
          : (exif[key] ?? (key === 'FocalLengthIn35mmFilm' ? exif.FocalLength : undefined));
      const hasValue = value !== undefined && value !== null && value !== '';

      const translatedLabel =
        key === 'FNumber'
          ? t('editor.metadata.camera.aperture')
          : key === 'ExposureTime'
            ? t('editor.metadata.camera.shutterSpeed')
            : key === 'PhotographicSensitivity'
              ? t('editor.metadata.camera.iso')
              : key === 'FocalLengthIn35mmFilm'
                ? t('editor.metadata.camera.focalLength')
                : '';

      return {
        key: key,
        label: translatedLabel,
        value:
          hasValue && KEY_CAMERA_SETTINGS_MAP[key].format
            ? KEY_CAMERA_SETTINGS_MAP[key].format!(value as number)
            : hasValue
              ? value
              : '-',
      };
    });

    const lensValue = exif['LensModel'];
    const hasLensValue = lensValue !== undefined && lensValue !== null && lensValue !== '';
    const lensSetting = {
      key: 'LensModel',
      label: t('editor.metadata.camera.lens'),
      value:
        hasLensValue && KEY_CAMERA_SETTINGS_MAP['LensModel'].format
          ? KEY_CAMERA_SETTINGS_MAP['LensModel'].format(lensValue as number)
          : hasLensValue
            ? lensValue
            : '-',
    };

    const handledKeys = [
      ...cameraGridKeys,
      'LensModel',
      'Make',
      'Model',
      'DateTimeOriginal',
      ...EDITABLE_FIELDS.map((f) => f.key),
    ];
    const otherExifEntries = Object.entries(exif)
      .filter(
        ([key, value]) =>
          !handledKeys.includes(key) &&
          !/^(MakerNote|ICCProfile|ThumbnailImage|PreviewImage|JpgFromRaw)$/i.test(key) &&
          !/^\s*(?:\[binary|binary data|0x[0-9a-f]{32})/i.test(String(value)),
      )
      .sort(([keyA], [keyB]) => keyA.localeCompare(keyB));

    return { cameraGridSettings, lensSetting, otherExifEntries };
  }, [selectedImage?.exif, t]);

  const currentColor = useMemo(() => {
    return tags.find((tag: string) => tag.startsWith('color:'))?.substring(6) || null;
  }, [tags]);

  const currentTags = useMemo(() => {
    return tags
      .filter((t) => !t.startsWith('color:'))
      .map((t) => ({
        tag: t.startsWith(USER_TAG_PREFIX) ? t.substring(USER_TAG_PREFIX.length) : t,
        isUser: t.startsWith(USER_TAG_PREFIX),
      }))
      .sort((a, b) => a.tag.localeCompare(b.tag));
  }, [tags]);

  const fullPath = selectedImage?.path || '';
  const isVirtualCopy = fullPath.includes('?vc=');
  const basePath = fullPath.split('?vc=')[0];
  const fileName = basePath.split(/[\\/]/).pop() || '';
  const fileExtension = fileName.split('.').pop()?.toUpperCase() || 'FILE';
  const megapixels =
    selectedImage?.width && selectedImage?.height
      ? ((selectedImage.width * selectedImage.height) / 1000000).toFixed(1)
      : null;

  const handleAddTag = async (tagToAdd: string) => {
    const newTagValue = tagToAdd.trim().toLowerCase();
    if (newTagValue && !currentTags.some((t) => t.tag === newTagValue)) {
      try {
        const prefixedTag = `${USER_TAG_PREFIX}${newTagValue}`;
        const pathsToUpdate = getPathsToUpdate();
        await invoke(Invokes.AddTagForPaths, { paths: pathsToUpdate, tag: prefixedTag });

        const newTags = [...currentTags, { tag: newTagValue, isUser: true }];
        handleTagsChanged(targetPaths, newTags);
        setTagInputValue('');
      } catch (err) {
        console.error(`Failed to add tag: ${err}`);
      }
    }
  };

  const handleRemoveTag = async (tagToRemove: { tag: string; isUser: boolean }) => {
    try {
      const prefixedTag = tagToRemove.isUser ? `${USER_TAG_PREFIX}${tagToRemove.tag}` : tagToRemove.tag;
      const pathsToUpdate = getPathsToUpdate();
      await invoke(Invokes.RemoveTagForPaths, { paths: pathsToUpdate, tag: prefixedTag });

      const newTags = currentTags.filter((t) => t.tag !== tagToRemove.tag);
      handleTagsChanged(targetPaths, newTags);
    } catch (err) {
      console.error(`Failed to remove tag: ${err}`);
    }
  };

  const handleTagInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleAddTag(tagInputValue);
    }
    e.stopPropagation();
  };

  const LensIcon = CAMERA_ICONS['LensModel'];

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('editor.metadata.title')}</Text>
      </div>
      <div className="grow overflow-y-auto p-3 custom-scrollbar">
        {selectedImage ? (
          <div className="flex flex-col gap-6">
            <div>
              <Text variant={TextVariants.heading} className="mb-3">
                {t('editor.metadata.fileInfo.title')}
              </Text>
              <div className="bg-surface border border-surface rounded-xl p-3.5 flex flex-col gap-2 cursor-default relative min-h-[5.5rem] overflow-hidden">
                {(liveThumbnailUrl || selectedImage?.thumbnailUrl) && (
                  <div
                    className="absolute inset-y-0 right-0 w-2/3 pointer-events-none opacity-20"
                    style={{
                      backgroundImage: `url(${liveThumbnailUrl || selectedImage.thumbnailUrl})`,
                      backgroundPosition: 'right center',
                      backgroundSize: 'cover',
                      filter: 'grayscale(100%)',
                      maskImage: 'linear-gradient(to right, transparent 5%, black 80%)',
                      WebkitMaskImage: 'linear-gradient(to right, transparent 5%, black 80%)',
                    }}
                  />
                )}

                <div className="flex justify-between items-start gap-4 relative z-10">
                  <Text weight={TextWeights.semibold} color={TextColors.primary} className="truncate drop-shadow-sm">
                    {fileName || '-'}
                  </Text>
                  <div className="flex items-center gap-1.5 shrink-0">
                    {isVirtualCopy && (
                      <div
                        className="bg-bg-primary/80 backdrop-blur-md text-text-secondary font-bold text-[10px] rounded-md px-2 py-1 tracking-wider uppercase shadow-sm border border-surface/50"
                        data-tooltip={t('editor.metadata.fileInfo.virtualCopy')}
                      >
                        {t('uiText.MetadataPanel.vc')}
                      </div>
                    )}
                    <div className="bg-bg-primary/80 backdrop-blur-md text-text-secondary font-bold text-[10px] rounded-md px-2 py-1 tracking-wider uppercase shadow-sm border border-surface/50">
                      {fileExtension}
                    </div>
                  </div>
                </div>

                <div className="flex flex-col gap-0.5 relative z-10">
                  <Text variant={TextVariants.small} color={TextColors.secondary} className="truncate drop-shadow-sm">
                    {selectedImage.width && selectedImage.height
                      ? t('editor.metadata.fileInfo.dimensions', {
                          width: selectedImage.width,
                          height: selectedImage.height,
                          megapixels,
                        })
                      : t('editor.metadata.fileInfo.emptyDimensions')}
                  </Text>
                  <Text variant={TextVariants.small} color={TextColors.secondary} className="truncate drop-shadow-sm">
                    {selectedImage.exif?.DateTimeOriginal || '-'}
                  </Text>
                </div>
              </div>
            </div>

            <div>
              <Text variant={TextVariants.heading} className="mb-3">
                {t('editor.metadata.camera.title')}
              </Text>
              <div className="flex flex-col gap-2">
                <MetadataItem
                  label={t('editor.metadata.camera.model')}
                  value={[selectedImage.exif?.Make, selectedImage.exif?.Model].filter(Boolean).join(' ') || '-'}
                />
                <div className="grid grid-cols-2 gap-2">
                  {cameraGridSettings.map((item: any) => {
                    const Icon = CAMERA_ICONS[item.key];
                    return (
                      <div
                        key={item.key}
                        className="flex items-center gap-2 bg-surface border border-surface px-3 py-2 rounded-xl cursor-default"
                        data-tooltip={item.label}
                      >
                        {Icon && (
                          <span className="text-text-secondary opacity-90 flex items-center justify-center shrink-0">
                            <Icon />
                          </span>
                        )}
                        <Text
                          as="span"
                          variant={TextVariants.small}
                          color={TextColors.primary}
                          weight={TextWeights.medium}
                          className="truncate"
                        >
                          {item.value}
                        </Text>
                      </div>
                    );
                  })}
                </div>

                <div
                  className="flex items-center gap-2 bg-surface border border-surface px-3 py-2 rounded-xl cursor-default"
                  data-tooltip={lensSetting.label}
                >
                  {LensIcon && (
                    <span className="text-text-secondary opacity-90 flex items-center justify-center shrink-0">
                      <LensIcon />
                    </span>
                  )}
                  <Text
                    as="span"
                    variant={TextVariants.small}
                    weight={TextWeights.medium}
                    color={TextColors.primary}
                    className="truncate"
                  >
                    {lensSetting.value}
                  </Text>
                </div>
              </div>
            </div>

            <div>
              <Text variant={TextVariants.heading} className="mb-3">
                {t('editor.metadata.author.title')}
              </Text>
              <div className="bg-surface rounded-xl overflow-hidden">
                <button
                  onClick={() => setIsAuthorExpanded(!isAuthorExpanded)}
                  className="w-full flex items-center justify-between p-3 hover:bg-card-active transition-colors"
                >
                  <Text
                    as="span"
                    variant={TextVariants.label}
                    color={TextColors.primary}
                    className="flex items-center gap-2"
                  >
                    <User size={16} /> {t('editor.metadata.author.creatorDetails')}
                  </Text>
                  <Text color={TextColors.secondary}>
                    {isAuthorExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                  </Text>
                </button>

                <AnimatePresence initial={false}>
                  {isAuthorExpanded && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
                      className="overflow-hidden"
                    >
                      <div className="px-2 pb-3 pt-2 border-t border-surface/50 flex flex-col gap-0.5">
                        {EDITABLE_FIELDS.map((field) => {
                          const rawValue = (selectedImage?.exif?.[field.key] as string) || '';
                          const cleanValue = rawValue.replace(/^"|"$/g, '').trim();
                          const displayValue = cleanValue.toLowerCase() === 'default' ? '' : cleanValue;
                          return (
                            <EditableMetadataItem
                              key={field.key}
                              label={String(t(`editor.metadata.fields.${field.label}` as any))}
                              value={displayValue}
                              onSave={(newVal) => {
                                handleUpdateExif(targetPaths, { [field.key]: newVal });
                              }}
                            />
                          );
                        })}
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </div>

            <div>
              <Text variant={TextVariants.heading} className="mb-3">
                {t('editor.metadata.organization.title')}
              </Text>
              <div className="bg-surface rounded-xl overflow-hidden">
                <button
                  onClick={() => setIsOrganizationExpanded(!isOrganizationExpanded)}
                  className="w-full flex items-center justify-between p-3 hover:bg-card-active transition-colors"
                >
                  <Text
                    as="span"
                    variant={TextVariants.label}
                    color={TextColors.primary}
                    className="flex items-center gap-2"
                  >
                    <Tag size={16} /> {t('editor.metadata.organization.ratingLabels')}
                  </Text>
                  <Text color={TextColors.secondary}>
                    {isOrganizationExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                  </Text>
                </button>

                <AnimatePresence initial={false}>
                  {isOrganizationExpanded && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
                      className="overflow-hidden"
                    >
                      <div className="px-4 pb-4 pt-2 border-t border-surface/50 flex flex-col gap-4">
                        <div>
                          <Text
                            variant={TextVariants.small}
                            color={TextColors.primary}
                            weight={TextWeights.semibold}
                            className="uppercase tracking-wider mb-2 block"
                          >
                            {t('editor.metadata.organization.rating')}
                          </Text>
                          <div className="flex items-center gap-2">
                            {[1, 2, 3, 4, 5].map((star) => (
                              <button
                                key={star}
                                onClick={() => handleRate(star, targetPaths)}
                                className="focus:outline-hidden transition-transform active:scale-95 hover:scale-110"
                              >
                                <Star
                                  size={20}
                                  className={clsx(
                                    'transition-colors duration-200',
                                    star <= rating
                                      ? 'fill-accent text-accent'
                                      : 'fill-transparent text-text-secondary hover:text-text-primary',
                                  )}
                                />
                              </button>
                            ))}
                          </div>
                        </div>
                        <div>
                          <Text
                            variant={TextVariants.small}
                            color={TextColors.primary}
                            weight={TextWeights.semibold}
                            className="uppercase tracking-wider mb-2 block"
                          >
                            {t('editor.metadata.organization.colorLabel')}
                          </Text>
                          <div className="flex flex-wrap gap-2">
                            <button
                              onClick={() => handleSetColorLabel(null, targetPaths)}
                              className={clsx(
                                'w-5 h-5 rounded-full flex items-center justify-center transition-all hover:scale-110',
                                currentColor === null
                                  ? 'ring-2 ring-text-secondary ring-offset-1 ring-offset-bg-primary'
                                  : 'opacity-50 hover:opacity-100 hover:ring-2 hover:ring-text-secondary/20',
                              )}
                              data-tooltip={t('editor.metadata.organization.none')}
                            >
                              <X size={12} className="text-text-tertiary" />
                            </button>
                            {COLOR_LABELS.map((color: Color) => (
                              <button
                                key={color.name}
                                onClick={() => handleSetColorLabel(color.name, targetPaths)}
                                className={clsx(
                                  'w-5 h-5 rounded-full transition-all hover:scale-110',
                                  currentColor === color.name
                                    ? 'ring-2 ring-white ring-offset-1 ring-offset-bg-primary'
                                    : 'hover:ring-2 hover:ring-white/20',
                                )}
                                style={{ backgroundColor: color.color }}
                                data-tooltip={color.name}
                              >
                                {currentColor === color.name && <Check size={12} className="text-black/50 mx-auto" />}
                              </button>
                            ))}
                          </div>
                        </div>
                        <div>
                          <Text
                            variant={TextVariants.small}
                            color={TextColors.primary}
                            weight={TextWeights.semibold}
                            className="uppercase tracking-wider mb-2 block"
                          >
                            {t('editor.metadata.organization.tags')}
                          </Text>
                          <div className="flex flex-wrap gap-1 mb-2">
                            <AnimatePresence>
                              {currentTags.length > 0 ? (
                                currentTags.map((tagItem) => (
                                  <motion.div
                                    key={tagItem.tag}
                                    layout
                                    initial={{ opacity: 0, scale: 0.8 }}
                                    animate={{ opacity: 1, scale: 1 }}
                                    exit={{ opacity: 0, scale: 0.8 }}
                                    className="flex items-center gap-1 bg-bg-primary px-2 py-1 rounded-md group cursor-pointer border border-surface hover:border-text-tertiary/50 transition-colors"
                                    onClick={() => handleRemoveTag(tagItem)}
                                  >
                                    <Text
                                      as="span"
                                      variant={TextVariants.small}
                                      color={TextColors.primary}
                                      weight={TextWeights.medium}
                                    >
                                      {tagItem.tag}
                                    </Text>
                                    <X size={10} className="opacity-50 group-hover:opacity-100" />
                                  </motion.div>
                                ))
                              ) : (
                                <Text variant={TextVariants.small} className="italic text-text-secondary">
                                  {t('editor.metadata.organization.noTags')}
                                </Text>
                              )}
                            </AnimatePresence>
                          </div>

                          <div
                            className={clsx(
                              'flex items-center bg-bg-primary border rounded-md px-2 py-1.5 transition-colors',
                              isTagInputFocused ? 'border-accent' : 'border-surface',
                            )}
                          >
                            <input
                              type="text"
                              value={tagInputValue}
                              onChange={(e) => setTagInputValue(e.target.value)}
                              onKeyDown={handleTagInputKeyDown}
                              onFocus={() => setIsTagInputFocused(true)}
                              onBlur={() => setIsTagInputFocused(false)}
                              placeholder={t('editor.metadata.organization.addTagPlaceholder')}
                              className="bg-transparent border-none outline-hidden text-xs w-full text-text-primary placeholder-text-tertiary"
                            />
                            <button
                              onClick={() => handleAddTag(tagInputValue)}
                              disabled={!tagInputValue.trim()}
                              className="text-text-secondary hover:text-accent disabled:opacity-30 transition-colors"
                            >
                              <Plus size={14} />
                            </button>
                          </div>
                          {appSettings?.taggingShortcuts && appSettings.taggingShortcuts.length > 0 && (
                            <div className="mt-2 flex flex-wrap gap-1">
                              {appSettings.taggingShortcuts.map((shortcut) => (
                                <button
                                  key={shortcut}
                                  onClick={() => handleAddTag(shortcut)}
                                  className="text-xs font-medium bg-bg-secondary hover:bg-card-active text-text-secondary px-1.5 py-0.5 rounded-sm border border-transparent hover:border-border-color transition-all"
                                >
                                  {shortcut}
                                </button>
                              ))}
                            </div>
                          )}
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </div>

            {otherExifEntries.length > 0 && (
              <details key={selectedImage.path} className="bg-surface rounded-xl">
                <summary className="cursor-pointer p-3 text-text-primary font-medium">
                  {t('editor.metadata.extendedExif.title')}
                </summary>
                <div className="max-h-80 overflow-y-auto custom-scrollbar p-3 flex flex-col gap-0.5">
                  {otherExifEntries.map(([tag, value]) => (
                    <MetadataItem key={tag} label={formatExifTag(tag)} value={value} />
                  ))}
                </div>
              </details>
            )}
          </div>
        ) : (
          <div className="flex items-center justify-center h-full">
            <Text
              variant={TextVariants.heading}
              color={TextColors.secondary}
              weight={TextWeights.normal}
              className="text-center"
            >
              {t('editor.noImageSelected')}
            </Text>
          </div>
        )}
      </div>
    </div>
  );
}
