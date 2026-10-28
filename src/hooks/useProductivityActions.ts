import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useUIStore } from '../store/useUIStore';
import { Invokes } from '../components/ui/AppProperties';

export function useProductivityActions() {
  const setUI = useUIStore((state) => state.setUI);

  const handleStartFocusStack = useCallback(
    (paths: string[]) => {
      setUI((state) => ({
        focusStackModalState: {
          ...state.focusStackModalState,
          isProcessing: true,
          error: null,
          finalImageBase64: null,
          depthMapBase64: null,
          progressMessage: 'Starting focus stacking process...',
        },
      }));
      invoke(Invokes.StitchFocusStack, { paths }).catch((err) => {
        setUI((state) => ({
          focusStackModalState: { ...state.focusStackModalState, isProcessing: false, error: String(err) },
        }));
      });
    },
    [setUI],
  );

  const handleSaveFocusStack = useCallback(async (): Promise<string> => {
    const { focusStackModalState } = useUIStore.getState();
    if (focusStackModalState.sourcePaths.length === 0) {
      const err = 'Source paths for focus stack not found.';
      setUI((state) => ({ focusStackModalState: { ...state.focusStackModalState, error: err } }));
      throw new Error(err);
    }
    try {
      const savedPath: string = await invoke(Invokes.SaveFocusStack, {
        firstPathStr: focusStackModalState.sourcePaths[0],
      });
      return savedPath;
    } catch (err) {
      console.error('Failed to save focus stack:', err);
      setUI((state) => ({ focusStackModalState: { ...state.focusStackModalState, error: String(err) } }));
      throw err;
    }
  }, [setUI]);

  return {
    handleStartFocusStack,
    handleSaveFocusStack,
  };
}
