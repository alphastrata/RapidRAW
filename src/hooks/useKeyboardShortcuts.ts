import { useEffect } from 'react';
import { Panel } from '../components/ui/AppProperties';
import { KEYBIND_DEFINITIONS, getDefaultCombo, normalizeCombo } from '../utils/keyboardUtils';
import { useEditorStore } from '../store/useEditorStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { useEditorActions } from './useEditorActions';

interface KeyboardShortcutsProps {
  handleOpenImage(): void;
  handleZoomChange(zoomValue: number, fitToWindow?: boolean): void;
}

export const useKeyboardShortcuts = ({ handleOpenImage, handleZoomChange }: KeyboardShortcutsProps) => {
  const { handleRotate, handleAutoStraighten, handleCopyAdjustments, handlePasteAdjustments, toggleCompare } =
    useEditorActions();

  useEffect(() => {
    const getStoreState = () => ({
      editor: useEditorStore.getState(),
      ui: useUIStore.getState(),
      settings: useSettingsStore.getState(),
      process: useProcessStore.getState(),
    });
    type ShortcutState = ReturnType<typeof getStoreState>;

    const comboMap = new Map<string, string>();
    const { appSettings, osPlatform } = useSettingsStore.getState();
    const keybinds = appSettings?.keybinds;

    for (const def of KEYBIND_DEFINITIONS) {
      const userCombo = keybinds?.[def.action];
      const effective = userCombo && userCombo.length > 0 ? userCombo : getDefaultCombo(def, osPlatform);
      if (effective) {
        comboMap.set(effective.join('+'), def.action);
      }
    }

    const actions: Record<string, any> = {
      open_new_image: {
        shouldFire: () => true,
        execute: (e: any) => {
          e.preventDefault();
          handleOpenImage();
        },
      },
      copy_adjustments: {
        shouldFire: () => true,
        execute: (e: any) => {
          e.preventDefault();
          handleCopyAdjustments();
        },
      },
      paste_adjustments: {
        shouldFire: () => true,
        execute: (e: any) => {
          e.preventDefault();
          handlePasteAdjustments();
        },
      },
      copy_image_path: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const physicalPath = s.editor.selectedImage.path.split('?vc=')[0];
          navigator.clipboard.writeText(physicalPath).catch((err: any) =>
            console.error('Failed to copy image path to clipboard', err),
          );
        },
      },
      zoom_in_step: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
          const currentPercent =
            s.editor.originalSize?.width > 0 && s.editor.displaySize?.width > 0
              ? (s.editor.displaySize.width * dpr) / s.editor.originalSize.width
              : 1.0;
          handleZoomChange(Math.min(currentPercent + 0.1, 2.0));
        },
      },
      zoom_out_step: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
          const currentPercent =
            s.editor.originalSize?.width > 0 && s.editor.displaySize?.width > 0
              ? (s.editor.displaySize.width * dpr) / s.editor.originalSize.width
              : 1.0;
          handleZoomChange(Math.max(currentPercent - 0.1, 0.1));
        },
      },
      cycle_zoom: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
          const { originalSize, displaySize, baseRenderSize } = s.editor;
          const currentPercent =
            originalSize?.width > 0 && displaySize?.width > 0
              ? Math.round(((displaySize.width * dpr) / originalSize.width) * 100)
              : 100;
          let fitPercent = 100;

          if (originalSize?.width > 0 && baseRenderSize?.width > 0) {
            const originalAspect = originalSize.width / originalSize.height;
            const baseAspect = baseRenderSize.width / baseRenderSize.height;
            fitPercent =
              originalAspect > baseAspect
                ? Math.round(((baseRenderSize.width * dpr) / originalSize.width) * 100)
                : Math.round(((baseRenderSize.height * dpr) / originalSize.height) * 100);
          }

          const doubleFitPercent = fitPercent * 2;
          if (Math.abs(currentPercent - fitPercent) < 5) {
            handleZoomChange(doubleFitPercent < 100 ? doubleFitPercent / 100 : 1.0);
          } else if (Math.abs(currentPercent - doubleFitPercent) < 5 && doubleFitPercent < 100) {
            handleZoomChange(1.0);
          } else {
            handleZoomChange(0, true);
          }
        },
      },
      zoom_in: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
          const currentPercent =
            s.editor.originalSize?.width > 0 && s.editor.displaySize?.width > 0
              ? (s.editor.displaySize.width * dpr) / s.editor.originalSize.width
              : 1.0;
          handleZoomChange(Math.min(currentPercent * 1.2, 2.0));
        },
      },
      zoom_out: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
          const currentPercent =
            s.editor.originalSize?.width > 0 && s.editor.displaySize?.width > 0
              ? (s.editor.displaySize.width * dpr) / s.editor.originalSize.width
              : 1.0;
          handleZoomChange(Math.max(currentPercent / 1.2, 0.1));
        },
      },
      zoom_fit: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any) => {
          e.preventDefault();
          handleZoomChange(0, true);
        },
      },
      zoom_100: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any) => {
          e.preventDefault();
          handleZoomChange(1.0);
        },
      },
      rotate_left: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any) => {
          e.preventDefault();
          handleRotate(-90);
        },
      },
      rotate_right: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any) => {
          e.preventDefault();
          handleRotate(90);
        },
      },
      undo: {
        shouldFire: (s: any) => !!s.editor.selectedImage && s.editor.historyIndex > 0,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.editor.undo();
        },
      },
      redo: {
        shouldFire: (s: any) => !!s.editor.selectedImage && s.editor.historyIndex < s.editor.history.length - 1,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.editor.redo();
        },
      },
      toggle_fullscreen: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.toggleFullScreen();
        },
      },
      show_original: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any) => {
          e.preventDefault();
          toggleCompare();
        },
      },
      toggle_adjustments: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.setPanel(Panel.Adjustments);
        },
      },
      toggle_crop_panel: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.setPanel(Panel.Crop);
        },
      },
      toggle_masks: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.setPanel(Panel.Masks);
        },
      },
      toggle_ai: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.setPanel(Panel.Ai);
        },
      },
      toggle_analytics: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const nextVisibility = !s.editor.isWaveformVisible;
          s.editor.setEditor({ isWaveformVisible: nextVisibility });
          s.settings.handleSettingsChange({
            ...s.settings.appSettings,
            isWaveformVisible: nextVisibility,
          });
        },
      },
      toggle_export: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          s.ui.setPanel(Panel.Export);
        },
      },
      toggle_left_panel: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const isOpening = !s.ui.uiVisibility.leftPanel;
          s.ui.setUI((state: any) => ({
            uiVisibility: { ...state.uiVisibility, leftPanel: isOpening },
            leftPanelWidth: isOpening && state.leftPanelWidth < 250 ? 350 : state.leftPanelWidth,
          }));
        },
      },
      toggle_right_panel: {
        shouldFire: () => true,
        execute: (e: any, s: any) => {
          e.preventDefault();
          const isOpening = !s.ui.uiVisibility.rightPanel;
          s.ui.setUI((state: any) => ({
            uiVisibility: { ...state.uiVisibility, rightPanel: isOpening },
            rightPanelWidth: isOpening && state.rightPanelWidth < 250 ? 350 : state.rightPanelWidth,
          }));
        },
      },
      toggle_crop: {
        shouldFire: (s: any) => !!s.editor.selectedImage,
        execute: (e: any, s: any) => {
          e.preventDefault();
          if (s.ui.activePanel === Panel.Crop) {
            s.editor.setEditor({ isStraightenActive: !s.editor.isStraightenActive });
          } else {
            s.ui.setPanel(Panel.Crop);
            s.editor.setEditor({ isStraightenActive: true });
          }
        },
      },
      auto_straighten: {
        shouldFire: (s: ShortcutState) => !!s.editor.selectedImage,
        execute: (e: KeyboardEvent, s: ShortcutState) => {
          e.preventDefault();
          if (s.ui.activePanel !== Panel.Crop) {
            s.ui.setPanel(Panel.Crop);
          }
          handleAutoStraighten();
        },
      },
      brush_size_up: {
        shouldFire: (s: any) =>
          !!s.editor.selectedImage && (s.ui.activePanel === Panel.Masks || s.ui.activePanel === Panel.Ai),
        execute: (e: any, s: any) => {
          e.preventDefault();
          const currentSettings = s.editor.brushSettings || { size: 50 };
          const newSize = Math.min((currentSettings.size || 50) + 10, 200);
          s.editor.setEditor({
            brushSettings: { ...currentSettings, size: newSize },
          });
        },
      },
      brush_size_down: {
        shouldFire: (s: any) =>
          !!s.editor.selectedImage && (s.ui.activePanel === Panel.Masks || s.ui.activePanel === Panel.Ai),
        execute: (e: any, s: any) => {
          e.preventDefault();
          const currentSettings = s.editor.brushSettings || { size: 50 };
          const newSize = Math.max((currentSettings.size || 50) - 10, 1);
          s.editor.setEditor({
            brushSettings: { ...currentSettings, size: newSize },
          });
        },
      },
    };

    const builtinShortcuts = [
      {
        match: (e: KeyboardEvent) => e.code === 'Escape',
        execute: (e: KeyboardEvent, s: any) => {
          e.preventDefault();
          if (s.editor.isStraightenActive) s.editor.setEditor({ isStraightenActive: false });
          else if (s.ui.customEscapeHandler) s.ui.customEscapeHandler();
          else if (s.editor.activeAiSubMaskId) s.editor.setEditor({ activeAiSubMaskId: null });
          else if (s.editor.activeAiPatchContainerId) s.editor.setEditor({ activeAiPatchContainerId: null });
          else if (s.editor.activeMaskId) s.editor.setEditor({ activeMaskId: null });
          else if (s.editor.activeMaskContainerId) s.editor.setEditor({ activeMaskContainerId: null });
          else if (s.ui.activePanel === Panel.Crop) s.ui.setPanel(Panel.Adjustments);
          else if (s.ui.isFullScreen) s.ui.toggleFullScreen();
        },
      },
      {
        match: (e: KeyboardEvent, s: any) => {
          const isDeleteKey = s.settings.osPlatform === 'macos' ? e.code === 'Backspace' : e.code === 'Delete';
          return isDeleteKey && (!!s.editor.activeMaskContainerId || !!s.editor.activeAiPatchContainerId);
        },
        execute: (e: KeyboardEvent, s: any) => {
          e.preventDefault();
          if (s.editor.activeMaskContainerId) {
            s.editor.setEditor((state: any) => ({
              adjustments: {
                ...state.adjustments,
                masks: state.adjustments.masks.filter((c: any) => c.id !== s.editor.activeMaskContainerId),
              },
              activeMaskContainerId: null,
              activeMaskId: null,
            }));
          } else if (s.editor.activeAiPatchContainerId) {
            s.editor.setEditor((state: any) => ({
              adjustments: {
                ...state.adjustments,
                aiPatches: state.adjustments.aiPatches.filter((c: any) => c.id !== s.editor.activeAiPatchContainerId),
              },
              activeAiPatchContainerId: null,
              activeAiSubMaskId: null,
            }));
          }
        },
      },
    ];

    const handleKeyDown = (event: KeyboardEvent) => {
      const state = getStoreState();

      const isModalOpen = state.ui.isCopyPasteSettingsModalOpen || state.ui.confirmModalState.isOpen;
      if (isModalOpen) return;

      const isInputFocused =
        document.activeElement?.tagName === 'INPUT' || document.activeElement?.tagName === 'TEXTAREA';
      if (isInputFocused) return;

      if (event.code === 'Escape') {
        builtinShortcuts[0].execute(event, state);
        return;
      }
      if (builtinShortcuts[1].match(event, state)) {
        builtinShortcuts[1].execute(event, state);
        return;
      }

      const normalized = normalizeCombo(event, state.settings.osPlatform);
      const action = comboMap.get(normalized.join('+'));

      if (action) {
        const handler = actions[action];
        if (handler && (!handler.shouldFire || handler.shouldFire(state))) {
          handler.execute(event, state);
          return;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [
    handleOpenImage,
    handleZoomChange,
    handleRotate,
    handleAutoStraighten,
    handleCopyAdjustments,
    handlePasteAdjustments,
    toggleCompare,
  ]);
};