import {
  Brush,
  BringToFront,
  Circle,
  Cloud,
  Droplet,
  Droplets,
  Eraser,
  SquareMousePointer,
  RectangleHorizontal,
  TriangleRight,
  User,
  Sun,
  Stamp,
  Bandage,
  Spline,
  MopSparkles,
} from 'lucide-react';
import { motion } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { v4 as uuidv4 } from 'uuid';
import Text from '../../ui/Text';
import Slider from '../../ui/Slider';
import { TextWeights } from '../../../types/typography';
import i18n from 'i18next';

export enum Mask {
  AiDepth = 'ai-depth',
  AiForeground = 'ai-foreground',
  AiSky = 'ai-sky',
  AiSubject = 'ai-subject',
  All = 'all',
  Brush = 'brush',
  Flow = 'flow',
  Color = 'color',
  Linear = 'linear',
  Luminance = 'luminance',
  QuickEraser = 'quick-eraser',
  Radial = 'radial',
  Clone = 'clone',
  Heal = 'heal',
  Liquify = 'liquify',
  Retouch = 'retouch',
}

export enum SubMaskMode {
  Additive = 'additive',
  Subtractive = 'subtractive',
  Intersect = 'intersect',
}

export enum ToolType {
  AiSeletor = 'ai-selector',
  Brush = 'brush',
  Eraser = 'eraser',
  GenerativeReplace = 'generative-replace',
  SelectSubject = 'select-subject',
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
  if (type === Mask.AiDepth) return i18n.t('masks.types.depth');
  if (type === Mask.AiSubject) return i18n.t('masks.types.subject');
  if (type === Mask.AiForeground) return i18n.t('masks.types.foreground');
  if (type === Mask.AiSky) return i18n.t('masks.types.sky');
  if (type === Mask.All) return i18n.t('masks.types.all');
  if (type === Mask.QuickEraser) return i18n.t('masks.types.quickEraser');
  if (type === Mask.Brush) return i18n.t('masks.types.brush');
  if (type === Mask.Flow) return i18n.t('masks.types.flow');
  if (type === Mask.Color) return i18n.t('masks.types.color');
  if (type === Mask.Linear) return i18n.t('masks.types.linear');
  if (type === Mask.Luminance) return i18n.t('masks.types.luminance');
  if (type === Mask.Radial) return i18n.t('masks.types.radial');
  if (type === Mask.Clone) return i18n.t('masks.types.clone');
  if (type === Mask.Heal) return i18n.t('masks.types.heal');
  if (type === Mask.Liquify) return i18n.t('masks.types.liquify');
  if (type === Mask.Retouch) return i18n.t('masks.types.retouch');
  return type.charAt(0).toUpperCase() + type.slice(1);
}

export function getMaskTypeName(mask: MaskType) {
  if (mask.id === 'others') return i18n.t('masks.types.others');
  if (mask.type === Mask.QuickEraser && mask.name === 'Quick Erase') {
    return i18n.t('masks.types.quickErase');
  }
  return formatMaskTypeName(mask.type);
}

export function getSubMaskName(subMask: Pick<SubMask, 'name' | 'type'>) {
  return subMask.name?.trim() || formatMaskTypeName(subMask.type);
}

export const MASK_ICON_MAP: Record<Mask, any> = {
  [Mask.AiDepth]: BringToFront,
  [Mask.AiForeground]: User,
  [Mask.AiSky]: Cloud,
  [Mask.AiSubject]: SquareMousePointer,
  [Mask.All]: RectangleHorizontal,
  [Mask.Brush]: Brush,
  [Mask.Flow]: Droplets,
  [Mask.Color]: Droplet,
  [Mask.Linear]: TriangleRight,
  [Mask.Luminance]: Sun,
  [Mask.QuickEraser]: Eraser,
  [Mask.Radial]: Circle,
  [Mask.Clone]: Stamp,
  [Mask.Heal]: Bandage,
  [Mask.Liquify]: Spline,
  [Mask.Retouch]: MopSparkles,
};

export const MASK_AI_TYPES: Array<MaskType> = [
  { disabled: false, icon: SquareMousePointer, name: 'Subject', type: Mask.AiSubject },
  { disabled: false, icon: Cloud, name: 'Sky', type: Mask.AiSky },
  { disabled: false, icon: User, name: 'Foreground', type: Mask.AiForeground },
  { disabled: false, icon: BringToFront, name: 'Depth', type: Mask.AiDepth },
];

export const MASK_BASIC_TYPES: Array<MaskType> = [
  { disabled: false, icon: Brush, name: 'Brush', type: Mask.Brush },
  { disabled: false, icon: TriangleRight, name: 'Linear', type: Mask.Linear },
  { disabled: false, icon: Circle, name: 'Radial', type: Mask.Radial },
  { disabled: false, icon: Droplets, name: 'Flow', type: Mask.Flow },
];

export const MASK_RANGE_TYPES: Array<MaskType> = [
  { disabled: false, icon: Droplet, name: 'Color', type: Mask.Color },
  { disabled: false, icon: Sun, name: 'Luminance', type: Mask.Luminance },
  { disabled: false, icon: RectangleHorizontal, name: 'Whole Image', type: Mask.All },
];

export const ALL_MASK_TYPES = [...MASK_AI_TYPES, ...MASK_BASIC_TYPES, ...MASK_RANGE_TYPES];

export const AI_DIRECT_PATCH_TYPES: Array<MaskType> = [
  {
    disabled: false,
    icon: Stamp,
    name: 'Clone',
    type: Mask.Clone,
  },
  {
    disabled: false,
    icon: Bandage,
    name: 'Heal',
    type: Mask.Heal,
  },
];

export const AI_TOUCH_UP_TYPES: Array<MaskType> = [
  {
    disabled: false,
    icon: Spline,
    name: 'Liquify',
    type: Mask.Liquify,
  },
  {
    disabled: false,
    icon: MopSparkles,
    name: 'Retouch',
    type: Mask.Retouch,
  },
];

export const AI_GENERATIVE_CREATION_TYPES: Array<MaskType> = [
  {
    disabled: false,
    icon: Eraser,
    name: 'Quick Erase',
    type: Mask.QuickEraser,
  },
  {
    disabled: false,
    icon: SquareMousePointer,
    name: 'Subject',
    type: Mask.AiSubject,
  },
  {
    disabled: false,
    icon: User,
    name: 'Foreground',
    type: Mask.AiForeground,
  },
  {
    disabled: false,
    icon: Brush,
    name: 'Brush',
    type: Mask.Brush,
  },
  {
    disabled: false,
    icon: TriangleRight,
    name: 'Linear',
    type: Mask.Linear,
  },
  {
    disabled: false,
    icon: Circle,
    name: 'Radial',
    type: Mask.Radial,
  },
];

export const OTHERS_MASK_TYPES: Array<MaskType> = [
  {
    disabled: false,
    icon: BringToFront,
    name: 'Depth',
    type: Mask.AiDepth,
  },
  {
    disabled: false,
    icon: Droplet,
    name: 'Color',
    type: Mask.Color,
  },
  {
    disabled: false,
    icon: Sun,
    name: 'Luminance',
    type: Mask.Luminance,
  },
  {
    disabled: false,
    icon: Brush,
    name: 'Brush',
    type: Mask.Brush,
  },
  {
    disabled: false,
    icon: Droplets,
    name: 'Flow',
    type: Mask.Flow,
  },
  {
    disabled: false,
    icon: RectangleHorizontal,
    name: 'Whole Image',
    type: Mask.All,
  },
];

export const AI_SUB_MASK_COMPONENT_TYPES: Array<MaskType> = [
  ...AI_DIRECT_PATCH_TYPES,
  ...AI_TOUCH_UP_TYPES,
  ...AI_GENERATIVE_CREATION_TYPES,
];

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
      <Text weight={TextWeights.medium}>{t(textKey)}</Text>
    </motion.div>
  );
}

export const BrushTools = ({
  settings,
  onSettingsChange,
  onDragStateChange,
  labelPrefix = 'editor.masks.brush',
}: {
  settings: any;
  onSettingsChange: any;
  onDragStateChange?: (isDragging: boolean) => void;
  labelPrefix?: string;
}) => {
  const { t } = useTranslation();

  return (
    <div>
      <Slider
        defaultValue={100}
        label={t(`${labelPrefix}.size`)}
        max={200}
        min={1}
        onChange={(e: any) => onSettingsChange((s: any) => ({ ...s, size: Number(e.target.value) }))}
        step={1}
        value={settings.size}
        fillOrigin="min"
        onDragStateChange={onDragStateChange}
      />
      <Slider
        defaultValue={50}
        label={t(`${labelPrefix}.feather`)}
        max={100}
        min={0}
        onChange={(e: any) => onSettingsChange((s: any) => ({ ...s, feather: Number(e.target.value) }))}
        step={1}
        value={settings.feather}
        fillOrigin="min"
        onDragStateChange={onDragStateChange}
      />
      <div className="grid grid-cols-2 gap-2 pt-2">
        <button
          className={`p-2 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2 ${
            settings.tool === ToolType.Brush ? 'text-primary bg-surface' : 'bg-surface text-text-secondary hover:bg-card-active'
          }`}
          onClick={() => onSettingsChange((s: any) => ({ ...s, tool: ToolType.Brush }))}
        >
          {t(`${labelPrefix}.${labelPrefix === 'editor.ai.brush' ? 'add' : 'brush'}`)}
        </button>
        <button
          className={`p-2 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2 ${
            settings.tool === ToolType.Eraser ? 'text-primary bg-surface' : 'bg-surface text-text-secondary hover:bg-card-active'
          }`}
          onClick={() => onSettingsChange((s: any) => ({ ...s, tool: ToolType.Eraser }))}
        >
          {t(`${labelPrefix}.${labelPrefix === 'editor.ai.brush' ? 'erase' : 'eraser'}`)}
        </button>
      </div>
    </div>
  );
};

export const cloneSubMaskData = (
  subMask: SubMask,
  options: { invert?: boolean; rename?: boolean; copyName?: string } = {},
): SubMask => {
  const clonedSubMask = JSON.parse(JSON.stringify(subMask));
  clonedSubMask.id = uuidv4();
  clonedSubMask.invert = options.invert ? !clonedSubMask.invert : clonedSubMask.invert;
  clonedSubMask.name =
    options.rename === false
      ? clonedSubMask.name
      : (options.copyName ?? `${getSubMaskName(subMask)} Copy`);
  return clonedSubMask;
};
