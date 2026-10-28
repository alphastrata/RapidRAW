import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { toast } from 'react-toastify';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { INITIAL_ADJUSTMENTS, normalizeLoadedAdjustments } from '../utils/adjustments';
import { globalImageCache } from '../utils/ImageLRUCache';
import { debouncedSave, debouncedSetHistory } from './useEditorActions';

export interface AppNavigationProps {
  refs: {
    transformWrapperRef: React.RefObject<any>;
    cachedEditStateRef: React.RefObject<any>;
    selectedImagePathRef: React.RefObject<string | null>;
    isBackendReadyRef: React.RefObject<boolean>;
    latestRenderedJobIdRef: React.RefObject<number>;
    previewJobIdRef: React.RefObject<number>;
    currentResRef: React.RefObject<number>;
    prevAdjustmentsRef: React.RefObject<any>;
  };
}

let imageSelectGeneration = 0;

export function useAppNavigation({ refs }: AppNavigationProps) {
  const {
    transformWrapperRef,
    cachedEditStateRef,
    selectedImagePathRef,
    isBackendReadyRef,
    latestRenderedJobIdRef,
    previewJobIdRef,
    currentResRef,
    prevAdjustmentsRef,
  } = refs;

  const resetEditorTransform = () => {
    if (transformWrapperRef.current) {
      transformWrapperRef.current.resetTransform(0);
    }
    useEditorStore.getState().setEditor({
      zoom: 1,
      showOriginal: false,
      previewOverride: null,
    });
  };

  const handleOpenImage = useCallback(async () => {
    try {
      const types: any = await invoke('get_supported_file_types').catch(() => null);
      const extensions: string[] = types
        ? [...(types.raw || []), ...(types.nonRaw || [])]
        : [];
      const selected = await open({
        multiple: false,
        filters: extensions.length ? [{ name: 'Images', extensions }] : undefined,
      });
      if (typeof selected === 'string') {
        useUIStore.getState().imageSelectHandler?.(selected, true);
      }
    } catch (err) {
      console.error('Failed to open image dialog:', err);
      toast.error('Failed to open image selection dialog.');
    }
  }, []);

  const handleImageSelect = useCallback(
    async (path: string, _openInEditor: boolean = true) => {
      const generation = ++imageSelectGeneration;
      const { selectedImage, isSliderDragging, resetHistory, setEditor } = useEditorStore.getState();
      const { setUI } = useUIStore.getState();

      if (selectedImage?.path === path) return;

      useEditorStore.getState().patchesSentToBackend.clear();
      debouncedSave.flush();
      debouncedSetHistory.cancel();

      if (selectedImage?.path && cachedEditStateRef.current) {
        globalImageCache.set(selectedImage.path, cachedEditStateRef.current);
      }

      resetEditorTransform();

      const cached = globalImageCache.get(path);
      const cachedEntry = cached && cached.selectedImage?.isReady ? cached : null;
      const isCachedInBackend = cachedEntry
        ? await invoke<boolean>('is_image_cached', { path }).catch(() => false)
        : false;

      if (generation !== imageSelectGeneration) return;

      const hasDifferentResolution =
        cached &&
        (useEditorStore.getState().originalSize.width !== cached.originalSize.width ||
          useEditorStore.getState().originalSize.height !== cached.originalSize.height);

      if (!isCachedInBackend || hasDifferentResolution) {
        setEditor({ hasRenderedFirstFrame: false });
      }

      selectedImagePathRef.current = path;

      setEditor({
        showOriginal: false,
        activeMaskId: null,
        activeMaskContainerId: null,
        activeAiPatchContainerId: null,
        activeAiSubMaskId: null,
        isWbPickerActive: false,
        isRelightPickerActive: false,
        activeRelightLightId: null,
        activeCenterMarker: null,
        previewOverride: null,
      });

      setUI({
        compactEditorPanelHeightOverride: null,
      });

      if (cachedEntry) {
        setEditor({
          selectedImage: {
            ...cachedEntry.selectedImage,
            thumbnailUrl: getThumbnailPath(path) ?? cachedEntry.selectedImage.thumbnailUrl,
          },
          originalSize: cachedEntry.originalSize,
          previewSize: cachedEntry.previewSize,
          histogram: cachedEntry.histogram,
          waveform: cachedEntry.waveform,
          finalPreviewUrl: cachedEntry.finalPreviewUrl,
          uncroppedAdjustedPreviewUrl: cachedEntry.uncroppedPreviewUrl,
        });

        setEditor({ adjustments: cachedEntry.adjustments });
        resetHistory(cachedEntry.adjustments);
        prevAdjustmentsRef.current = { path, adjustments: cachedEntry.adjustments };

        latestRenderedJobIdRef.current = previewJobIdRef.current;
        isBackendReadyRef.current = false;
        currentResRef.current = Infinity;

        invoke('load_image', { path })
          .then((_result: any) => {
            if (selectedImagePathRef.current !== path) return;
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
            setEditor({ originalSize: { width: _result.width, height: _result.height } });
          })
          .catch((err: any) => {
            if (String(err).includes('cancelled')) return;
            console.error('Background load_image failed on cache hit:', err);
            isBackendReadyRef.current = true;
            currentResRef.current = 0;
          });

        invoke('load_metadata', { path })
          .then((metadata: any) => {
            if (selectedImagePathRef.current !== path) return;
            let freshAdjustments: any;
            if (metadata.adjustments && !metadata.adjustments.is_null) {
              freshAdjustments = normalizeLoadedAdjustments(metadata.adjustments);
            } else {
              freshAdjustments = { ...INITIAL_ADJUSTMENTS };
            }
            if (!isSliderDragging && JSON.stringify(cachedEntry.adjustments) !== JSON.stringify(freshAdjustments)) {
              setEditor({ adjustments: freshAdjustments });
              resetHistory(freshAdjustments);
              prevAdjustmentsRef.current = { path, adjustments: freshAdjustments };
              globalImageCache.set(path, { ...cachedEntry, adjustments: freshAdjustments });
            }
          })
          .catch((err) => console.error('Failed background metadata sync on cache hit:', err));

        return;
      }

      isBackendReadyRef.current = true;

      setEditor({
        selectedImage: {
          exif: null,
          height: 0,
          isRaw: false,
          isReady: false,
          metadata: null,
          path,
          thumbnailUrl: getThumbnailPath(path) ?? null,
          width: 0,
        },
        originalSize: { width: 0, height: 0 },
        previewSize: { width: 0, height: 0 },
        histogram: null,
        waveform: null,
        uncroppedAdjustedPreviewUrl: null,
      });

      setEditor((state) => {
        const prev = state.finalPreviewUrl;
        if (prev?.startsWith('blob:') && !globalImageCache.isProtected(prev)) {
          setTimeout(() => {
            if (!globalImageCache.isProtected(prev)) {
              URL.revokeObjectURL(prev);
            }
          }, 250);
        }
        return { finalPreviewUrl: null };
      });

      setEditor((state) => {
        if (state.interactivePatch?.url) URL.revokeObjectURL(state.interactivePatch.url);
        return { interactivePatch: null };
      });
    },
    [refs],
  );

  return {
    handleOpenImage,
    handleImageSelect,
  };
}

function getThumbnailPath(path: string): string | undefined {
  const state = useProcessStore.getState();
  return state.mediumThumbnails?.[path] || state.thumbnails?.[path];
}