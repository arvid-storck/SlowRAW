import { CalendarDays, ChevronDown, ChevronRight, Sparkles, Folder, Images } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import clsx from 'clsx';
import { useState, useMemo } from 'react';
import Text from '../../ui/Text';
import { TextVariants, TextWeights } from '../../../types/typography';
import { useLibraryStore } from '../../../store/useLibraryStore';
import { AlbumItem } from '../../ui/AppProperties';

interface CatalogPanelProps {
  isResizing: boolean;
  onSelectAlbum(albumId: string, albumName: string, images: string[], preserveEditor?: boolean, filter?: any): void;
  onAlbumContextMenu(event: any, item: AlbumItem | null): void;
  style: any;
}

export default function CatalogPanel({ isResizing, onSelectAlbum, onAlbumContextMenu, style }: CatalogPanelProps) {
  const { t } = useTranslation();
  const { albumTree, activeAlbumId, expandedAlbumGroups } = useLibraryStore();
  const { catalogFacets, catalogDateFilter, setLibrary } = useLibraryStore();
  const [expandedYears, setExpandedYears] = useState<Set<string>>(new Set());
  const [expandedMonths, setExpandedMonths] = useState<Set<string>>(new Set());

  const years = useMemo(() => {
    const grouped: Record<
      string,
      {
        count: number;
        months: Record<string, { count: number; days: Array<{ key: string; day: string; count: number }> }>;
      }
    > = {};
    for (const entry of catalogFacets?.dateDays ?? []) {
      const [year, month, day] = entry.date.split('-');
      if (!year || !month || !day) continue;
      grouped[year] ??= { count: 0, months: {} };
      grouped[year].count += entry.count;
      grouped[year].months[month] ??= { count: 0, days: [] };
      grouped[year].months[month].count += entry.count;
      grouped[year].months[month].days.push({ key: entry.date, day, count: entry.count });
    }
    return Object.entries(grouped)
      .sort(([a], [b]) => b.localeCompare(a))
      .map(([year, value]) => ({
        year,
        count: value.count,
        months: Object.entries(value.months)
          .sort(([a], [b]) => b.localeCompare(a))
          .map(([month, monthValue]) => ({ month, ...monthValue })),
      }));
  }, [catalogFacets?.dateDays]);

  const toggle = (setter: React.Dispatch<React.SetStateAction<Set<string>>>, key: string) =>
    setter((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const selectDate = (date: string) =>
    setLibrary((state) => ({
      catalogDateFilter: date,
      catalogPage: 0,
      isCatalogMode: true,
      catalogFilter: { ...state.catalogFilter, date },
      activeAlbumId: null,
    }));
  const rowClass = (selected: boolean) =>
    clsx(
      'w-full flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors',
      selected
        ? 'bg-card-active text-text-primary'
        : 'text-text-secondary hover:bg-card-active hover:text-text-primary',
    );

  const renderAlbums = (items: AlbumItem[]): React.ReactNode =>
    items.map((item) => (
      <div
        key={item.id}
        onContextMenu={(event) => {
          event.stopPropagation();
          onAlbumContextMenu(event, item);
        }}
      >
        <button
          className={rowClass(activeAlbumId === item.id)}
          onClick={() => {
            if (item.type === 'group') {
              setLibrary((state) => {
                const expanded = new Set(state.expandedAlbumGroups);
                if (expanded.has(item.id)) expanded.delete(item.id);
                else expanded.add(item.id);
                return { expandedAlbumGroups: expanded };
              });
            } else {
              setLibrary({ editingSmartGroupId: null });
              onSelectAlbum(
                item.id,
                item.name,
                item.type === 'album' ? item.images : [],
                false,
                item.type === 'smartGroup' ? item.filter : undefined,
              );
            }
          }}
          aria-expanded={item.type === 'group' ? expandedAlbumGroups.has(item.id) : undefined}
        >
          {item.type === 'group' ? (
            expandedAlbumGroups.has(item.id) ? (
              <ChevronDown size={16} />
            ) : (
              <ChevronRight size={16} />
            )
          ) : item.type === 'smartGroup' ? (
            <Sparkles size={16} />
          ) : (
            <Images size={16} />
          )}
          {item.type === 'group' && <Folder size={16} />}
          <span className="grow truncate">{item.name}</span>
        </button>
        {item.type === 'group' && expandedAlbumGroups.has(item.id) && (
          <div className="ml-4">{renderAlbums(item.children)}</div>
        )}
      </div>
    ));

  return (
    <div
      className={clsx(
        'relative bg-bg-secondary rounded-lg shrink-0 flex flex-col h-full',
        !isResizing && 'transition-[width] duration-300 ease-in-out',
      )}
      style={style}
    >
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('catalog.dates')}</Text>
      </div>
      <div className="flex-1 overflow-y-auto p-3">
        <button className={rowClass(catalogDateFilter === '')} onClick={() => selectDate('')}>
          <CalendarDays size={16} />
          <span className="grow">{t('catalog.allDates')}</span>
          <span className="text-xs opacity-70">
            {catalogFacets?.dateDays.reduce((sum, item) => sum + item.count, 0) ?? 0}
          </span>
        </button>
        {years.map(({ year, count, months }) => {
          const yearOpen = expandedYears.has(year);
          return (
            <div key={year}>
              <div className="flex items-center">
                <button className="p-1 text-text-secondary" onClick={() => toggle(setExpandedYears, year)}>
                  {yearOpen ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
                </button>
                <button className={rowClass(catalogDateFilter === year)} onClick={() => selectDate(year)}>
                  <span className="grow font-medium">{year}</span>
                  <span className="text-xs opacity-70">{count}</span>
                </button>
              </div>
              {yearOpen && (
                <div className="ml-4">
                  {months.map(({ month, count: monthCount, days }) => {
                    const monthKey = `${year}-${month}`;
                    const monthOpen = expandedMonths.has(monthKey);
                    const monthName = new Intl.DateTimeFormat(undefined, { month: 'long' }).format(
                      new Date(Date.UTC(Number(year), Number(month) - 1, 1)),
                    );
                    return (
                      <div key={monthKey}>
                        <div className="flex items-center">
                          <button
                            className="p-1 text-text-secondary"
                            onClick={() => toggle(setExpandedMonths, monthKey)}
                          >
                            {monthOpen ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
                          </button>
                          <button
                            className={rowClass(catalogDateFilter === monthKey)}
                            onClick={() => selectDate(monthKey)}
                          >
                            <span className="grow capitalize">{monthName}</span>
                            <span className="text-xs opacity-70">{monthCount}</span>
                          </button>
                        </div>
                        {monthOpen && (
                          <div className="ml-7">
                            {days.map((entry) => (
                              <button
                                key={entry.key}
                                className={rowClass(catalogDateFilter === entry.key)}
                                onClick={() => selectDate(entry.key)}
                              >
                                <span className="grow">{entry.day}</span>
                                <span className="text-xs opacity-70">{entry.count}</span>
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
        {years.length === 0 && <Text className="p-3 text-center">{t('catalog.noDates')}</Text>}
        {
          <div
            className="mt-3 border-t border-surface pt-3"
            onContextMenu={(event) => {
              event.preventDefault();
              onAlbumContextMenu(event, null);
            }}
          >
            <Text variant={TextVariants.small} weight={TextWeights.bold} className="px-2 pb-1 uppercase tracking-wider">
              {t('catalog.albums')}
            </Text>
            <button
              className="px-2 py-1 text-sm text-text-secondary hover:text-text-primary"
              onClick={(event) => onAlbumContextMenu(event, null)}
            >
              {t('catalog.newAlbumGroup')}
            </button>
            {renderAlbums(albumTree)}
          </div>
        }
      </div>
    </div>
  );
}
