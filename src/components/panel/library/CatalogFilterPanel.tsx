import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Trash2, Sparkles } from 'lucide-react';
import { v4 as uuidv4 } from 'uuid';
import { toast } from 'react-toastify';
import { AlbumItem, Invokes } from '../../ui/AppProperties';
import {
  CatalogFilterState,
  CatalogStatus,
  EMPTY_CATALOG_FILTER,
  useLibraryStore,
} from '../../../store/useLibraryStore';
import { useSettingsStore } from '../../../store/useSettingsStore';
import { refreshCatalog, updateSmartGroup } from '../../../utils/catalog';
import Button from '../../ui/Button';
import Input from '../../ui/Input';
import Text from '../../ui/Text';
import { TextVariants } from '../../../types/typography';

function Choices({
  label,
  selected,
  values,
  onChange,
}: {
  label: string;
  selected: string[];
  values: string[];
  onChange: (values: string[]) => void;
}) {
  const { t } = useTranslation();
  return (
    <fieldset className="space-y-1">
      <legend className="text-xs text-text-secondary">{label}</legend>
      <div className="max-h-32 overflow-y-auto rounded-md bg-bg-primary border border-border-color p-2 space-y-1">
        <label className="flex items-center gap-2 text-sm cursor-pointer">
          <input type="checkbox" checked={!selected.length} onChange={() => onChange([])} /> {t('catalog.all')}
        </label>
        {values.map((value) => (
          <label key={value} className="flex items-center gap-2 text-sm cursor-pointer">
            <input
              type="checkbox"
              checked={selected.includes(value)}
              onChange={() =>
                onChange(selected.includes(value) ? selected.filter((v) => v !== value) : [...selected, value])
              }
            />
            <span className="truncate" title={value}>
              {value}
            </span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

export default function CatalogFilterPanel() {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [smartGroupName, setSmartGroupName] = useState('');
  const {
    editingSmartGroupId,
    albumTree,
    catalogStatus,
    catalogFacets,
    catalogFilter: filter,
    catalogDateFilter,
    setLibrary,
  } = useLibraryStore();

  const setFilter = (next: CatalogFilterState) =>
    setLibrary({
      catalogFilter: next,
      catalogPage: 0,
      activeAlbumId: null,
      isCatalogMode: true,
    });
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    if (!editingSmartGroupId) {
      setSmartGroupName('');
      return;
    }
    const find = (items: AlbumItem[]): AlbumItem | undefined => {
      for (const item of items) {
        if (item.id === editingSmartGroupId) return item;
        if (item.type === 'group') {
          const found = find(item.children);
          if (found) return found;
        }
      }
    };
    setSmartGroupName(find(albumTree)?.name ?? '');
  }, [editingSmartGroupId, albumTree]);

  const saveSmartGroup = async () => {
    const name = smartGroupName.trim();
    if (!name) return;
    const tree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
    const criteria = { ...filter, date: catalogDateFilter, minimumRating: filter.minimumRating ?? undefined };
    let savedId = editingSmartGroupId;
    if (savedId) {
      if (!updateSmartGroup(tree, savedId, name, criteria)) throw new Error(t('catalog.groupNotFound'));
    } else {
      savedId = uuidv4();
      tree.push({ type: 'smartGroup', id: savedId, name, filter: criteria });
    }
    await invoke(Invokes.SaveAlbums, { tree });
    const albumTree = await invoke<AlbumItem[]>(Invokes.GetAlbums);
    setLibrary((state) => ({
      albumTree,
      ...(state.editingSmartGroupId === editingSmartGroupId && state.catalogFilter === filter
        ? { editingSmartGroupId: null, activeAlbumId: savedId }
        : {}),
    }));
    setSmartGroupName('');
    toast.success(t('catalog.saved', { name }));
  };

  return (
    <aside className="w-64 shrink-0 min-h-0 h-full flex flex-col overflow-hidden rounded-lg border border-surface bg-bg-secondary shadow-xs">
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('catalog.filters')}</Text>
      </div>
      <div className="grow overflow-y-auto p-3 space-y-3">
        <Input
          value={filter.text}
          placeholder={t('catalog.search')}
          onChange={(event) => setFilter({ ...filter, text: event.target.value })}
        />
        <p className="text-xs text-text-secondary">{t('catalog.match')}</p>
        <label className="block text-xs text-text-secondary">
          {t('catalog.flags')}
          <select
            className="block w-full bg-bg-primary text-text-primary rounded p-2 mt-1"
            value={filter.flag ?? ''}
            onChange={(event) => setFilter({ ...filter, flag: event.target.value })}
          >
            <option value="">{t('catalog.allFlags')}</option>
            <option value="pick">{t('catalog.picked')}</option>
            <option value="reject">{t('catalog.rejected')}</option>
            <option value="unflagged">{t('catalog.unflagged')}</option>
          </select>
        </label>
        <Choices
          label={t('catalog.cameras')}
          selected={filter.cameras}
          values={catalogFacets?.cameras ?? []}
          onChange={(cameras) => setFilter({ ...filter, cameras, camera: '' })}
        />
        <Choices
          label={t('catalog.lenses')}
          selected={filter.lenses}
          values={catalogFacets?.lenses ?? []}
          onChange={(lenses) => setFilter({ ...filter, lenses, lens: '' })}
        />
        <Choices
          label={t('catalog.ratings')}
          selected={filter.ratings.map(String)}
          values={['0', '1', '2', '3', '4', '5']}
          onChange={(ratings) => setFilter({ ...filter, ratings: ratings.map(Number), minimumRating: null })}
        />
        <Choices
          label={t('catalog.tags')}
          selected={filter.tags}
          values={catalogFacets?.tags ?? []}
          onChange={(tags) => setFilter({ ...filter, tags, tag: '' })}
        />
        <Choices
          label={t('catalog.colors')}
          selected={filter.colors}
          values={catalogFacets?.colors ?? []}
          onChange={(colors) => setFilter({ ...filter, colors, color: '' })}
        />
        <Choices
          label={t('catalog.fileTypes')}
          selected={filter.fileTypes}
          values={catalogFacets?.fileTypes ?? []}
          onChange={(fileTypes) => setFilter({ ...filter, fileTypes, fileType: '' })}
        />
        <Button
          className="w-full"
          onClick={() => {
            setFilter({ ...EMPTY_CATALOG_FILTER });
            setLibrary({ catalogDateFilter: '' });
          }}
        >
          {t('catalog.clear')}
        </Button>
        <div className="flex gap-1">
          <Input
            value={smartGroupName}
            onChange={(event) => setSmartGroupName(event.target.value)}
            placeholder={t('catalog.smartName')}
          />
          <Button
            disabled={busy || !smartGroupName.trim()}
            onClick={() => run(saveSmartGroup)}
            data-tooltip={t(editingSmartGroupId ? 'catalog.saveChanges' : 'catalog.save')}
          >
            <Sparkles size={16} />
          </Button>
        </div>
        {editingSmartGroupId && (
          <Button onClick={() => setLibrary({ editingSmartGroupId: null })}>{t('catalog.cancelEdit')}</Button>
        )}
        <details className="border-t border-border-color pt-3">
          <summary className="cursor-pointer text-sm">{t('catalog.folders')}</summary>
          <div className="space-y-2 pt-2">
            {(catalogStatus?.folders ?? []).map((path) => (
              <div key={path} className="flex items-center gap-2 text-xs">
                <span className="truncate grow" title={path}>
                  {path}
                </span>
                <button
                  disabled={busy}
                  title={t('catalog.removeFolder')}
                  onClick={() =>
                    run(async () => {
                      const status = await invoke<CatalogStatus>(Invokes.CatalogRemoveFolder, { path });
                      const settings = useSettingsStore.getState();
                      if (settings.appSettings)
                        settings.setAppSettings({ ...settings.appSettings, catalogFolders: status.folders });
                      setLibrary({ catalogPage: 0 });
                      await refreshCatalog();
                    })
                  }
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))}
            <Button
              disabled={busy}
              onClick={() =>
                run(async () => {
                  for (const path of catalogStatus?.folders ?? []) {
                    await invoke(Invokes.CatalogAddFolder, { path });
                  }
                  setLibrary({ catalogPage: 0 });
                  await refreshCatalog();
                })
              }
            >
              {t('catalog.rescan')}
            </Button>
          </div>
        </details>
      </div>
    </aside>
  );
}
