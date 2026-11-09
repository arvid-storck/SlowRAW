import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useTranslation } from 'react-i18next';
import '../i18n';

export default function ViewerWindow() {
  const { t } = useTranslation();
  const [image, setImage] = useState<[number, number] | null>(null);
  const [error, setError] = useState('');
  const [viewport, setViewport] = useState({ width: innerWidth, height: innerHeight });
  const [view, setView] = useState({ zoom: 1, x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; px: number; py: number } | null>(null);
  const fit = useCallback(() => setView({ zoom: 1, x: 0, y: 0 }), []);
  const fullscreen = useCallback(async () => {
    const window = getCurrentWindow();
    await window.setFullscreen(!(await window.isFullscreen()));
  }, []);
  useEffect(() => {
    let active = true;
    let busy = false;
    const refresh = async () => {
      if (busy) return;
      busy = true;
      try {
        const frame = await invoke<[number, number] | null>('viewer_frame');
        if (active) {
          setImage((old) => (old?.[0] === frame?.[0] && old?.[1] === frame?.[1] ? old : frame));
          setError('');
        }
      } catch (error) {
        if (active) setError(String(error));
      } finally {
        busy = false;
      }
    };
    void refresh();
    const currentWindow = getCurrentWindow();
    const subscriptions = [
      currentWindow.listen<[number, number]>('viewer-frame', ({ payload }) => {
        if (active) {
          setImage((old) => (old?.[0] === payload[0] && old?.[1] === payload[1] ? old : payload));
          setError('');
        }
      }),
      currentWindow.listen<string>('viewer-error', ({ payload }) => {
        if (active) setError(payload);
      }),
      currentWindow.listen<string>('wgpu-display-unavailable', ({ payload }) => {
        if (active) setError(payload);
      }),
    ];
    const resize = () => setViewport({ width: innerWidth, height: innerHeight });
    const key = (event: KeyboardEvent) => {
      if (event.code === 'KeyF') {
        event.preventDefault();
        void fullscreen();
      }
      if (event.code === 'Digit0') fit();
      if (event.code === 'Escape') void getCurrentWindow().setFullscreen(false);
    };
    window.addEventListener('resize', resize);
    window.addEventListener('keydown', key);
    return () => {
      active = false;
      void Promise.all(subscriptions).then((stops) => stops.forEach((stop) => stop()));
      window.removeEventListener('resize', resize);
      window.removeEventListener('keydown', key);
    };
  }, [fit, fullscreen]);
  useEffect(() => {
    if (!image) return;
    const ratio = Math.min(viewport.width / image[0], viewport.height / image[1]);
    const width = image[0] * ratio * view.zoom;
    const height = image[1] * ratio * view.zoom;
    const dpr = devicePixelRatio || 1;
    let active = true;
    const frame = requestAnimationFrame(() => {
      if (!active) return;
      void invoke('update_wgpu_transform', {
        payload: {
          x: ((viewport.width - width) / 2 + view.x) * dpr,
          y: ((viewport.height - height) / 2 + view.y) * dpr,
          width: width * dpr,
          height: height * dpr,
          clipX: 0,
          clipY: 0,
          clipWidth: viewport.width * dpr,
          clipHeight: viewport.height * dpr,
          windowWidth: viewport.width * dpr,
          windowHeight: viewport.height * dpr,
          bgPrimary: [0.025, 0.025, 0.025, 1],
          bgSecondary: [0.025, 0.025, 0.025, 1],
          pixelated: false,
        },
      }).catch((error) => {
        if (active) setError(String(error));
      });
    });
    const demand = setTimeout(
      () => void invoke('viewer_resolution', { resolution: Math.ceil(Math.max(width, height) * dpr) }),
      100,
    );
    return () => {
      active = false;
      cancelAnimationFrame(frame);
      clearTimeout(demand);
    };
  }, [image, viewport, view]);
  return (
    <div
      className="h-screen w-screen select-none overflow-hidden"
      style={{ cursor: drag.current ? 'grabbing' : 'grab' }}
      onDoubleClick={() => void fullscreen()}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, y: event.clientY, px: view.x, py: view.y };
      }}
      onPointerMove={(event) => {
        const start = drag.current;
        if (start)
          setView((old) => ({ ...old, x: start.px + event.clientX - start.x, y: start.py + event.clientY - start.y }));
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onWheel={(event) => {
        event.preventDefault();
        const factor = Math.exp(-Math.max(-100, Math.min(100, event.deltaY)) * 0.003);
        setView((old) => {
          const zoom = Math.max(1, Math.min(32, old.zoom * factor));
          const scale = zoom / old.zoom;
          const x = event.clientX - viewport.width / 2;
          const y = event.clientY - viewport.height / 2;
          return { zoom, x: zoom === 1 ? 0 : x - (x - old.x) * scale, y: zoom === 1 ? 0 : y - (y - old.y) * scale };
        });
      }}
    >
      {(!image || error) && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-white bg-black">
          {error || t('viewer.empty')}
        </div>
      )}
      <div
        className="absolute bottom-4 left-1/2 flex -translate-x-1/2 gap-3 bg-black/70 px-4 py-2 text-sm text-white"
        onPointerDown={(event) => event.stopPropagation()}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <button onClick={fit}>{t('viewer.fit')}</button>
        <span>{Math.round(view.zoom * 100)}%</span>
        <button onClick={() => void fullscreen()}>{t('viewer.fullscreen')}</button>
      </div>
    </div>
  );
}
