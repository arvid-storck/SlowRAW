import { useEffect, useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';
import { useSettingsStore } from '../store/useSettingsStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { initializeCatalog, loadCatalogWindow, resetCatalogWindow } from '../utils/catalog';

// Loading belongs to the application, not a filter panel that can be hidden.
export function useCatalog() {
  const { t } = useTranslation();
  const appSettings = useSettingsStore((state) => state.appSettings);
  const initialized = useRef(false);
  const { isCatalogMode, ready, filter, date, album, revision, sort } = useLibraryStore(
    useShallow((state) => ({
      isCatalogMode: state.isCatalogMode,
      ready: state.catalogStatus !== null,
      filter: state.catalogFilter,
      date: state.catalogDateFilter,
      album: state.activeAlbumId,
      revision: state.catalogRevision,
      sort: state.sortCriteria,
    })),
  );
  useEffect(() => {
    if (!appSettings || initialized.current) return;
    initialized.current = true;
    initializeCatalog().catch((error) => {
      initialized.current = false;
      toast.error(t('catalog.loadError', { error: String(error) }));
    });
  }, [appSettings, t]);
  useEffect(() => {
    if (isCatalogMode && ready) resetCatalogWindow();
  }, [isCatalogMode, ready, filter, date, album, sort]);
  useEffect(() => {
    if (!isCatalogMode || !ready) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      loadCatalogWindow(undefined, true).catch((error) => {
        if (!cancelled) toast.error(t('catalog.loadError', { error: String(error) }));
      });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [isCatalogMode, ready, filter, date, album, revision, sort, t]);
}
