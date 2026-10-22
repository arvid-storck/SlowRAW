import React, { useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import debounce from 'lodash.debounce';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { Adjustments, COPYABLE_ADJUSTMENT_KEYS } from '../utils/adjustments';
import { Invokes, Panel } from '../components/ui/AppProperties';
import { debouncedSave } from './useEditorActions';
import { globalImageCache } from '../utils/ImageLRUCache';

export function useImageProcessing(
  transformWrapperRef: any,
  prevAdjustmentsRef: React.RefObject<any>,
  renderRefs: {
    previewJobIdRef: React.RefObject<number>;
    latestRenderedJobIdRef: React.RefObject<number>;
    currentResRef: React.RefObject<number>;
  },
) {
  const { previewJobIdRef, latestRenderedJobIdRef, currentResRef } = renderRefs;
  const { t } = useTranslation();

  const selectedImage = useEditorStore((state) => state.selectedImage);
  const adjustments = useEditorStore((state) => state.adjustments);
  const previewOverride = useEditorStore((state) => state.previewOverride);
  const isWaveformVisible = useEditorStore((state) => state.isWaveformVisible);
  const activeWaveformChannel = useEditorStore((state) => state.activeWaveformChannel);
  const displaySize = useEditorStore((state) => state.displaySize);
  const baseRenderSize = useEditorStore((state) => state.baseRenderSize);
  const originalSize = useEditorStore((state) => state.originalSize);
  const isSliderDragging = useEditorStore((state) => state.isSliderDragging);
  const setEditor = useEditorStore((state) => state.setEditor);

  const activeView = useUIStore((state) => state.activeView);
  const activePanel = useUIStore((state) => state.activePanel);
  const appSettings = useSettingsStore((state) => state.appSettings);
  const multiSelectedPaths = useLibraryStore((state) => state.multiSelectedPaths);

  const uncroppedJobIdRef = useRef(0);
  const latestUncroppedJobIdRef = useRef(0);

  const inFlightCountRef = useRef(0);
  const lastAnalyticsTimeRef = useRef<number>(0);
  const pendingApplyRef = useRef<{ adjustments: Adjustments; targetRes?: number } | null>(null);
  const displayErrorText = useCallback((code: string) => {
    const errors: Record<string, string> = {
      NATIVE_DISPLAY_UNAVAILABLE: t('colorManagement.nativeDisplayUnavailable'),
      NATIVE_DISPLAY_COLOR_SPACE_UNSUPPORTED: t('colorManagement.nativeDisplayUnsupported'),
      NATIVE_DISPLAY_LOST: t('colorManagement.nativeDisplayLost'),
      NATIVE_DISPLAY_PROFILE_INVALID: t('colorManagement.nativeDisplayProfileInvalid'),
    };
    return errors[code] || code;
  }, [t]);

  const dragIdleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeWaveformChannelRef = useRef(activeWaveformChannel);
  activeWaveformChannelRef.current = activeWaveformChannel;

  const selectedImagePathRef = useRef<string | null>(null);
  useEffect(() => {
    setEditor({ hasRenderedFirstFrame: false, previewError: null });
  }, [selectedImage?.path, setEditor]);
  useEffect(() => {
    selectedImagePathRef.current = selectedImage?.path ?? null;
  }, [selectedImage?.path]);

  const calculateROI = useCallback(() => {
    if (!transformWrapperRef.current) return null;
    const state = transformWrapperRef.current.instance.transformState;
    if (!state) return null;

    if (!baseRenderSize) return null;

    const { scale, positionX, positionY } = state;
    const { width: baseW, height: baseH, offsetX, offsetY, containerWidth, containerHeight } = baseRenderSize;

    if (!baseW || !baseH || !containerWidth || !containerHeight) return null;
    if (scale <= 1.01) return null;

    const paddingPixels = 2.0;
    const paddingX = paddingPixels / baseW;
    const paddingY = paddingPixels / baseH;

    const visibleLeft = -positionX / scale;
    const visibleTop = -positionY / scale;
    const visibleRight = visibleLeft + containerWidth / scale;
    const visibleBottom = visibleTop + containerHeight / scale;

    const imgLeft = offsetX;
    const imgTop = offsetY;
    const imgRight = offsetX + baseW;
    const imgBottom = offsetY + baseH;

    const intersectLeft = Math.max(visibleLeft, imgLeft);
    const intersectTop = Math.max(visibleTop, imgTop);
    const intersectRight = Math.min(visibleRight, imgRight);
    const intersectBottom = Math.min(visibleBottom, imgBottom);

    if (intersectLeft >= intersectRight || intersectTop >= intersectBottom) {
      return null;
    }

    const roiX = (intersectLeft - imgLeft) / baseW;
    const roiY = (intersectTop - imgTop) / baseH;
    const roiW = (intersectRight - intersectLeft) / baseW;
    const roiH = (intersectBottom - intersectTop) / baseH;

    const newRoiX = roiX - paddingX;
    const newRoiY = roiY - paddingY;
    const newRoiW = roiW + paddingX * 2;
    const newRoiH = roiH + paddingY * 2;

    const clampedX = Math.max(0, newRoiX);
    const clampedY = Math.max(0, newRoiY);
    const clampedW = Math.min(1 - clampedX, newRoiW);
    const clampedH = Math.min(1 - clampedY, newRoiH);

    if (clampedW > 0.999 && clampedH > 0.999) return null;

    return [clampedX, clampedY, clampedW, clampedH] as [number, number, number, number];
  }, [baseRenderSize, transformWrapperRef]);

  const executeApplyAdjustments = useCallback(
    async (currentAdjustments: Adjustments, dragging: boolean = false, targetRes?: number, requestAnalytics = true) => {
      const currentPath = selectedImage?.path;
      if (!currentPath) return;

      let shouldRequestAnalytics = false;
      if (!requestAnalytics) {
        shouldRequestAnalytics = false;
      } else if (dragging) {
        const now = performance.now();
        if (now - lastAnalyticsTimeRef.current > 33.33) {
          shouldRequestAnalytics = true;
          lastAnalyticsTimeRef.current = now;
        }
      } else {
        shouldRequestAnalytics = true;
        lastAnalyticsTimeRef.current = 0;
      }

      const payload = structuredClone(currentAdjustments);
      if (currentPath !== selectedImagePathRef.current) return;
      const jobId = ++previewJobIdRef.current;
      const roi = calculateROI();

      try {
        await invoke(Invokes.ApplyAdjustments, {
          jsAdjustments: payload,
          isInteractive: dragging,
          targetResolution: targetRes || null,
          roi: roi || null,
          requestAnalytics: shouldRequestAnalytics,
          computeWaveform: requestAnalytics && !!isWaveformVisible,
          activeWaveformChannel: activeWaveformChannelRef.current || null,
        });

        if (currentPath !== selectedImagePathRef.current) return;

        if (jobId >= latestRenderedJobIdRef.current) {
          latestRenderedJobIdRef.current = jobId;
          setEditor({ previewError: null });
        }
      } catch (err) {
        if (
          err !== 'Superseded or worker failed' &&
          currentPath === selectedImagePathRef.current &&
          jobId >= latestRenderedJobIdRef.current
        ) {
          latestRenderedJobIdRef.current = jobId;
          setEditor({ previewError: displayErrorText(String(err)), hasRenderedFirstFrame: false });
        }
      }
    },
    [selectedImage?.path, calculateROI, isWaveformVisible, setEditor, previewJobIdRef, latestRenderedJobIdRef, displayErrorText],
  );

  const flushPipeline = useCallback(() => {
    if (inFlightCountRef.current >= 3) return;
    if (!pendingApplyRef.current) return;

    const { adjustments, targetRes } = pendingApplyRef.current;
    pendingApplyRef.current = null;

    inFlightCountRef.current += 1;

    executeApplyAdjustments(adjustments, true, targetRes).finally(() => {
      inFlightCountRef.current -= 1;
      if (pendingApplyRef.current) {
        requestAnimationFrame(() => flushPipeline());
      }
    });
  }, [executeApplyAdjustments]);

  const applyAdjustments = useCallback(
    (currentAdjustments: Adjustments, dragging: boolean = false, targetRes?: number) => {
      if (!selectedImage?.isReady) return;

      if (dragging) {
        pendingApplyRef.current = { adjustments: currentAdjustments, targetRes };
        flushPipeline();
      } else {
        pendingApplyRef.current = null;
        executeApplyAdjustments(currentAdjustments, false, targetRes);
      }
    },
    [selectedImage?.isReady, flushPipeline, executeApplyAdjustments],
  );

  useEffect(() => {
    let active = true;
    const unlisten = listen<string | null>('wgpu-display-unavailable', ({ payload }) => {
      if (!active) return;
      setEditor({ hasRenderedFirstFrame: false, previewError: displayErrorText(payload || 'NATIVE_DISPLAY_LOST') });
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop());
    };
  }, [setEditor, displayErrorText]);

  const throttledUncroppedPreview = useMemo(
    () =>
      debounce((adj: Adjustments) => {
        if (!useEditorStore.getState().selectedImage?.isReady) return;
        const imagePath = useEditorStore.getState().selectedImage?.path;
        const jobId = ++uncroppedJobIdRef.current;
        invoke<string>(Invokes.GenerateUncroppedPreview, {
          jsAdjustments: adj,
          targetResolution: Math.ceil(
            Math.max(useEditorStore.getState().displaySize.width, useEditorStore.getState().displaySize.height, 512) *
              1.25,
          ),
        })
          .then((dataUrl) => {
            if (
              imagePath === useEditorStore.getState().selectedImage?.path &&
              jobId >= latestUncroppedJobIdRef.current
            ) {
              latestUncroppedJobIdRef.current = jobId;
              useEditorStore.getState().setEditor({ uncroppedAdjustedPreviewUrl: dataUrl });
            }
          })
          .catch(console.error);
      }, 75),
    [],
  );

  const generateUncroppedPreview = useCallback(
    (currentAdjustments: Adjustments) => {
      throttledUncroppedPreview(currentAdjustments);
    },
    [throttledUncroppedPreview],
  );

  useEffect(() => {
    if (activeView === 'editor' && activePanel === Panel.Crop && selectedImage?.isReady) {
      generateUncroppedPreview(adjustments);
    }
  }, [
    activeView,
    adjustments,
    activePanel,
    selectedImage?.isReady,
    displaySize.width,
    displaySize.height,
    generateUncroppedPreview,
  ]);

  const calculateTargetRes = useCallback(() => {
    const baseTargetRes = appSettings?.editorPreviewResolution || 1920;
    if (!(appSettings?.enableZoomHifi ?? true) || displaySize.width === 0) {
      return baseTargetRes;
    }

    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
    const sharpnessFactor = 1.25;
    const zoomMultiplier = appSettings?.highResZoomMultiplier || 1.0;
    const effectiveDpr = appSettings?.useFullDpiRendering ? dpr : 1;

    let targetRes = Math.max(displaySize.width, displaySize.height) * effectiveDpr * sharpnessFactor * zoomMultiplier;
    targetRes = Math.max(targetRes, 512);

    if (originalSize && originalSize.width > 0 && originalSize.height > 0) {
      const origMax = Math.max(originalSize.width, originalSize.height);
      targetRes = Math.min(targetRes, origMax);
      if (targetRes >= origMax * 0.8) {
        targetRes = origMax;
      }
    }

    if (originalSize && targetRes !== Math.max(originalSize.width, originalSize.height)) {
      targetRes = Math.ceil(targetRes / 256) * 256;
    }

    return Math.round(targetRes);
  }, [
    appSettings?.enableZoomHifi,
    appSettings?.editorPreviewResolution,
    appSettings?.highResZoomMultiplier,
    appSettings?.useFullDpiRendering,
    displaySize.width,
    displaySize.height,
    originalSize,
  ]);

  const requestHiFiZoom = useMemo(
    () =>
      debounce((targetRes: number) => {
        if (targetRes > currentResRef.current) {
          currentResRef.current = targetRes;
          const { adjustments, previewOverride } = useEditorStore.getState();
          const renderAdjustments = previewOverride ?? adjustments;
          executeApplyAdjustments(renderAdjustments, false, targetRes, false);
        }
      }, 45),
    [executeApplyAdjustments, currentResRef],
  );

  useEffect(() => {
    if (
      activeView === 'editor' &&
      activePanel !== Panel.Crop &&
      selectedImage?.isReady &&
      displaySize.width > 0 &&
      !isSliderDragging
    ) {
      let baseRes = calculateTargetRes();
      if (originalSize.width > 0 && originalSize.height > 0) {
        const maxRes = Math.max(originalSize.width, originalSize.height);
        if (baseRes > maxRes) baseRes = maxRes;
      }
      const finalRes = Math.round(baseRes);

      if (finalRes > currentResRef.current) {
        requestHiFiZoom(finalRes);
      }
    }
    return () => {
      requestHiFiZoom.cancel();
    };
  }, [
    activeView,
    activePanel,
    displaySize.width,
    displaySize.height,
    calculateTargetRes,
    selectedImage?.isReady,
    isSliderDragging,
    requestHiFiZoom,
    originalSize,
  ]);

  useEffect(() => {
    if (!selectedImage?.isReady) return;

    if (dragIdleTimer.current) clearTimeout(dragIdleTimer.current);

    const targetRes = calculateTargetRes();
    const renderAdjustments = previewOverride ?? adjustments;

    if (activeView !== 'editor') {
      if (isSliderDragging) return;
    }

    if (isSliderDragging) {
      if (appSettings?.enableLivePreviews !== false) {
        applyAdjustments(renderAdjustments, true, targetRes);
      }
    } else {
      dragIdleTimer.current = setTimeout(() => {
        currentResRef.current = targetRes;

        applyAdjustments(renderAdjustments, false, targetRes);

        if (previewOverride) return;

        const prev = prevAdjustmentsRef.current;

        if (!prev || prev.path !== selectedImage.path) {
          prevAdjustmentsRef.current = { path: selectedImage.path, adjustments };
          return;
        }

        const hasAdjustmentsChanged = prev.adjustments !== adjustments;

        if (hasAdjustmentsChanged) {
          debouncedSave(selectedImage.path, adjustments);

          const otherPaths = multiSelectedPaths.filter((p) => p !== selectedImage.path);
          if (appSettings?.copyPasteSettings?.autoSync && otherPaths.length > 0) {
            const delta: Partial<Adjustments> = {};
            const includedKeys = appSettings?.copyPasteSettings?.includedAdjustments || COPYABLE_ADJUSTMENT_KEYS;
            for (const key of Object.keys(adjustments) as Array<keyof Adjustments>) {
              if (includedKeys.includes(key as string)) {
                if (JSON.stringify(adjustments[key]) !== JSON.stringify(prev.adjustments[key])) {
                  (delta as any)[key] = adjustments[key];
                }
              }
            }
            if (Object.keys(delta).length > 0) {
              otherPaths.forEach((p) => globalImageCache.delete(p));
              invoke(Invokes.ApplyAdjustmentsToPaths, { paths: otherPaths, adjustments: delta }).catch((err) => {
                console.error('Failed to apply adjustments to multi-selection:', err);
              });
            }
          }

          prevAdjustmentsRef.current = { path: selectedImage.path, adjustments };
        }
      }, 50);
    }

    return () => {
      if (dragIdleTimer.current) clearTimeout(dragIdleTimer.current);
    };
  }, [
    activeView,
    adjustments,
    previewOverride,
    selectedImage?.path,
    selectedImage?.isReady,
    isSliderDragging,
    multiSelectedPaths,
    appSettings?.enableLivePreviews,
    appSettings?.copyPasteSettings?.includedAdjustments,
    appSettings?.copyPasteSettings?.autoSync,
    isWaveformVisible,
  ]);

  return {
    applyAdjustments,
    executeApplyAdjustments,
  };
}
