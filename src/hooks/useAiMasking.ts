import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { useEditorStore } from '../store/useEditorStore';
import { useProcessStore } from '../store/useProcessStore';
import { useEditorActions } from './useEditorActions';
import { Adjustments, AiPatch, MaskContainer } from '../utils/adjustments';
import { SubMask } from '../components/panel/right/Masks';
import { Invokes } from '../components/ui/AppProperties';

const getTransformAdjustments = (adj: Adjustments) => ({
  transformDistortion: adj.transformDistortion,
  transformVertical: adj.transformVertical,
  transformHorizontal: adj.transformHorizontal,
  transformRotate: adj.transformRotate,
  transformAspect: adj.transformAspect,
  transformScale: adj.transformScale,
  transformXOffset: adj.transformXOffset,
  transformYOffset: adj.transformYOffset,
  lensDistortionAmount: adj.lensDistortionAmount,
  lensVignetteAmount: adj.lensVignetteAmount,
  lensTcaAmount: adj.lensTcaAmount,
  lensDistortionParams: adj.lensDistortionParams,
  lensMaker: adj.lensMaker,
  lensModel: adj.lensModel,
  lensDistortionEnabled: adj.lensDistortionEnabled,
  lensTcaEnabled: adj.lensTcaEnabled,
  lensVignetteEnabled: adj.lensVignetteEnabled,
});

export function useAiMasking() {
  const { setAdjustments } = useEditorActions();
  const setEditor = useEditorStore((state) => state.setEditor);
  const registerAiTask = useProcessStore((state) => state.registerAiTask);
  const unregisterAiTask = useProcessStore((state) => state.unregisterAiTask);

  const updateSubMask = useCallback(
    (subMaskId: string, updatedData: any) => {
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        masks: prev.masks.map((c: MaskContainer) => ({
          ...c,
          subMasks: c.subMasks.map((sm: SubMask) => (sm.id === subMaskId ? { ...sm, ...updatedData } : sm)),
        })),
        aiPatches: (prev.aiPatches || []).map((p: AiPatch) => ({
          ...p,
          subMasks: p.subMasks.map((sm: SubMask) => (sm.id === subMaskId ? { ...sm, ...updatedData } : sm)),
        })),
      }));
    },
    [setAdjustments],
  );

  const handleCancelAiTask = useCallback(
    async (taskId: string) => {
      try {
        await invoke(Invokes.CancelAiTask, { taskId });
      } catch (e) {
        console.error('Failed to cancel AI task:', e);
      } finally {
        unregisterAiTask(taskId);
        setAdjustments((prev: Adjustments) => ({
          ...prev,
          aiPatches: (prev.aiPatches || []).map((p: AiPatch) =>
            p.id === taskId || p.subMasks.some((sm) => sm.id === taskId) ? { ...p, isLoading: false } : p,
          ),
        }));
      }
    },
    [unregisterAiTask, setAdjustments],
  );

  const handleDirectPatch = useCallback(
    async (subMaskId: string, sourceX: number, sourceY: number) => {
      const { selectedImage, adjustments, patchesSentToBackend } = useEditorStore.getState();
      if (!selectedImage?.path) return;

      const patchId = adjustments.aiPatches?.find((p: AiPatch) =>
        p.subMasks.some((sm: SubMask) => sm.id === subMaskId),
      )?.id;
      if (!patchId) return;

      patchesSentToBackend.add(patchId);
      setAdjustments((prev: Partial<Adjustments>) => ({
        ...prev,
        aiPatches: prev.aiPatches?.map((p: AiPatch) => (p.id === patchId ? { ...p, isLoading: true } : p)),
      }));

      try {
        const patchDefinitionForBackend = adjustments.aiPatches?.find((p: AiPatch) => p.id === patchId);
        const isLiquify = patchDefinitionForBackend?.subMasks.some((sm: SubMask) => sm.type === 'liquify');
        const isRetouch = patchDefinitionForBackend?.subMasks.some((sm: SubMask) => sm.type === 'retouch');
        const command = isLiquify
          ? 'generate_liquify_patch'
          : isRetouch
            ? 'generate_retouch_patch'
            : 'generate_manual_cleanup_patch';

        const newPatchDataJson: any = await invoke(command, {
          currentAdjustments: adjustments,
          patchDefinition: patchDefinitionForBackend,
          sourcePoint: [sourceX, sourceY],
        });

        const newPatchData = JSON.parse(newPatchDataJson);
        patchesSentToBackend.delete(patchId);

        setAdjustments((prev: Partial<Adjustments>) => ({
          ...prev,
          aiPatches: prev.aiPatches?.map((p: AiPatch) =>
            p.id === patchId ? { ...p, patchData: newPatchData, isLoading: false } : p,
          ),
        }));
      } catch (err: any) {
        patchesSentToBackend.delete(patchId);

        if (!String(err).includes('cancelled')) {
          toast.error(`Patch Generation Failed: ${err.message || String(err)}`);
        }
        setAdjustments((prev: Partial<Adjustments>) => ({
          ...prev,
          aiPatches: prev.aiPatches?.map((p: AiPatch) => (p.id === patchId ? { ...p, isLoading: false } : p)),
        }));
      }
    },
    [setAdjustments],
  );

  const handleGenerativeReplace = useCallback(
    async (patchId: string, prompt: string, useFastInpaint: boolean) => {
      const { selectedImage, adjustments, patchesSentToBackend } = useEditorStore.getState();
      if (!selectedImage?.path) return;

      const patch: AiPatch | undefined = adjustments.aiPatches?.find((p: AiPatch) => p.id === patchId);
      if (!patch) return;

      const patchDefinition = { ...patch, prompt };
      registerAiTask(patchId, 'inpaint');
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        aiPatches: prev.aiPatches.map((p: AiPatch) => (p.id === patchId ? { ...p, isLoading: true, prompt } : p)),
      }));

      try {
        const newPatchDataJson: any = await invoke(Invokes.InvokeGenerativeReplaceWithMaskDef, {
          currentAdjustments: adjustments,
          patchDefinition: patchDefinition,
          path: selectedImage.path,
          useFastInpaint: useFastInpaint,
          taskId: patchId,
        });

        const newPatchData = JSON.parse(newPatchDataJson);
        patchesSentToBackend.delete(patchId);

        setAdjustments((prev: Adjustments) => ({
          ...prev,
          aiPatches: prev.aiPatches.map((p: AiPatch) =>
            p.id === patchId
              ? {
                  ...p,
                  patchData: newPatchData,
                  isLoading: false,
                  name: useFastInpaint ? 'Inpaint' : prompt && prompt.trim() ? prompt.trim() : p.name,
                }
              : p,
          ),
        }));
        setEditor({ activeAiPatchContainerId: null, activeAiSubMaskId: null });
      } catch (err: any) {
        if (!String(err).includes('cancelled')) {
          toast.error(`AI Replace Failed: ${err}`);
        }
        setAdjustments((prev: Adjustments) => ({
          ...prev,
          aiPatches: prev.aiPatches.map((p: AiPatch) => (p.id === patchId ? { ...p, isLoading: false } : p)),
        }));
      } finally {
        unregisterAiTask(patchId);
      }
    },
    [setAdjustments, registerAiTask, unregisterAiTask, setEditor],
  );

  const handleDeleteMaskContainer = useCallback(
    (containerId: string) => {
      const { activeMaskContainerId } = useEditorStore.getState();
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        masks: (prev.masks || []).filter((c) => c.id !== containerId),
      }));
      if (activeMaskContainerId === containerId) {
        setEditor({ activeMaskContainerId: null, activeMaskId: null });
      }
    },
    [setAdjustments, setEditor],
  );

  const handleDeleteAiPatch = useCallback(
    (patchId: string) => {
      const { activeAiPatchContainerId } = useEditorStore.getState();
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        aiPatches: (prev.aiPatches || []).filter((p) => p.id !== patchId),
      }));
      if (activeAiPatchContainerId === patchId) {
        setEditor({ activeAiPatchContainerId: null, activeAiSubMaskId: null });
      }
    },
    [setAdjustments, setEditor],
  );

  const handleToggleAiPatchVisibility = useCallback(
    (patchId: string) => {
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        aiPatches: (prev.aiPatches || []).map((p: AiPatch) => (p.id === patchId ? { ...p, visible: !p.visible } : p)),
      }));
    },
    [setAdjustments],
  );

  const handleGenerateAiDepthMask = async (subMaskId: string, parameters: any) => {
    const { selectedImage, adjustments, patchesSentToBackend } = useEditorStore.getState();
    if (!selectedImage?.path) return;
    registerAiTask(subMaskId, 'depth');

    try {
      const transformAdjustments = getTransformAdjustments(adjustments);
      const newParameters: any = await invoke('generate_ai_depth_mask', {
        jsAdjustments: transformAdjustments,
        path: selectedImage.path,
        minDepth: parameters.minDepth ?? 20,
        maxDepth: parameters.maxDepth ?? 100,
        minFade: parameters.minFade ?? 15,
        maxFade: parameters.maxFade ?? 15,
        feather: parameters.feather ?? 10,
        flipHorizontal: adjustments.flipHorizontal,
        flipVertical: adjustments.flipVertical,
        orientationSteps: adjustments.orientationSteps,
        rotation: adjustments.rotation,
        taskId: subMaskId,
      });

      const subMask =
        adjustments.masks?.flatMap((m: MaskContainer) => m.subMasks).find((sm: SubMask) => sm.id === subMaskId) ||
        adjustments.aiPatches?.flatMap((p: AiPatch) => p.subMasks).find((sm: SubMask) => sm.id === subMaskId);

      const mergedParameters = { ...(subMask?.parameters || {}), ...newParameters };
      patchesSentToBackend.delete(subMaskId);
      updateSubMask(subMaskId, { parameters: mergedParameters });
    } catch (error) {
      if (!String(error).includes('cancelled')) {
        toast.error(`AI Depth Mask Failed: ${error}`);
      }
    } finally {
      unregisterAiTask(subMaskId);
    }
  };

  return {
    updateSubMask,
    handleGenerativeReplace,
    handleDirectPatch,
    handleDeleteMaskContainer,
    handleDeleteAiPatch,
    handleToggleAiPatchVisibility,
    handleGenerateAiDepthMask,
    handleCancelAiTask,
  };
}
