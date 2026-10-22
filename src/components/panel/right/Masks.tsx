import {
  Brush,
  Circle,
  Droplet,
  Droplets,
  RectangleHorizontal,
  TriangleRight,
  Sun,
  Stamp,
  Bandage,
} from 'lucide-react';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import Text from '../../ui/Text';
import { TextWeights } from '../../../types/typography';
import i18n from 'i18next';

export enum Mask {
  All = 'all',
  Brush = 'brush',
  Flow = 'flow',
  Color = 'color',
  Linear = 'linear',
  Luminance = 'luminance',
  Radial = 'radial',
  Clone = 'clone',
  Heal = 'heal',
}

export enum SubMaskMode {
  Additive = 'additive',
  Subtractive = 'subtractive',
  Intersect = 'intersect',
}

export enum ToolType {
  Brush = 'brush',
  Eraser = 'eraser',
}

export interface MaskType {
  disabled: boolean;
  icon: any;
  id?: string;
  name: string;
  type: Mask;
}

export interface SubMask {
  id: string;
  invert: boolean;
  mode: SubMaskMode;
  name?: string;
  opacity: number;
  parameters?: any;
  type: Mask;
  visible: boolean;
}

export function formatMaskTypeName(type: string) {
  if (type === Mask.All) return i18n.t('masks.types.all');
  if (type === Mask.Brush) return i18n.t('masks.types.brush');
  if (type === Mask.Flow) return i18n.t('masks.types.flow');
  if (type === Mask.Color) return i18n.t('masks.types.color');
  if (type === Mask.Linear) return i18n.t('masks.types.linear');
  if (type === Mask.Luminance) return i18n.t('masks.types.luminance');
  if (type === Mask.Radial) return i18n.t('masks.types.radial');
  if (type === Mask.Clone) return i18n.t('masks.types.clone');
  if (type === Mask.Heal) return i18n.t('masks.types.heal');
  return type.charAt(0).toUpperCase() + type.slice(1);
}

export function getMaskTypeName(mask: MaskType) {
  if (mask.id === 'others') return i18n.t('masks.types.others');
  return formatMaskTypeName(mask.type);
}

export function getSubMaskName(subMask: Pick<SubMask, 'name' | 'type'>) {
  return subMask.name?.trim() || formatMaskTypeName(subMask.type);
}

export const MASK_ICON_MAP: Record<Mask, any> = {
  [Mask.All]: RectangleHorizontal,
  [Mask.Brush]: Brush,
  [Mask.Flow]: Droplets,
  [Mask.Color]: Droplet,
  [Mask.Linear]: TriangleRight,
  [Mask.Luminance]: Sun,
  [Mask.Radial]: Circle,
  [Mask.Clone]: Stamp,
  [Mask.Heal]: Bandage,
};

export const MASK_BASIC_TYPES: Array<MaskType> = [
  { disabled: false, icon: Brush, name: 'Brush', type: Mask.Brush },
  { disabled: false, icon: Stamp, name: 'Clone', type: Mask.Clone },
  { disabled: false, icon: Bandage, name: 'Heal', type: Mask.Heal },
  { disabled: false, icon: TriangleRight, name: 'Linear', type: Mask.Linear },
  { disabled: false, icon: Circle, name: 'Radial', type: Mask.Radial },
  { disabled: false, icon: Droplets, name: 'Flow', type: Mask.Flow },
];

export const MASK_RANGE_TYPES: Array<MaskType> = [
  { disabled: false, icon: Droplet, name: 'Color', type: Mask.Color },
  { disabled: false, icon: Sun, name: 'Luminance', type: Mask.Luminance },
  { disabled: false, icon: RectangleHorizontal, name: 'Whole Image', type: Mask.All },
];

export const ALL_MASK_TYPES = [...MASK_BASIC_TYPES, ...MASK_RANGE_TYPES];

export function NewMaskDropZone({ isOver, textKey }: { isOver: boolean; textKey: string }) {
  const { t } = useTranslation();
  return (
    <motion.div
      layout
      initial={{ opacity: 0, height: 0, marginTop: 0 }}
      animate={{ opacity: 1, height: 'auto', marginTop: '4px' }}
      exit={{ opacity: 0, height: 0, marginTop: 0 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className={`p-3 rounded-lg text-center ${isOver ? 'border border-accent/80 bg-bg-tertiary/50' : ''}`}
    >
      <Text weight={TextWeights.medium}>{String(t(textKey as any))}</Text>
    </motion.div>
  );
}
