import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Channel, convertFileSrc, invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { Status } from '../components/ui/ExportImportProperties';
import { Invokes, WaveformData } from '../components/ui/AppProperties';
import { JobKind, JobStatus, useProcessStore } from '../store/useProcessStore';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';

const parseAnalyticsPacket = async (buffer: ArrayBuffer) => {
  const view = new DataView(buffer);
  let offset = 0;
  const readU32 = () => {
    const value = view.getUint32(offset, true);
    offset += 4;
    return value;
  };

  const pathLength = readU32();
  const path = new TextDecoder().decode(new Uint8Array(buffer, offset, pathLength));
  offset += pathLength;

  const bins = readU32();
  let histogram: { red: number[]; green: number[]; blue: number[]; luma: number[] } | null = null;
  if (bins > 0) {
    const readChannel = () => {
      const channel = new Array<number>(bins);
      for (let i = 0; i < bins; i++) {
        channel[i] = view.getFloat32(offset, true);
        offset += 4;
      }
      return channel;
    };
    histogram = { red: readChannel(), green: readChannel(), blue: readChannel(), luma: readChannel() };
  }

  const width = readU32();
  const height = readU32();
  let waveform: WaveformData | null = null;
  if (width > 0 && height > 0) {
    const readPlane = () => {
      const length = readU32();
      const plane = new Uint8ClampedArray(buffer, offset, length);
      offset += length;
      return length === width * height * 4 ? createImageBitmap(new ImageData(plane, width, height)) : null;
    };
    const [rgb, luma, parade, vectorscope] = await Promise.all([readPlane(), readPlane(), readPlane(), readPlane()]);
    waveform = { width, height, rgb, luma, parade, vectorscope };
  }

  return { path, histogram, waveform };
};

interface TauriListenerProps {
  markGenerated: (path: string) => void;
}

export function useTauriListeners({ markGenerated }: TauriListenerProps) {
  const refs = useRef({ markGenerated });
  useEffect(() => {
    refs.current = { markGenerated };
  });
  const thumbnailBuffer = useRef<Record<string, string>>({});
  const mediumThumbnailBuffer = useRef<Record<string, string>>({});
  const flushHandle = useRef<number | null>(null);

  useEffect(() => {
    let isEffectActive = true;
    const modelDownloadJobIds: Record<string, string> = {};

    const flushThumbnailBatch = () => {
      flushHandle.current = null;
      if (!isEffectActive) return;

      const pendingThumbs = thumbnailBuffer.current;
      const pendingMediumThumbs = mediumThumbnailBuffer.current;

      thumbnailBuffer.current = {};
      mediumThumbnailBuffer.current = {};

      if (Object.keys(pendingThumbs).length > 0) {
        useProcessStore.getState().setProcess((state) => ({
          thumbnails: { ...state.thumbnails, ...pendingThumbs },
          mediumThumbnails: { ...state.mediumThumbnails, ...pendingMediumThumbs },
        }));
      }
    };

    const scheduleFlush = () => {
      if (flushHandle.current !== null) return;
      flushHandle.current = requestAnimationFrame(flushThumbnailBatch);
    };

    const listeners = [
      listen('preview-update-uncropped', (event: any) => {
        if (isEffectActive) useEditorStore.getState().setEditor({ uncroppedAdjustedPreviewUrl: event.payload });
      }),
      listen('open-with-file', (event: any) => {
        if (isEffectActive) useProcessStore.getState().setProcess({ initialFileToOpen: event.payload as string });
      }),
      listen('external-edit-session', (event: any) => {
        if (isEffectActive) useProcessStore.getState().setProcess({ externalEditSession: event.payload });
      }),
      listen('thumbnail-generated', (event: any) => {
        if (!isEffectActive) return;
        const { path, thumbnailPath, previewPath, data } = event.payload;

        if (thumbnailPath && previewPath) {
          thumbnailBuffer.current[path] = convertFileSrc(thumbnailPath.replace(/\\/g, '/'));
          mediumThumbnailBuffer.current[path] = convertFileSrc(previewPath.replace(/\\/g, '/'));
          refs.current.markGenerated(path);
        } else if (data) {
          thumbnailBuffer.current[path] = data;
          mediumThumbnailBuffer.current[path] = data;
          refs.current.markGenerated(path);
        }
        if (thumbnailPath || data) {
          scheduleFlush();
        }
      }),
      listen<string>('ai-model-download-start', (event) => {
        if (isEffectActive)
          modelDownloadJobIds[event.payload] = useProcessStore
            .getState()
            .startJob(JobKind.ModelDownload, { detail: event.payload });
      }),
      listen<{ name: string; current: number; total: number }>('ai-model-download-progress', (event) => {
        const { name, current, total } = event.payload;
        const jobId = modelDownloadJobIds[name];
        if (isEffectActive && jobId) useProcessStore.getState().updateJob(jobId, { progress: { current, total } });
      }),
      listen<{ name: string; error: string | null }>('ai-model-download-finish', (event) => {
        if (!isEffectActive) return;
        const { name, error } = event.payload;
        const jobId = modelDownloadJobIds[name];
        delete modelDownloadJobIds[name];
        if (jobId)
          useProcessStore
            .getState()
            .updateJob(jobId, error ? { status: JobStatus.Error, error } : { status: JobStatus.Success });
      }),
      listen('batch-export-progress', (event: any) => {
        if (isEffectActive) useProcessStore.getState().setExportState({ progress: event.payload });
      }),
      listen('export-complete', () => {
        if (isEffectActive) useProcessStore.getState().setExportState({ status: Status.Success });
      }),
      listen('export-error', (event: any) => {
        if (isEffectActive)
          useProcessStore.getState().setExportState({
            status: Status.Error,
            errorMessage: typeof event.payload === 'string' ? event.payload : 'Unknown error',
          });
      }),
      listen('export-cancelling', () => {
        if (isEffectActive) useProcessStore.getState().setExportState({ status: Status.Cancelling });
      }),
      listen('export-cancelled', () => {
        if (isEffectActive) useProcessStore.getState().setExportState({ status: Status.Cancelled });
      }),
      listen<{ path: string; progress: number }>('ai-denoise-progress', (event) => {
        if (!isEffectActive) return;
        const { path, progress } = event.payload;
        useEditorStore
          .getState()
          .setEditor((state) => ({ aiDenoiseProgress: { ...state.aiDenoiseProgress, [path]: progress } }));
      }),
      listen<string>('ai-denoise-ready', (event) => {
        if (!isEffectActive) return;
        useEditorStore.getState().setEditor((state) => {
          const { [event.payload]: _, ...aiDenoiseProgress } = state.aiDenoiseProgress;
          return {
            aiDenoiseProgress,
            ...(state.selectedImage?.path === event.payload ? { adjustments: { ...state.adjustments } } : {}),
          };
        });
      }),
      listen<{ path: string; error: string }>('ai-denoise-error', (event) => {
        if (!isEffectActive) return;
        const { path, error } = event.payload;
        toast.error(`AI denoise failed: ${error}`);
        useEditorStore.getState().setEditor((state) => {
          const { [path]: _, ...aiDenoiseProgress } = state.aiDenoiseProgress;
          return {
            aiDenoiseProgress,
            ...(state.selectedImage?.path === path
              ? { adjustments: { ...state.adjustments, aiDenoiseEnabled: false } }
              : {}),
          };
        });
      }),
      listen('wgpu-frame-ready', (event: any) => {
        const editorState = useEditorStore.getState();
        if (
          isEffectActive &&
          !editorState.hasRenderedFirstFrame &&
          event.payload?.path === editorState.selectedImage?.path
        ) {
          editorState.setEditor({ hasRenderedFirstFrame: true });
        }
      }),
      listen('focus-stack-progress', (event: any) => {
        if (isEffectActive) {
          useUIStore.getState().setUI((state) => {
            if (state.focusStackModalState.finalImageBase64 || state.focusStackModalState.error) return state;
            return { focusStackModalState: { ...state.focusStackModalState, progressMessage: event.payload } };
          });
        }
      }),
      listen('focus-stack-complete', (event: any) => {
        if (isEffectActive) {
          useUIStore.getState().setUI((state) => ({
            focusStackModalState: {
              ...state.focusStackModalState,
              error: null,
              finalImageBase64: event.payload.base64,
              depthMapBase64: event.payload.depthMap,
              isProcessing: false,
              progressMessage: null,
            },
          }));
        }
      }),
      listen('focus-stack-error', (event: any) => {
        if (isEffectActive) {
          useUIStore.getState().setUI((state) => ({
            focusStackModalState: {
              ...state.focusStackModalState,
              error: String(event.payload),
              finalImageBase64: null,
              depthMapBase64: null,
              isProcessing: false,
              progressMessage: null,
            },
          }));
        }
      }),
    ];

    let analyticsSequence = 0;
    let latestAppliedAnalytics = 0;
    const analyticsChannel = new Channel<ArrayBuffer>();
    analyticsChannel.onmessage = async (buffer) => {
      const sequence = ++analyticsSequence;
      const { path, histogram, waveform } = await parseAnalyticsPacket(buffer);
      if (!isEffectActive || sequence < latestAppliedAnalytics) return;
      if (path !== useEditorStore.getState().selectedImage?.path) return;
      latestAppliedAnalytics = sequence;
      const update: { histogram?: any; waveform?: WaveformData } = {};
      if (histogram) update.histogram = histogram;
      if (waveform) update.waveform = waveform;
      useEditorStore.getState().setEditor(update);
    };
    invoke(Invokes.SubscribeAnalytics, { channel: analyticsChannel }).catch((err) =>
      console.error('Failed to subscribe to analytics:', err),
    );

    return () => {
      isEffectActive = false;
      if (flushHandle.current !== null) {
        cancelAnimationFrame(flushHandle.current);
        flushHandle.current = null;
      }
      thumbnailBuffer.current = {};
      mediumThumbnailBuffer.current = {};
      listeners.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);
}
