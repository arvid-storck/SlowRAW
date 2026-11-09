import { ListTodo, X, FolderOpen } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';
import { cancelJob, clearFinishedJobs, useJobStore } from '../../store/useJobStore';
import { usePopover } from '../../hooks/usePopover';

export default function JobQueue() {
  const { t } = useTranslation();
  const { open, setOpen, container } = usePopover();
  const jobs = useJobStore((state) => state.jobs);
  const active = jobs.filter((job) => job.status === 'running' || job.status === 'queued').length;
  return (
    <div ref={container} className="relative">
      <button
        className="flex items-center gap-1 p-2 text-text-secondary hover:text-text-primary"
        onClick={() => setOpen(!open)}
        aria-label={t('jobs.title')}
        aria-expanded={open}
      >
        <ListTodo size={18} />
        {active > 0 && <span className="text-xs">{active}</span>}
      </button>
      {open && (
        <div className="absolute right-0 bottom-full mb-2 w-80 max-h-96 overflow-y-auto bg-bg-secondary border border-surface shadow-lg p-3 z-50">
          <div className="flex justify-between items-center mb-3">
            <span className="font-semibold">{t('jobs.title')}</span>
            <button className="text-xs text-text-secondary" onClick={clearFinishedJobs}>
              {t('jobs.clearFinished')}
            </button>
          </div>
          {jobs.length === 0 && <p className="text-sm text-text-secondary">{t('jobs.empty')}</p>}
          {jobs.map((job) => (
            <div key={job.id} className="py-2 border-t border-surface text-sm">
              <div className="flex items-center justify-between gap-2">
                <span>{t(`jobs.kinds.${job.kind}`)}</span>
                <span className="text-xs text-text-secondary">{t(`jobs.status.${job.status}`)}</span>
                {job.cancellable && (
                  <button
                    aria-label={t('jobs.cancel')}
                    onClick={() => cancelJob(job.id).catch((error) => toast.error(String(error)))}
                  >
                    <X size={14} />
                  </button>
                )}
                {job.status === 'completed' && job.path && (
                  <button
                    aria-label={t('jobs.showFiles')}
                    onClick={() =>
                      invoke('show_in_finder', { path: job.path }).catch((error) => toast.error(String(error)))
                    }
                  >
                    <FolderOpen size={14} />
                  </button>
                )}
              </div>
              {job.progress && (
                <progress
                  className="w-full accent-accent"
                  max={Math.max(1, job.progress.total)}
                  value={job.progress.current}
                />
              )}
              {job.error && <p className="text-red-400 break-words">{job.error}</p>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
