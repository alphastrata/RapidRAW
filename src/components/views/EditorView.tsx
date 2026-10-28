import { type RefObject, type PointerEvent as ReactPointerEvent } from 'react';
import { useShallow } from 'zustand/react/shallow';
import clsx from 'clsx';
import { motion, AnimatePresence } from 'framer-motion';

import Editor from '../panel/Editor';
import BottomBar from '../panel/BottomBar';
import Resizer from '../ui/Resizer';
import { MobilePanelSwitcher } from '../panel/PanelSwitcher';

import { useEditorStore } from '../../store/useEditorStore';
import { useUIStore } from '../../store/useUIStore';
import { useProcessStore } from '../../store/useProcessStore';

import { Orientation } from '../ui/AppProperties';

interface EditorViewProps {
  transformWrapperRef: RefObject<any>;
  isResizing: boolean;
  layoutMode: 'compact' | 'wide' | 'full';
  compactEditorPanelHeight: number;
  compactEditorPanelCollapsedHeight: number;
  createResizeHandler: (stateKey: string, startSize: number) => (e: ReactPointerEvent<HTMLDivElement>) => void;
  createResizeResetHandler: (stateKey: string) => () => void;
  handleCopyAdjustments: () => void;
  handlePasteAdjustments: () => void;
  handleZoomChange: (zoom: number) => void;
  handlePanelSelect: (panelId: any) => void;
  handleEditorContextMenu: (event: any) => void;
  renderAppPanel: (panelId: any) => React.ReactNode;
}

export default function EditorView({
  transformWrapperRef,
  isResizing,
  layoutMode,
  compactEditorPanelHeight,
  compactEditorPanelCollapsedHeight,
  createResizeHandler,
  createResizeResetHandler,
  handleCopyAdjustments,
  handlePasteAdjustments,
  handleZoomChange,
  handlePanelSelect,
  handleEditorContextMenu,
  renderAppPanel,
}: EditorViewProps) {
  const { selectedImage } = useEditorStore(
    useShallow((state) => ({
      selectedImage: state.selectedImage,
    })),
  );

  const { isFullScreen, isInstantTransition, activePanel, setUI } = useUIStore(
    useShallow((state) => ({
      isFullScreen: state.isFullScreen,
      isInstantTransition: state.isInstantTransition,
      activePanel: state.activePanel,
      setUI: state.setUI,
    })),
  );

  const { isCopied, isPasted } = useProcessStore(
    useShallow((state) => ({
      isCopied: state.isCopied,
      isPasted: state.isPasted,
    })),
  );

  const editorNode = <Editor onContextMenu={handleEditorContextMenu} transformWrapperRef={transformWrapperRef} />;

  const editorBottomBarComponent = (
    <BottomBar
      isCopied={isCopied}
      isCopyDisabled={!selectedImage}
      isPasted={isPasted}
      isPasteDisabled={useEditorStore.getState().copiedAdjustments === null}
      onCopy={handleCopyAdjustments}
      onOpenCopyPasteSettings={() => setUI({ isCopyPasteSettingsModalOpen: true })}
      onPaste={() => handlePasteAdjustments()}
      onZoomChange={handleZoomChange}
      selectedImage={selectedImage ?? undefined}
      showZoomControls={true}
    />
  );

  const editorBottomBarNode = (
    <div
      className={clsx(
        'flex flex-col w-full overflow-hidden shrink-0',
        !isResizing && !isInstantTransition && 'transition-all duration-300 ease-in-out',
      )}
      style={{
        maxHeight: isFullScreen ? '0px' : '500px',
        opacity: isFullScreen ? 0 : 1,
      }}
    >
      {editorBottomBarComponent}
    </div>
  );

  const editorMobilePanelNode =
    layoutMode === 'compact' ? (
      <div
        className={clsx(
          'flex overflow-hidden shrink-0 flex-col bg-bg-secondary rounded-lg',
          !isResizing && !isInstantTransition && 'transition-all duration-300 ease-in-out',
        )}
        style={{
          height: isFullScreen ? 0 : activePanel ? compactEditorPanelHeight : compactEditorPanelCollapsedHeight,
          opacity: isFullScreen ? 0 : 1,
        }}
      >
        {activePanel && !isFullScreen && (
          <Resizer
            direction={Orientation.Horizontal}
            onMouseDown={createResizeHandler('compact', compactEditorPanelHeight)}
            onDoubleClick={createResizeResetHandler('compact')}
          />
        )}
        <div className="min-h-0 flex-1 overflow-hidden relative">
          <AnimatePresence mode="wait">
            {activePanel && (
              <motion.div
                key={activePanel}
                initial={{ opacity: 0, y: 10 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -10 }}
                transition={{ duration: 0.15 }}
                className="absolute inset-0 overflow-y-auto custom-scrollbar"
              >
                {renderAppPanel(activePanel)}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
        <MobilePanelSwitcher activePanel={activePanel} onPanelSelect={handlePanelSelect} />
        <div className="shrink-0 border-t border-surface">{editorBottomBarComponent}</div>
      </div>
    ) : null;

  return (
    <div className={clsx('flex grow h-full min-h-0', layoutMode === 'compact' ? 'flex-col gap-2' : 'flex-col')}>
      <div className={clsx('flex-1 flex flex-col min-w-0', layoutMode === 'compact' && 'min-h-0')}>{editorNode}</div>
      {layoutMode === 'compact' ? editorMobilePanelNode : editorBottomBarNode}
    </div>
  );
}