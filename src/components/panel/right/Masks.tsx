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
  Eye,
  EyeOff,
  FileEdit,
  Loader2,
  Minus,
  Plus,
  PlusSquare,
  RotateCcw,
  Trash2,
  Copy,
  ClipboardPaste,
  SquaresIntersect,
} from 'lucide-react';
import { useState, useRef, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { useDraggable, useDroppable } from '@dnd-kit/core';
import Text from '../../ui/Text';
import Slider from '../../ui/Slider';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { useContextMenu } from '../../../context/ContextMenuContext';
import { OPTION_SEPARATOR } from '../../ui/AppProperties';
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
  clonedSubMask.id = crypto.randomUUID();
  clonedSubMask.invert = options.invert ? !clonedSubMask.invert : clonedSubMask.invert;
  clonedSubMask.name =
    options.rename === false
      ? clonedSubMask.name
      : (options.copyName ?? `${getSubMaskName(subMask)} Copy`);
  return clonedSubMask;
};

export interface SharedSubMaskRowProps {
  subMask: any;
  index: number;
  totalCount: number;
  containerId: string;
  isActive: boolean;
  parentVisible?: boolean;
  activeAiTasks: Record<string, any>;
  onSelect: () => void;
  updateSubMask: (id: string, updates: any) => void;
  handleDelete: () => void;
  handleDuplicate: () => void;
  handleDuplicateAndInvert: () => void;
  handlePaste: () => void;
  handleCopy: () => void;
  hasCopiedSubMask: boolean;
  activeDragItem: any;
  renamingId: string | null;
  setRenamingId: (id: string | null) => void;
  tempName: string;
  setTempName: (name: string) => void;
  i18nPrefix: 'editor.masks' | 'editor.ai';
  isParentLoading?: boolean;
  setIsMaskControlHovered?: (isHovered: boolean) => void;
}

export function SharedSubMaskRow({
  subMask,
  index,
  totalCount,
  containerId,
  isActive,
  parentVisible,
  activeAiTasks,
  onSelect,
  updateSubMask,
  handleDelete,
  handleDuplicate,
  handleDuplicateAndInvert,
  handlePaste,
  handleCopy,
  hasCopiedSubMask,
  activeDragItem,
  renamingId,
  setRenamingId,
  tempName,
  setTempName,
  i18nPrefix,
  isParentLoading,
  setIsMaskControlHovered,
}: SharedSubMaskRowProps) {
  const { t } = useTranslation();
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: subMask.id,
    data: { type: 'SubMask', item: subMask, parentId: containerId },
  });
  const { setNodeRef: setDroppableRef, isOver } = useDroppable({
    id: subMask.id,
    data: { type: 'SubMask', item: subMask, parentId: containerId },
  });
  const setCombinedRef = (node: HTMLElement | null) => {
    setNodeRef(node);
    setDroppableRef(node);
  };
  const MaskIcon = MASK_ICON_MAP[subMask.type as Mask] || Circle;
  const { showContextMenu } = useContextMenu();
  const [isHovered, setIsHovered] = useState(false);
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const isDraggingContainer = activeDragItem?.type === 'Container';
  const isSubMaskTaskRunning = Boolean(activeAiTasks?.[subMask.id]);
  const isAnalyzing =
    isSubMaskTaskRunning || (Boolean(isParentLoading) && subMask.type === Mask.QuickEraser);

  const handleMouseEnter = () => {
    if (hoverTimeoutRef.current) {
      clearTimeout(hoverTimeoutRef.current);
      hoverTimeoutRef.current = null;
    }
    setIsHovered(true);
  };
  const handleMouseLeave = () => {
    hoverTimeoutRef.current = setTimeout(() => setIsHovered(false), 1000);
  };

  useEffect(() => {
    return () => {
      if (hoverTimeoutRef.current) clearTimeout(hoverTimeoutRef.current);
    };
  }, []);

  const handleRenameSubmit = () => {
    if (tempName.trim()) {
      updateSubMask(subMask.id, { name: tempName.trim() });
    }
    setRenamingId(null);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e.clientX, e.clientY, [
      {
        label: t(`${i18nPrefix}.actions.rename`),
        icon: FileEdit,
        onClick: () => {
          setRenamingId(subMask.id);
          setTempName(getSubMaskName(subMask));
        },
      },
      { label: t(`${i18nPrefix}.actions.duplicateComponent`), icon: PlusSquare, onClick: handleDuplicate },
      {
        label: t(`${i18nPrefix}.actions.duplicateAndInvertComponent`),
        icon: RotateCcw,
        onClick: handleDuplicateAndInvert,
      },
      { label: t(`${i18nPrefix}.actions.copyComponent`), icon: Copy, onClick: handleCopy },
      {
        label: t(`${i18nPrefix}.actions.pasteComponent`),
        icon: ClipboardPaste,
        disabled: !hasCopiedSubMask,
        onClick: handlePaste,
      },
      { type: OPTION_SEPARATOR },
      { label: t(`${i18nPrefix}.actions.deleteComponent`), icon: Trash2, isDestructive: true, onClick: handleDelete },
    ]);
  };

  const showNumber = isHovered && totalCount > 1;

  return (
    <motion.div
      layout="position"
      initial={{ opacity: 0, x: -15 }}
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={{ opacity: 0, x: -15, scale: 0.95, transition: { duration: 0.2 } }}
      ref={setCombinedRef}
      {...attributes}
      {...listeners}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      className={`flex items-center gap-2 p-2 rounded-md transition-colors group cursor-pointer
            ${isActive ? 'bg-surface' : 'hover:bg-card-active'}
            ${isOver && !isDraggingContainer ? 'border-t-2 border-accent' : ''}
            ${isDragging ? 'opacity-40 z-50' : ''}
            ${parentVisible === false ? 'opacity-50' : ''}
            ${isDraggingContainer ? 'opacity-30 pointer-events-none' : ''}
            transition-opacity duration-300`}
      onClick={(e) => {
        e.stopPropagation();
        onSelect();
      }}
      onContextMenu={onContextMenu}
    >
      <Text
        as="div"
        variant={TextVariants.small}
        weight={TextWeights.bold}
        className="relative w-4 h-4 ml-1 shrink-0 flex items-center justify-center"
      >
        <AnimatePresence mode="wait" initial={false}>
          {isAnalyzing ? (
            <motion.div
              key="analyzing"
              initial={{ opacity: 0, scale: 0.5 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.5 }}
              transition={{ duration: 0.15 }}
              className="absolute flex items-center justify-center"
            >
              <Loader2 size={16} className="animate-spin text-accent" />
            </motion.div>
          ) : showNumber ? (
            <motion.span
              key="number"
              initial={{ opacity: 0, scale: 0.5 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.5 }}
              transition={{ duration: 0.15 }}
              className="absolute"
            >
              {index}
            </motion.span>
          ) : (
            <motion.div
              key="icon"
              initial={{ opacity: 0, scale: 0.5 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.5 }}
              transition={{ duration: 0.15 }}
              className="absolute flex items-center justify-center"
            >
              <MaskIcon size={16} />
            </motion.div>
          )}
        </AnimatePresence>
      </Text>
      {renamingId === subMask.id ? (
        <input
          autoFocus
          className="bg-bg-primary text-sm w-full rounded px-1 outline-none border border-accent"
          value={tempName}
          onChange={(e) => setTempName(e.target.value)}
          onBlur={handleRenameSubmit}
          onKeyDown={(e) => e.key === 'Enter' && handleRenameSubmit()}
          onClick={(e) => e.stopPropagation()}
        />
      ) : (
        <Text color={TextColors.primary} className="flex-1 truncate select-none">
          {getSubMaskName(subMask)}
        </Text>
      )}
      <div className="flex opacity-0 group-hover:opacity-100 transition-opacity">
        {index > 1 && (
          <button
            className="p-1 hover:text-text-primary text-text-secondary"
            data-tooltip={
              subMask.mode === SubMaskMode.Additive
                ? t(`${i18nPrefix}.actions.switchToSubtract`)
                : subMask.mode === SubMaskMode.Subtractive
                  ? t(`${i18nPrefix}.actions.switchToIntersect`)
                  : t(`${i18nPrefix}.actions.switchToAdd`)
            }
            onClick={(e) => {
              e.stopPropagation();
              updateSubMask(subMask.id, {
                mode:
                  subMask.mode === SubMaskMode.Additive
                    ? SubMaskMode.Subtractive
                    : subMask.mode === SubMaskMode.Subtractive
                      ? SubMaskMode.Intersect
                      : SubMaskMode.Additive,
              });
            }}
          >
            {subMask.mode === SubMaskMode.Additive ? (
              <Plus size={16} />
            ) : subMask.mode === SubMaskMode.Subtractive ? (
              <Minus size={16} />
            ) : (
              <SquaresIntersect size={16} />
            )}
          </button>
        )}
        {setIsMaskControlHovered && (
          <button
            className="p-1 hover:text-text-primary text-text-secondary"
            data-tooltip={
              subMask.visible ? t(`${i18nPrefix}.actions.hideComponent`) : t(`${i18nPrefix}.actions.showComponent`)
            }
            onMouseEnter={() => setIsMaskControlHovered(true)}
            onMouseLeave={() => setIsMaskControlHovered(false)}
            onClick={(e) => {
              e.stopPropagation();
              updateSubMask(subMask.id, { visible: !subMask.visible });
            }}
          >
            {subMask.visible ? <Eye size={16} /> : <EyeOff size={16} />}
          </button>
        )}
        <button
          className="p-1 hover:text-red-500 text-text-secondary"
          data-tooltip={t(`${i18nPrefix}.actions.deleteComponent`)}
          onClick={(e) => {
            e.stopPropagation();
            handleDelete();
          }}
        >
          <Trash2 size={16} />
        </button>
      </div>
    </motion.div>
  );
}
