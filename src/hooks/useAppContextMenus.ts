import { useCallback } from 'react';
import {
  Aperture,
  Check,
  ClipboardPaste,
  Copy,
  FileInput,
  Grip,
  Film,
  LayersArrowDown,
  Layers,
  Redo,
  RotateCcw,
  Undo,
  X,
  PencilSparkles,
  ScanLine,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useContextMenu } from '../context/ContextMenuContext';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { Option, OPTION_SEPARATOR, Panel } from '../components/ui/AppProperties';
import {
  getAiDenoiseEnablePatch,
  INITIAL_ADJUSTMENTS,
} from '../utils/adjustments';
import { buildAutoAdjustModeOptions, useEditorActions } from './useEditorActions';



export function useAppContextMenus() {
  const { t } = useTranslation();
  const { showContextMenu } = useContextMenu();

  const {
    handleAutoAdjustments,
    handleAutoLensCorrection,
    handleAutoStraighten,
    handleEnableAdjustments,
    handleCopyAdjustments,
    handlePasteAdjustments,
  } = useEditorActions();

  const handleEditorContextMenu = useCallback(
    (event: any) => {
      event.preventDefault();
      event.stopPropagation();

      const {
        selectedImage,
        adjustments,
        history,
        historyIndex,
        undo,
        redo,
        resetHistory,
        copiedAdjustments,
        setEditor,
      } = useEditorStore.getState();
      const { setPanel } = useUIStore.getState();

      if (!selectedImage) return;

      const canUndo = historyIndex > 0;
      const canRedo = historyIndex < history.length - 1;

      const options: Array<Option> = [
        {
          label: t('contextMenus.editor.exportImage'),
          icon: FileInput,
          onClick: () => setPanel(Panel.Export),
        },
        { type: OPTION_SEPARATOR },
        { label: t('contextMenus.editor.undo'), icon: Undo, onClick: undo, disabled: !canUndo },
        { label: t('contextMenus.editor.redo'), icon: Redo, onClick: redo, disabled: !canRedo },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.editor.copyAdjustments'),
          icon: Copy,
          onClick: () => handleCopyAdjustments(),
        },
        {
          label: t('contextMenus.editor.pasteAdjustments'),
          icon: ClipboardPaste,
          onClick: () => handlePasteAdjustments(),
          disabled: copiedAdjustments === null,
        },
        {
          label: t('contextMenus.editor.productivity'),
          icon: Grip,
          submenu: [
            {
              label: t('contextMenus.editor.autoAdjust'),
              icon: PencilSparkles,
              disabled: !selectedImage?.isReady,
              submenu: buildAutoAdjustModeOptions(t, handleAutoAdjustments),
            },
            {
              label: t('contextMenus.editor.autoLensCorrection'),
              icon: Aperture,
              onClick: () => handleAutoLensCorrection([selectedImage.path]),
              disabled: !selectedImage?.isReady,
            },
            {
              label: t('contextMenus.editor.autoStraighten'),
              icon: ScanLine,
              onClick: handleAutoStraighten,
              disabled: !selectedImage?.isReady,
            },
            {
              label: adjustments.aiDenoiseEnabled
                ? t('contextMenus.editor.removeDenoise')
                : t('contextMenus.editor.denoise'),
              icon: Grip,
              onClick: () =>
                handleEnableAdjustments(
                  [selectedImage.path],
                  adjustments.aiDenoiseEnabled
                    ? { aiDenoiseEnabled: false }
                    : getAiDenoiseEnablePatch(useSettingsStore.getState().appSettings?.aiProvider),
                ),
              disabled: !selectedImage?.isReady,
            },
            {
              label: adjustments.negativeConversionEnabled
                ? t('contextMenus.editor.removeNegativeConversion')
                : t('contextMenus.editor.convertNegative'),
              icon: Film,
              onClick: () =>
                handleEnableAdjustments([selectedImage.path], {
                  negativeConversionEnabled: !adjustments.negativeConversionEnabled,
                }),
              disabled: !selectedImage?.isReady,
            },
          ],
        },
        {
          label: t('contextMenus.merge.title'),
          icon: LayersArrowDown,
          submenu: [{ disabled: true, icon: Layers, label: t('contextMenus.merge.focusStack') }],
        },
        { type: OPTION_SEPARATOR },
        {
          label: t('contextMenus.editor.resetAdjustments'),
          icon: RotateCcw,
          submenu: [
            { label: t('contextMenus.editor.cancel'), icon: X, onClick: () => {} },
            {
              label: t('contextMenus.editor.confirmReset'),
              icon: Check,
              isDestructive: true,
              onClick: () => {
                resetHistory({
                  ...INITIAL_ADJUSTMENTS,
                  aiPatches: [],
                });
                setEditor({ adjustments: { ...INITIAL_ADJUSTMENTS, aiPatches: [] } });
              },
            },
          ],
        },
      ];
      showContextMenu(event.clientX, event.clientY, options);
    },
    [handleCopyAdjustments, handlePasteAdjustments, handleAutoAdjustments, showContextMenu, t],
  );

  return { handleEditorContextMenu };
}