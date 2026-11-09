import { invoke } from '@tauri-apps/api/core';
import type { Adjustments, MaskContainer } from './adjustments';
import { Mask, type SubMask } from '../components/panel/right/Masks';
import { useEditorStore } from '../store/useEditorStore';

export async function createSpot(
  containerId: string,
  template: SubMask,
  line: { points: { x: number; y: number }[]; brushSize: number; tool: string },
  path: string,
  adjustments: Adjustments,
  commit: (updater: (previous: Adjustments) => Adjustments) => void,
) {
  const id = crypto.randomUUID();
  const coordinateFrame = Object.fromEntries(
    Object.entries(adjustments).filter(
      ([key]) =>
        key.startsWith('transform') ||
        key.startsWith('lens') ||
        ['guidedPerspective', 'orientationSteps', 'rotation', 'flipHorizontal', 'flipVertical'].includes(key),
    ),
  );
  const parameters = { ...template.parameters, lines: [line], coordinateFrame };
  if (!parameters.manualSource) {
    delete parameters.sourceX;
    delete parameters.sourceY;
    parameters.sourceVariant = 0;
    const source = await invoke<[number, number] | null>('find_spot_source', {
      path,
      parameters,
      heal: template.type === Mask.Heal,
      variant: 0,
    });
    if (source) [parameters.sourceX, parameters.sourceY] = source;
  }
  if (useEditorStore.getState().selectedImage?.path !== path) return null;
  let applied = false;
  commit((previous) => ({
    ...previous,
    masks: previous.masks.map((container: MaskContainer) => {
      if (container.id !== containerId) return container;
      applied = true;
      return {
        ...container,
        subMasks: [
          ...container.subMasks.filter((mask) => mask.id !== template.id || mask.parameters?.lines?.length),
          { ...template, id, parameters },
        ],
      };
    }),
  }));
  return applied
    ? { id, sourceFound: Number.isFinite(parameters.sourceX) && Number.isFinite(parameters.sourceY) }
    : null;
}
