import { create } from 'zustand';

export type JobStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
export type JobKind =
  'export' | 'import' | 'copy' | 'move' | 'auto' | 'catalog' | 'denoise' | 'panorama' | 'hdr' | 'focus' | 'cleanup';
export interface Job {
  id: number;
  kind: JobKind;
  status: JobStatus;
  progress?: { current: number; total: number };
  error?: string;
  path?: string;
  cancellable: boolean;
}

export const useJobStore = create<{ jobs: Job[] }>(() => ({ jobs: [] }));
type Task = {
  id: number;
  run: (id: number) => Promise<unknown>;
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
  cancel?: () => Promise<void>;
};
const pending: Task[] = [];
let active: Task | undefined;
let sequence = 0;

export function updateJob(id: number, update: Partial<Job>) {
  useJobStore.setState(({ jobs }) => ({ jobs: jobs.map((job) => (job.id === id ? { ...job, ...update } : job)) }));
}

async function drain() {
  if (active) return;
  const task = pending.shift();
  if (!task) return;
  active = task;
  updateJob(task.id, { status: 'running', cancellable: !!task.cancel });
  try {
    const result = await task.run(task.id);
    const status = useJobStore.getState().jobs.find((job) => job.id === task.id)?.status;
    if (status !== 'cancelled') updateJob(task.id, { status: 'completed', cancellable: false });
    task.resolve(result);
  } catch (error) {
    updateJob(task.id, { status: 'failed', error: String(error), cancellable: false });
    task.reject(error);
  } finally {
    active = undefined;
    const jobs = useJobStore.getState().jobs;
    const history = jobs.filter((job) => job.status !== 'queued' && job.status !== 'running').slice(-50);
    useJobStore.setState({
      jobs: jobs.filter((job) => job.status === 'queued' || job.status === 'running' || history.includes(job)),
    });
    void drain();
  }
}

export function enqueueJob<T>(
  kind: JobKind,
  run: (id: number) => Promise<T>,
  path?: string,
  cancel?: () => Promise<void>,
): Promise<T> {
  const id = ++sequence;
  useJobStore.setState(({ jobs }) => ({ jobs: [...jobs, { id, kind, status: 'queued', path, cancellable: true }] }));
  return new Promise<T>((resolve, reject) => {
    pending.push({ id, run, resolve: (value) => resolve(value as T), reject, cancel });
    void drain();
  });
}

export async function cancelJob(id: number) {
  const index = pending.findIndex((task) => task.id === id);
  if (index >= 0) {
    const [task] = pending.splice(index, 1);
    updateJob(id, { status: 'cancelled', cancellable: false });
    task.reject(new Error('Job cancelled'));
  } else if (active?.id === id && active.cancel) {
    await active.cancel();
    updateJob(id, { cancellable: false });
  }
}

export function clearFinishedJobs() {
  useJobStore.setState(({ jobs }) => ({
    jobs: jobs.filter((job) => job.status === 'queued' || job.status === 'running'),
  }));
}
