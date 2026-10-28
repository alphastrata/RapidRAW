import { type PointerEvent as ReactPointerEvent, useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ToastContainer, Slide } from 'react-toastify';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  MeasuringStrategy,
  pointerWithin,
} from '@dnd-kit/core';
import clsx from 'clsx';

import TitleBar from './window/TitleBar';
import GlobalTooltip from './components/ui/GlobalTooltip';
import AppModals from './components/modals/AppModals';

import SidePanelArea from './components/panel/SidePanelArea';
import { PANEL_ICONS } from './components/panel/PanelSwitcher';
import Controls from './components/panel/right/ControlsPanel';
import CropPanel from './components/panel/right/CropPanel';
import MasksPanel from './components/panel/right/MasksPanel';
import AIPanel from './components/panel/right/AIPanel';

import EditorView from './components/views/EditorView';
import ExportPanel from './components/panel/right/ExportPanel';
import ExternalEditBar from './components/ui/ExternalEditBar';

import { ContextMenuProvider } from './context/ContextMenuContext';
import { useSettingsStore } from './store/useSettingsStore';
import { DEFAULT_PANEL_WIDTH, useUIStore } from './store/useUIStore';
import { useEditorStore } from './store/useEditorStore';
import { useProcessStore } from './store/useProcessStore';
import { useShallow } from 'zustand/react/shallow';

import { ImageDimensions } from './hooks/useImageRenderSize';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { useTauriListeners } from './hooks/useTauriListeners';
import { useAppContextMenus } from './hooks/useAppContextMenus';
import { useAppNavigation } from './hooks/useAppNavigation';
import { useExternalEditSession } from './hooks/useExternalEditSession';

import { useEditorActions } from './hooks/useEditorActions';
import { useProductivityActions } from './hooks/useProductivityActions';

import { useAppInitialization } from './hooks/useAppInitialization';
import './i18n';

import { Invokes, Panel, PanelRegion, Theme } from './components/ui/AppProperties';
import { Status } from './components/ui/ExportImportProperties';

import ImageProcessingManager from './components/managers/ImageProcessingManager';
import ImageLoaderManager from './components/managers/ImageLoaderManager';

const DND_MEASURING = { droppable: { strategy: MeasuringStrategy.Always } };

function App() {
  const { appSettings, theme } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      theme: state.theme,
    })),
  );

  const {
    isFullScreen,
    isWindowFullScreen,
    isInstantTransition,
    uiVisibility,
    leftPanelWidth,
    rightPanelWidth,
    compactEditorPanelHeightOverride,
    activePanel,
    activeLayoutDragItem,
    setUI,
    setPanel,
    setLayoutDragItem,
    movePanel,
  } = useUIStore(
    useShallow((state) => ({
      isFullScreen: state.isFullScreen,
      isWindowFullScreen: state.isWindowFullScreen,
      isInstantTransition: state.isInstantTransition,
      uiVisibility: state.uiVisibility,
      leftPanelWidth: state.leftPanelWidth,
      rightPanelWidth: state.rightPanelWidth,
      compactEditorPanelHeightOverride: state.compactEditorPanelHeightOverride,
      activePanel: state.activePanel,
      activeLayoutDragItem: state.activeLayoutDragItem,
      setUI: state.setUI,
      setPanel: state.setPanel,
      setLayoutDragItem: state.setLayoutDragItem,
      movePanel: state.movePanel,
    })),
  );

  const { exportState, setExportState } = useProcessStore(
    useShallow((state) => ({
      exportState: state.exportState,
      setExportState: state.setExportState,
    })),
  );

  const { selectedImage, activeMaskContainerId, activeAiPatchContainerId, hasRenderedFirstFrame, setEditor } =
    useEditorStore(
      useShallow((state) => ({
        selectedImage: state.selectedImage,
        activeMaskContainerId: state.activeMaskContainerId,
        activeAiPatchContainerId: state.activeAiPatchContainerId,
        hasRenderedFirstFrame: state.hasRenderedFirstFrame,
        setEditor: state.setEditor,
      })),
    );

  const [viewportSize, setViewportSize] = useState<ImageDimensions>(() => {
    if (typeof window === 'undefined') {
      return { width: 0, height: 0 };
    }

    return {
      width: Math.round(window.visualViewport?.width ?? window.innerWidth),
      height: Math.round(window.visualViewport?.height ?? window.innerHeight),
    };
  });

  const isBackendReadyRef = useRef(true);
  const previewJobIdRef = useRef<number>(0);
  const latestRenderedJobIdRef = useRef<number>(0);
  const currentResRef = useRef<number>(1280);
  const cachedEditStateRef = useRef<any | null>(null);

  const prevAdjustmentsRef = useRef<any>(null);
  const selectedImagePathRef = useRef<string | null>(null);
  const transformWrapperRef = useRef<any>(null);
  useEffect(() => {
    selectedImagePathRef.current = selectedImage?.path ?? null;
  }, [selectedImage?.path]);

  useAppInitialization();

  const COMPACT_EDITOR_MAX_WIDTH = 900;

  const isPortraitViewport = viewportSize.width > 0 && viewportSize.height > viewportSize.width;

  type LayoutMode = 'compact' | 'wide' | 'full';
  const layoutMode: LayoutMode =
    isPortraitViewport && viewportSize.width > 0 && viewportSize.width <= COMPACT_EDITOR_MAX_WIDTH
      ? 'compact'
      : 'full';

  const useCompactPanels = layoutMode === 'compact';
  const compactEditorPanelMinHeight = 220;
  const compactEditorPanelMaxHeight =
    viewportSize.height > 0
      ? Math.max(compactEditorPanelMinHeight, Math.min(Math.round(viewportSize.height * 0.85), 850))
      : 520;

  const getDynamicCompactPanelHeight = () => {
    const { originalSize, adjustments } = useEditorStore.getState();
    const halfScreenHeight = viewportSize.height > 0 ? Math.round(viewportSize.height * 0.5) : 340;

    if (!selectedImage || originalSize.width === 0 || originalSize.height === 0 || viewportSize.width === 0) {
      return halfScreenHeight;
    }
    let effectiveRatio = originalSize.width / originalSize.height;
    const orientationSteps = adjustments?.orientationSteps || 0;
    if (orientationSteps % 2 !== 0) {
      effectiveRatio = originalSize.height / originalSize.width;
    }
    if (adjustments?.aspectRatio && adjustments.aspectRatio > 0) {
      effectiveRatio = adjustments.aspectRatio;
    }
    const desiredImageHeight = viewportSize.width / effectiveRatio;
    const topUiEstimation = !appSettings?.decorations && !isWindowFullScreen ? 110 : 60;
    const totalDesiredTopHeight = desiredImageHeight + topUiEstimation;
    const calculatedBottomHeight = Math.round(viewportSize.height - totalDesiredTopHeight);
    return Math.max(halfScreenHeight, calculatedBottomHeight);
  };

  const compactEditorPanelDefaultHeight = getDynamicCompactPanelHeight();
  const compactEditorPanelHeight = Math.max(
    compactEditorPanelMinHeight,
    Math.min(compactEditorPanelHeightOverride ?? compactEditorPanelDefaultHeight, compactEditorPanelMaxHeight),
  );
  const compactEditorPanelCollapsedHeight = 96;

  const { handleCopyAdjustments, handlePasteAdjustments, handleZoomChange } =
    useEditorActions();

  const navigationRefs = useMemo(
    () => ({
      transformWrapperRef,
      cachedEditStateRef,
      selectedImagePathRef,
      isBackendReadyRef,
      latestRenderedJobIdRef,
      previewJobIdRef,
      currentResRef,
      prevAdjustmentsRef,
    }),
    [],
  );

  const { handleOpenImage, handleImageSelect } = useAppNavigation({ refs: navigationRefs });

  const { externalEditSession, isFinishing: isExternalEditFinishing, finishExternalEdit } =
    useExternalEditSession(handleImageSelect);

  useEffect(() => {
    const { setImageSelectHandler } = useUIStore.getState();
    setImageSelectHandler(handleImageSelect);
    return () => setImageSelectHandler(null);
  }, [handleImageSelect]);

  const { handleStartFocusStack, handleSaveFocusStack } = useProductivityActions();

  const { handleEditorContextMenu } = useAppContextMenus();

  useTauriListeners({
    markGenerated: () => {},
  });

  useKeyboardShortcuts({
    handleOpenImage,
    handleZoomChange,
  });

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const updateViewportSize = () => {
      const nextViewportSize = {
        width: Math.round(window.visualViewport?.width ?? window.innerWidth),
        height: Math.round(window.visualViewport?.height ?? window.innerHeight),
      };

      setViewportSize((prev) =>
        prev.width === nextViewportSize.width && prev.height === nextViewportSize.height ? prev : nextViewportSize,
      );
    };

    updateViewportSize();

    window.addEventListener('resize', updateViewportSize);
    window.addEventListener('orientationchange', updateViewportSize);
    window.visualViewport?.addEventListener('resize', updateViewportSize);

    return () => {
      window.removeEventListener('resize', updateViewportSize);
      window.removeEventListener('orientationchange', updateViewportSize);
      window.visualViewport?.removeEventListener('resize', updateViewportSize);
    };
  }, []);

  useEffect(() => {
    const handleGlobalContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };
    window.addEventListener('contextmenu', handleGlobalContextMenu);
    return () => window.removeEventListener('contextmenu', handleGlobalContextMenu);
  }, []);

  const isLightTheme = useMemo(() => [Theme.Light, Theme.Snow, Theme.Arctic].includes(theme as Theme), [theme]);

  useEffect(() => {
    if (
      (activePanel !== Panel.Masks || !activeMaskContainerId) &&
      (activePanel !== Panel.Ai || !activeAiPatchContainerId)
    ) {
      setEditor({ isMaskControlHovered: false });
    }
  }, [activePanel, activeMaskContainerId, activeAiPatchContainerId, setEditor]);

  useEffect(() => {
    setEditor({
      isWbPickerActive: false,
      isRelightPickerActive: false,
      activeCenterMarker: null,
      isStraightenActive: false,
      isGuidedPerspectiveActive: false,
      activeMaskId: null,
      activeAiSubMaskId: null,
    });
  }, [activePanel, setEditor]);

  useEffect(() => {
    const unlisten = listen('ai-connector-status-update', (event: any) => {
      setEditor({ isAIConnectorConnected: event.payload.connected });
    });
    invoke(Invokes.CheckAIConnectorStatus);
    const interval = setInterval(() => invoke(Invokes.CheckAIConnectorStatus), 10000);
    return () => {
      clearInterval(interval);
      unlisten.then((f) => f());
    };
  }, [setEditor]);

  const createResizeHandler = (stateKey: string, startSize: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();

    const pointerId = e.pointerId;
    const target = e.currentTarget;
    const startX = e.clientX;
    const startY = e.clientY;

    const previousTouchAction = document.documentElement.style.touchAction;
    const previousUserSelect = document.documentElement.style.userSelect;

    target.setPointerCapture?.(pointerId);
    document.documentElement.style.touchAction = 'none';
    document.documentElement.style.userSelect = 'none';

    const doDrag = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      moveEvent.preventDefault();

      if (stateKey === 'left') {
        let w = startSize + (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, leftPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            leftPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, leftPanel: true },
          }));
        }
      } else if (stateKey === 'right') {
        let w = startSize - (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, rightPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            rightPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, rightPanel: true },
          }));
        }
      } else if (stateKey === 'compact') {
        setUI({
          compactEditorPanelHeightOverride: Math.round(
            Math.max(
              compactEditorPanelMinHeight,
              Math.min(startSize - (moveEvent.clientY - startY), compactEditorPanelMaxHeight),
            ),
          ),
        });
      }
    };

    const stopDrag = (upEvent: PointerEvent) => {
      if (upEvent.pointerId !== pointerId) return;
      if (target.hasPointerCapture?.(pointerId)) target.releasePointerCapture(pointerId);

      document.documentElement.style.cursor = '';
      document.documentElement.style.touchAction = previousTouchAction;
      document.documentElement.style.userSelect = previousUserSelect;

      window.removeEventListener('pointermove', doDrag);
      window.removeEventListener('pointerup', stopDrag);
      window.removeEventListener('pointercancel', stopDrag);
    };

    document.documentElement.style.cursor = stateKey === 'compact' ? 'row-resize' : 'col-resize';

    window.addEventListener('pointermove', doDrag, { passive: false });
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
  };

  const createResizeResetHandler = (stateKey: string) => () => {
    if (stateKey === 'left') {
      setUI((state) => ({
        leftPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, leftPanel: true },
      }));
    } else if (stateKey === 'right') {
      setUI((state) => ({
        rightPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, rightPanel: true },
      }));
    } else if (stateKey === 'compact') {
      setUI({ compactEditorPanelHeightOverride: null });
    }
  };

  useEffect(() => {
    const appWindow = getCurrentWindow();
    const checkFullscreen = async () => {
      setUI({ isWindowFullScreen: await appWindow.isFullscreen() });
    };
    checkFullscreen();
    const unlistenPromise = appWindow.onResized(checkFullscreen);
    return () => {
      unlistenPromise.then((unlisten: any) => unlisten());
    };
  }, [setUI]);

  const handlePanelSelect = useCallback(
    (panelId: Panel) => {
      setPanel(panelId);
    },
    [setPanel],
  );

  const renderAppPanel = useCallback(
    (panelId: Panel) => {
      switch (panelId) {
        case Panel.Export:
          return (
            <ExportPanel
              exportState={exportState}
              selectedImage={selectedImage}
              setExportState={setExportState}
              isVisible={true}
            />
          );
        case Panel.Adjustments:
          return <Controls />;
        case Panel.Crop:
          return <CropPanel />;
        case Panel.Masks:
          return <MasksPanel />;
        case Panel.Ai:
          return <AIPanel />;
        default:
          return null;
      }
    },
    [setExportState, selectedImage],
  );

  const isWgpuActive = appSettings?.useWgpuRenderer !== false && selectedImage?.isReady && hasRenderedFirstFrame;

  const layoutSensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));
  const handleDragStart = (e: any) => {
    if (e.active.data.current?.type === 'layout-tab') {
      setLayoutDragItem(e.active.data.current.panel as Panel);
    }
  };

  const handleDragEnd = (e: any) => {
    setLayoutDragItem(null);
    const { active, over } = e;

    if (active.data.current?.type === 'layout-tab' && over?.data.current?.type === 'layout-region') {
      movePanel(active.data.current.panel as Panel, over.data.current.region as PanelRegion);
    }
  };

  const ActiveOverlayIcon = activeLayoutDragItem ? PANEL_ICONS[activeLayoutDragItem] : null;
  const effectiveLeftWidth = uiVisibility.leftPanel ? leftPanelWidth : 48;
  const effectiveRightWidth = uiVisibility.rightPanel ? rightPanelWidth : 48;

  return (
    <>
      <ImageProcessingManager
        transformWrapperRef={transformWrapperRef}
        prevAdjustmentsRef={prevAdjustmentsRef}
        previewJobIdRef={previewJobIdRef}
        latestRenderedJobIdRef={latestRenderedJobIdRef}
        currentResRef={currentResRef}
      />
      <ImageLoaderManager cachedEditStateRef={cachedEditStateRef} />
      <div
        className={clsx(
          'flex flex-col h-screen font-sans text-text-primary overflow-hidden select-none',
          isWgpuActive ? 'bg-transparent' : 'bg-bg-primary',
        )}
      >
        <div
          className={clsx(
            'shrink-0 overflow-hidden z-50',
            !isInstantTransition && 'transition-all duration-300 ease-in-out',
            isFullScreen ? 'max-h-0 opacity-0 pointer-events-none' : 'max-h-15 opacity-100',
          )}
        >
          {appSettings?.decorations || (!isWindowFullScreen && <TitleBar />)}
        </div>
        <div
          className={clsx(
            'flex-1 flex flex-col min-h-0',
            !!selectedImage && !isInstantTransition && 'transition-all duration-300 ease-in-out',
            (!!selectedImage && (isFullScreen ? 'p-0 gap-0' : 'p-2 gap-2')) || '',
          )}
        >
          <DndContext
            sensors={layoutSensors}
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
            collisionDetection={pointerWithin}
            measuring={DND_MEASURING}
          >
            <div className="flex flex-row grow h-full min-h-0">
              {!useCompactPanels && (
                <SidePanelArea
                  side="left"
                  width={effectiveLeftWidth}
                  topRegion="leftTop"
                  bottomRegion="leftBottom"
                  renderPanel={renderAppPanel}
                  onWidthChange={createResizeHandler('left', effectiveLeftWidth)}
                  onWidthReset={createResizeResetHandler('left')}
                  isResizing={false}
                />
              )}
              <div className="relative flex-1 flex flex-col min-w-0">
                {selectedImage && externalEditSession && (
                  <ExternalEditBar
                    session={externalEditSession}
                    isFinishing={isExternalEditFinishing}
                    errorMessage={exportState.status === Status.Error ? exportState.errorMessage : ''}
                    onDone={finishExternalEdit}
                  />
                )}
                <div className="flex-1 flex flex-col min-w-0 h-full">
                  <EditorView
                    transformWrapperRef={transformWrapperRef}
                    isResizing={false}
                    layoutMode={layoutMode}
                    compactEditorPanelHeight={compactEditorPanelHeight}
                    compactEditorPanelCollapsedHeight={compactEditorPanelCollapsedHeight}
                    createResizeHandler={createResizeHandler}
                    createResizeResetHandler={createResizeResetHandler}
                    handleCopyAdjustments={handleCopyAdjustments}
                    handlePasteAdjustments={handlePasteAdjustments}
                    handleZoomChange={handleZoomChange}
                    handlePanelSelect={handlePanelSelect}
                    handleEditorContextMenu={handleEditorContextMenu}
                    renderAppPanel={renderAppPanel}
                  />
                </div>
              </div>
              {!useCompactPanels && (
                <SidePanelArea
                  side="right"
                  width={effectiveRightWidth}
                  topRegion="rightTop"
                  bottomRegion="rightBottom"
                  renderPanel={renderAppPanel}
                  onWidthChange={createResizeHandler('right', effectiveRightWidth)}
                  onWidthReset={createResizeResetHandler('right')}
                  isResizing={false}
                  showAdditionalTabs={true}
                />
              )}
            </div>
            <DragOverlay dropAnimation={null}>
              {activeLayoutDragItem && ActiveOverlayIcon ? (
                <div className="w-10 h-10 bg-surface shadow-2xl rounded-md flex items-center justify-center text-text-primary ring-1 ring-border-color">
                  <ActiveOverlayIcon size={20} />
                </div>
              ) : null}
            </DragOverlay>
          </DndContext>
        </div>
        <AppModals
          handleImageSelect={handleImageSelect}
          handleStartFocusStack={handleStartFocusStack}
          handleSaveFocusStack={handleSaveFocusStack}
        />
        <ToastContainer
          position="bottom-right"
          autoClose={5000}
          hideProgressBar={false}
          newestOnTop
          closeOnClick
          rtl={false}
          pauseOnFocusLoss
          draggable={false}
          pauseOnHover
          theme={isLightTheme ? 'light' : 'dark'}
          transition={Slide}
          toastClassName={() =>
            clsx(
              'relative flex min-h-16 p-4 rounded-lg justify-between overflow-hidden cursor-pointer mb-4',
              'bg-surface! text-text-primary! border! border-border-color! shadow-2xl! max-w-[420px]!',
            )
          }
        />
      </div>
    </>
  );
}

const AppWrapper = () => {
  return (
    <ContextMenuProvider>
      <App />
      <GlobalTooltip />
    </ContextMenuProvider>
  );
};

export default AppWrapper;