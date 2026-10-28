import { useShallow } from 'zustand/react/shallow';
import { useUIStore } from '../../store/useUIStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useProcessStore } from '../../store/useProcessStore';
import CopyPasteSettingsModal from './CopyPasteSettingsModal';
import FocusStackModal from './FocusStackModal';
import ConfirmModal from './ConfirmModal';
import { AppSettings } from '../ui/AppProperties';
import { CopyPasteSettings } from '../../utils/adjustments';

export interface AppModalsProps {
  handleImageSelect: (path: string) => void;
  handleStartFocusStack: (paths: string[]) => void;
  handleSaveFocusStack: () => Promise<string>;
}

export default function AppModals(props: AppModalsProps) {
  const { appSettings, handleSettingsChange } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      handleSettingsChange: state.handleSettingsChange,
    })),
  );

  const { isCopyPasteSettingsModalOpen, confirmModalState, focusStackModalState, setUI } = useUIStore(
    useShallow((state) => ({
      isCopyPasteSettingsModalOpen: state.isCopyPasteSettingsModalOpen,
      confirmModalState: state.confirmModalState,
      focusStackModalState: state.focusStackModalState,
      setUI: state.setUI,
    })),
  );

  const middlePath = (paths: string[]) => (paths.length > 0 ? paths[Math.floor(paths.length / 2)] : null);
  const focusStackLoadingPath = middlePath(focusStackModalState.sourcePaths);

  const { focusStackThumbnail } = useProcessStore(
    useShallow((state) => ({
      focusStackThumbnail: focusStackLoadingPath ? state.thumbnails[focusStackLoadingPath] : undefined,
    })),
  );

  const closeConfirmModal = () => {
    setUI((state) => ({ confirmModalState: { ...state.confirmModalState, isOpen: false } }));
  };

  return (
    <>
      <CopyPasteSettingsModal
        isOpen={isCopyPasteSettingsModalOpen}
        onClose={() => setUI({ isCopyPasteSettingsModalOpen: false })}
        settings={appSettings?.copyPasteSettings as CopyPasteSettings}
        onSave={(newSettings) =>
          handleSettingsChange({ ...appSettings, copyPasteSettings: newSettings } as AppSettings)
        }
      />
      <FocusStackModal
        error={focusStackModalState.error}
        finalImageBase64={focusStackModalState.finalImageBase64}
        imageCount={focusStackModalState.sourcePaths.length}
        isOpen={focusStackModalState.isOpen}
        isProcessing={focusStackModalState.isProcessing}
        loadingImageUrl={focusStackModalState.sourcePaths.length > 0 ? focusStackThumbnail || null : null}
        depthMapBase64={focusStackModalState.depthMapBase64}
        onClose={() =>
          setUI({
            focusStackModalState: {
              isOpen: false,
              isProcessing: false,
              progressMessage: '',
              finalImageBase64: null,
              depthMapBase64: null,
              error: null,
              sourcePaths: [],
            },
          })
        }
        onOpenFile={(path: string) => props.handleImageSelect(path)}
        onSave={props.handleSaveFocusStack}
        onMerge={() => props.handleStartFocusStack(focusStackModalState.sourcePaths)}
        progressMessage={focusStackModalState.progressMessage}
      />
      <ConfirmModal {...confirmModalState} onClose={closeConfirmModal} />
    </>
  );
}