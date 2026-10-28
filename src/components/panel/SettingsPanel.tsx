import { useEffect, useMemo, useState } from 'react';
import {
  ArrowLeft,
  Cpu,
  Unplug,
  ExternalLink as ExternalLinkIcon,
  Server,
  Info,
  Trash2,
  Wifi,
  WifiOff,
  Plus,
  X,
  SlidersHorizontal,
  Keyboard,
  Bookmark,
  Scaling,
  Image as ImageIcon,
  Mouse,
  Touchpad,
  ChevronDown,
  ChevronRight,
  Settings as SettingsIcon,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { relaunch } from '@tauri-apps/plugin-process';
import { motion, AnimatePresence, LayoutGroup } from 'framer-motion';
import clsx from 'clsx';
import Button from '../ui/Button';
import ConfirmModal from '../modals/ConfirmModal';
import Dropdown, { OptionItem } from '../ui/Dropdown';
import Switch from '../ui/Switch';
import Input from '../ui/Input';
import Slider from '../ui/Slider';
import { ThemeProps, THEMES, DEFAULT_THEME_ID } from '../../utils/themes';
import { useTranslation } from 'react-i18next';
import { Invokes } from '../ui/AppProperties';
import {
  formatKeyCode,
  getDefaultCombo,
  KeybindDefinition,
  KEYBIND_DEFINITIONS,
  KEYBIND_SECTIONS,
  normalizeCombo,
} from '../../utils/keyboardUtils';
import Text from '../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../types/typography';
import { useOsPlatform } from '../../hooks/useOsPlatform';
import { open } from '@tauri-apps/plugin-shell';
import DataManagementSettings from './settings/DataManagementSettings';

interface ConfirmModalState {
  confirmText: string;
  confirmVariant: string;
  isOpen: boolean;
  message: string;
  onConfirm(): void;
  title: string;
}

interface KeybindRowProps {
  def: KeybindDefinition;
  currentCombo?: string[];
  osPlatform: string;
  onSave: (action: string, combo: string[]) => void;
  recordingAction: string | null;
  onStartRecording: (action: string) => void;
  isConflicting: boolean;
}

interface SettingItemProps {
  children: any;
  description?: string;
  label: string;
}

interface SettingsPanelProps {
  appSettings: any;
  onBack(): void;
  onLibraryRefresh(): void;
  onSettingsChange(settings: any): Promise<void>;
  rootPaths: string[];
}

interface TestStatus {
  message: string;
  success: boolean | null;
  testing: boolean;
}

interface MyLens {
  maker: string;
  model: string;
}

const EXECUTE_TIMEOUT = 3000;
const DEFAULT_BASELINE_EXPOSURE = 0.7;

const resolutions: OptionItem<number>[] = [
  { value: 720, label: '720px' },
  { value: 1280, label: '1280px' },
  { value: 1920, label: '1920px' },
  { value: 2560, label: '2560px' },
  { value: 3840, label: '3840px' },
];

const smallThumbnailResolutions: OptionItem<number>[] = [
  { value: 240, label: '240px' },
  { value: 360, label: '360px' },
  { value: 480, label: '480px' },
  { value: 640, label: '640px' },
  { value: 720, label: '720px' },
];

const exportMemoryLimits: OptionItem<number>[] = [
  { value: 50, label: '50%' },
  { value: 65, label: '65%' },
  { value: 80, label: '80%' },
  { value: 90, label: '90%' },
];

const mediumThumbnailResolutions: OptionItem<number>[] = [
  { value: 720, label: '720px' },
  { value: 1024, label: '1024px' },
  { value: 1280, label: '1280px' },
  { value: 1440, label: '1440px' },
  { value: 1920, label: '1920px' },
];

const zoomMultiplierOptions: OptionItem<number>[] = [
  { value: 1.0, label: '1.0x (Native)' },
  { value: 0.75, label: '0.75x' },
  { value: 0.5, label: '0.50x (Half)' },
  { value: 0.25, label: '0.25x' },
];

const KeybindRow = ({
  def,
  currentCombo,
  osPlatform,
  onSave,
  recordingAction,
  onStartRecording,
  isConflicting,
}: KeybindRowProps) => {
  const { t } = useTranslation();
  const recording = recordingAction === def.action;

  useEffect(() => {
    if (!recording) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onSave(def.action, []);
        onStartRecording('');
        return;
      }
      e.preventDefault();
      const parts = normalizeCombo(e, osPlatform);
      if (parts.length > 0 && !['ctrl', 'shift', 'alt'].includes(parts[parts.length - 1])) {
        onSave(def.action, parts);
        onStartRecording('');
      }
    };
    window.addEventListener('keydown', handler, { capture: true });
    return () => window.removeEventListener('keydown', handler, { capture: true });
  }, [recording, def.action, onSave, onStartRecording]);

  const displayCombo =
    currentCombo !== undefined ? (currentCombo.length ? currentCombo : null) : getDefaultCombo(def, osPlatform);

  return (
    <div className="flex justify-between items-center py-2">
      <Text variant={TextVariants.label}>{t(def.description as any)}</Text>
      <div className="flex items-center gap-1">
        {isConflicting && <span className="text-yellow-400 text-xs">⚠</span>}
        <button onClick={() => onStartRecording(def.action)} className="flex items-center gap-1 flex-wrap shrink-0">
          {recording ? (
            <Text
              as="kbd"
              variant={TextVariants.small}
              color={TextColors.accent}
              weight={TextWeights.semibold}
              className="px-2 py-1 font-sans bg-bg-primary border border-accent rounded-md animate-pulse"
            >
              {t('settings.controls.pressKey')}
            </Text>
          ) : (
            <Text
              as="kbd"
              variant={TextVariants.small}
              color={TextColors.primary}
              weight={TextWeights.semibold}
              className={`px-2 py-1 font-sans bg-bg-primary border rounded-md cursor-pointer hover:border-accent transition-colors ${isConflicting ? 'border-yellow-400' : 'border-border-color'}`}
            >
              {displayCombo ? (
                displayCombo.map((k) => formatKeyCode(k, osPlatform)).join(' + ')
              ) : (
                <span className="text-text-secondary italic">{t('settings.controls.notAssigned')}</span>
              )}
            </Text>
          )}
        </button>
      </div>
    </div>
  );
};

const SettingItem = ({ children, description, label }: SettingItemProps) => (
  <div>
    <Text variant={TextVariants.heading} className="block mb-2">
      {label}
    </Text>
    {children}
    {description && (
      <Text variant={TextVariants.small} className="mt-2">
        {description}
      </Text>
    )}
  </div>
);

interface AiProviderSwitchProps {
  selectedProvider: string;
  onProviderChange: (provider: string) => void;
}

interface SegmentedSwitchItem<T extends string> {
  id: T;
  label: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
}

function SegmentedSwitch<T extends string>({
  items,
  selected,
  onChange,
  layoutId,
}: {
  items: SegmentedSwitchItem<T>[];
  selected: T;
  onChange: (val: T) => void;
  layoutId: string;
}) {
  return (
    <div className="relative flex w-full p-1 bg-bg-primary rounded-md border border-border-color">
      {items.map((item) => (
        <button
          key={item.id}
          onClick={() => onChange(item.id)}
          className={clsx(
            'relative flex-1 flex items-center justify-center gap-2 px-3 py-1.5 text-sm font-medium rounded-md transition-colors',
            {
              'text-text-primary hover:bg-surface': selected !== item.id,
              'text-button-text': selected === item.id,
            },
          )}
          style={{ WebkitTapHighlightColor: 'transparent' }}
        >
          {selected === item.id && (
            <motion.span
              layoutId={layoutId}
              className="absolute inset-0 z-0 bg-accent"
              style={{ borderRadius: 6 }}
              transition={{ type: 'spring', bounce: 0.2, duration: 0.6 }}
            />
          )}
          <span className="relative z-10 flex items-center">
            <item.icon size={16} className="mr-2" />
            {item.label}
          </span>
        </button>
      ))}
    </div>
  );
}

const AiProviderSwitch = ({ selectedProvider, onProviderChange }: AiProviderSwitchProps) => {
  const { t } = useTranslation();
  const aiProviders = useMemo(
    () => [
      { id: 'cpu', label: t('settings.processing.ai.providers.cpu'), icon: Cpu },
      { id: 'ai-connector', label: t('settings.processing.ai.providers.aiConnector'), icon: Server },
      { id: 'ai-free', label: t('settings.processing.ai.providers.aiFree'), icon: Unplug },
    ],
    [t],
  );
  return (
    <SegmentedSwitch
      items={aiProviders}
      selected={selectedProvider}
      onChange={onProviderChange}
      layoutId="ai-provider-switch-bubble"
    />
  );
};

interface CanvasInputModeSwitchProps {
  mode: 'mouse' | 'trackpad';
  onModeChange: (mode: 'mouse' | 'trackpad') => void;
}

const CanvasInputModeSwitch = ({ mode, onModeChange }: CanvasInputModeSwitchProps) => {
  const { t } = useTranslation();
  const canvasInputModes = useMemo<SegmentedSwitchItem<'mouse' | 'trackpad'>[]>(
    () => [
      { id: 'mouse', label: t('settings.controls.modes.mouse'), icon: Mouse },
      { id: 'trackpad', label: t('settings.controls.modes.trackpad'), icon: Touchpad },
    ],
    [t],
  );
  return (
    <SegmentedSwitch
      items={canvasInputModes}
      selected={mode}
      onChange={onModeChange}
      layoutId="canvas-input-mode-switch-bubble"
    />
  );
};

interface PreviewModeSwitchProps {
  mode: 'static' | 'dynamic';
  onModeChange: (mode: 'static' | 'dynamic') => void;
}

const PreviewModeSwitch = ({ mode, onModeChange }: PreviewModeSwitchProps) => {
  const { t } = useTranslation();
  const previewModes = useMemo<SegmentedSwitchItem<'static' | 'dynamic'>[]>(
    () => [
      { id: 'static', label: t('settings.processing.modes.static'), icon: ImageIcon },
      { id: 'dynamic', label: t('settings.processing.modes.dynamic'), icon: Scaling },
    ],
    [t],
  );
  return (
    <SegmentedSwitch
      items={previewModes}
      selected={mode}
      onChange={onModeChange}
      layoutId="preview-mode-switch-bubble"
    />
  );
};

export default function SettingsPanel({
  appSettings,
  onBack,
  onLibraryRefresh,
  onSettingsChange,
  rootPaths,
}: SettingsPanelProps) {
  const { t } = useTranslation();
  const [confirmModalState, setConfirmModalState] = useState<ConfirmModalState>({
    confirmText: t('settings.data.modals.confirmClear'),
    confirmVariant: 'primary',
    isOpen: false,
    message: '',
    onConfirm: () => {},
    title: '',
  });
  const [testStatus, setTestStatus] = useState<TestStatus>({ message: '', success: null, testing: false });
  const [hasInteractedWithLivePreview, setHasInteractedWithLivePreview] = useState(false);
  const [isAdvancedProcessingExpanded, setIsAdvancedProcessingExpanded] = useState(false);
  const [recordingAction, setRecordingAction] = useState<string | null>(null);

  const [aiProvider, setAiProvider] = useState(appSettings?.aiProvider || 'cpu');
  const [aiConnectorAddress, setAiConnectorAddress] = useState<string>(appSettings?.aiConnectorAddress || '');

  const [lensMakers, setLensMakers] = useState<string[]>([]);
  const [lensModels, setLensModels] = useState<string[]>([]);
  const [tempLensMaker, setTempLensMaker] = useState<string>('');
  const [tempLensModel, setTempLensModel] = useState<string>('');

  const osPlatform = useOsPlatform();
  const [processingSettings, setProcessingSettings] = useState({
    editorPreviewResolution: appSettings?.editorPreviewResolution || 1920,
    smallThumbnailResolution: appSettings?.smallThumbnailResolution || 480,
    mediumThumbnailResolution: appSettings?.mediumThumbnailResolution || 1280,
    rawHighlightCompression: appSettings?.rawHighlightCompression ?? 2.5,
    baselineExposure: appSettings?.baselineExposure ?? DEFAULT_BASELINE_EXPOSURE,
    processingBackend: appSettings?.processingBackend || 'auto',
    linuxGpuOptimization: appSettings?.linuxGpuOptimization ?? false,
    highResZoomMultiplier: appSettings?.highResZoomMultiplier || 1.0,
    useFullDpiRendering: appSettings?.useFullDpiRendering ?? false,
    useWgpuRenderer:
      appSettings?.useWgpuRenderer ?? (osPlatform === 'linux' || osPlatform === 'android' ? false : true),
    thumbnailWorkerThreads: appSettings?.thumbnailWorkerThreads ?? 4,
    imageCacheSize: appSettings?.imageCacheSize ?? 5,
    rawPreprocessingColorNr: appSettings?.rawPreprocessingColorNr ?? 0.5,
    rawPreprocessingSharpening: appSettings?.rawPreprocessingSharpening ?? 0.35,
    applyPreprocessingToNonRaws: appSettings?.applyPreprocessingToNonRaws ?? false,
    useAppleRaw9: appSettings?.useAppleRaw9 ?? false,
  });
  const [restartRequired, setRestartRequired] = useState(false);
  const [activeCategory, setActiveCategory] = useState('general');
  const [dpr, setDpr] = useState(() => (typeof window !== 'undefined' ? window.devicePixelRatio : 1));

  const settingCategories = useMemo(
    () => [
      { id: 'general', label: t('settings.categories.general'), icon: SlidersHorizontal },
      { id: 'processing', label: t('settings.categories.processing'), icon: Cpu },
      { id: 'shortcuts', label: t('settings.categories.shortcuts'), icon: Keyboard },
    ],
    [t],
  );

  const exportWorkerThreadOptions = useMemo<OptionItem<number>[]>(
    () => [
      { value: 0, label: t('settings.processing.advanced.auto') },
      ...[1, 2, 3, 4, 6, 8].map((threads) => ({ value: threads, label: String(threads) })),
    ],
    [t],
  );

  const livePreviewQualityOptions = useMemo<OptionItem<string>[]>(
    () => [
      { value: 'high', label: t('settings.processing.qualities.high') },
      { value: 'performance', label: t('settings.processing.qualities.performance') },
    ],
    [t],
  );

  const filteredBackendOptions = useMemo<OptionItem<string>[]>(() => {
    const rawOptions = [
      { value: 'auto', label: t('settings.processing.backends.auto') },
      { value: 'vulkan', label: t('settings.processing.backends.vulkan') },
      { value: 'dx12', label: t('settings.processing.backends.dx12') },
      { value: 'metal', label: t('settings.processing.backends.metal') },
      { value: 'gl', label: t('settings.processing.backends.gl') },
    ];
    return rawOptions.filter((opt) => {
      if (opt.value === 'metal' && osPlatform !== 'macos') return false;
      if (opt.value === 'dx12' && osPlatform === 'macos') return false;
      return true;
    });
  }, [t, osPlatform]);

  const linearRawOptions = useMemo<OptionItem<string>[]>(
    () => [
      { value: 'auto', label: t('settings.processing.preprocessing.linearOptions.auto') },
      { value: 'gamma', label: t('settings.processing.preprocessing.linearOptions.gamma') },
      { value: 'skip_calib', label: t('settings.processing.preprocessing.linearOptions.skip_calib') },
      { value: 'gamma_skip_calib', label: t('settings.processing.preprocessing.linearOptions.gamma_skip_calib') },
    ],
    [t],
  );

  const tonemapperOptions = useMemo<OptionItem<string>[]>(
    () => [
      { value: 'agx', label: t('settings.processing.preprocessing.tonemapperOptions.agx') },
      { value: 'basic', label: t('settings.processing.preprocessing.tonemapperOptions.basic') },
    ],
    [t],
  );

  const fontOptions = useMemo<OptionItem<string>[]>(
    () => [
      { value: 'poppins', label: t('settings.general.poppins') },
      { value: 'system', label: t('settings.general.system') },
    ],
    [t],
  );

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const updateDpr = () => setDpr(window.devicePixelRatio);

    const mediaQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    mediaQuery.addEventListener('change', updateDpr);

    window.addEventListener('resize', updateDpr);

    return () => {
      mediaQuery.removeEventListener('change', updateDpr);
      window.removeEventListener('resize', updateDpr);
    };
  }, []);


  useEffect(() => {
    if (appSettings?.aiConnectorAddress !== aiConnectorAddress) {
      setAiConnectorAddress(appSettings?.aiConnectorAddress || '');
    }
    if (appSettings?.aiProvider !== aiProvider) {
      setAiProvider(appSettings?.aiProvider || 'cpu');
    }
    setProcessingSettings({
      editorPreviewResolution: appSettings?.editorPreviewResolution || 1920,
      smallThumbnailResolution: appSettings?.smallThumbnailResolution || 480,
      mediumThumbnailResolution: appSettings?.mediumThumbnailResolution || 1280,
      rawHighlightCompression: appSettings?.rawHighlightCompression ?? 2.5,
      baselineExposure: appSettings?.baselineExposure ?? DEFAULT_BASELINE_EXPOSURE,
      processingBackend: appSettings?.processingBackend || 'auto',
      linuxGpuOptimization: appSettings?.linuxGpuOptimization ?? false,
      highResZoomMultiplier: appSettings?.highResZoomMultiplier || 1.0,
      useFullDpiRendering: appSettings?.useFullDpiRendering ?? false,
      useWgpuRenderer: appSettings?.useWgpuRenderer ?? true,
      thumbnailWorkerThreads: appSettings?.thumbnailWorkerThreads ?? 4,
      imageCacheSize: appSettings?.imageCacheSize ?? 5,
      rawPreprocessingColorNr: appSettings?.rawPreprocessingColorNr ?? 0.5,
      rawPreprocessingSharpening: appSettings?.rawPreprocessingSharpening ?? 0.35,
      applyPreprocessingToNonRaws: appSettings?.applyPreprocessingToNonRaws ?? false,
      useAppleRaw9: appSettings?.useAppleRaw9 ?? false,
    });
    setRestartRequired(false);
  }, [appSettings]);

  useEffect(() => {
    invoke<string[]>('get_lensfun_makers').then(setLensMakers).catch(console.error);
  }, []);

  const handleProcessingSettingChange = async (key: string, value: any) => {
    setProcessingSettings((prev) => ({ ...prev, [key]: value }));

    if (
      key === 'processingBackend' ||
      key === 'linuxGpuOptimization' ||
      key === 'useWgpuRenderer' ||
      key === 'thumbnailWorkerThreads'
    ) {
      setRestartRequired(true);
    } else {
      await onSettingsChange({ ...appSettings, [key]: value });
      if (
        key === 'rawHighlightCompression' ||
        key === 'rawPreprocessingColorNr' ||
        key === 'rawPreprocessingSharpening' ||
        key === 'applyPreprocessingToNonRaws' ||
        key === 'useAppleRaw9'
      ) {
        await invoke('clear_image_caches');
      }
    }
  };

  const handleSaveAndRelaunch = async () => {
    await onSettingsChange({
      ...appSettings,
      ...processingSettings,
    });
    await relaunch();
  };

  const handleProviderChange = (provider: string) => {
    setAiProvider(provider);
    onSettingsChange({ ...appSettings, aiProvider: provider });
  };

  const handlePreviewModeChange = (mode: 'static' | 'dynamic') => {
    const enableZoomHifi = mode === 'dynamic';
    onSettingsChange({ ...appSettings, enableZoomHifi });
  };

  const handleTempMakerChange = (maker: string) => {
    setTempLensMaker(maker);
    setTempLensModel('');
    setLensModels([]);
    if (maker) {
      invoke('get_lensfun_lenses_for_maker', { maker })
        .then((l: any) => setLensModels(l))
        .catch(console.error);
    }
  };

  const handleAddLens = () => {
    if (tempLensMaker && tempLensModel) {
      const currentLenses: MyLens[] = appSettings?.myLenses || [];
      if (!currentLenses.some((l) => l.maker === tempLensMaker && l.model === tempLensModel)) {
        const newLenses = [...currentLenses, { maker: tempLensMaker, model: tempLensModel }];

        newLenses.sort((a, b) => {
          const makerComp = a.maker.localeCompare(b.maker);
          if (makerComp !== 0) return makerComp;
          return a.model.localeCompare(b.model);
        });

        onSettingsChange({
          ...appSettings,
          myLenses: newLenses,
        });
        setTempLensMaker('');
        setTempLensModel('');
        setLensModels([]);
      }
    }
  };

  const handleRemoveLens = (index: number) => {
    const currentLenses: MyLens[] = appSettings?.myLenses || [];
    const newLenses = [...currentLenses];
    newLenses.splice(index, 1);
    onSettingsChange({ ...appSettings, myLenses: newLenses });
  };

  const effectiveRootPaths = rootPaths?.length > 0 ? rootPaths : appSettings?.rootFolders || [];

  const handleTestConnection = async () => {
    if (!aiConnectorAddress) {
      return;
    }
    setTestStatus({ testing: true, message: t('settings.processing.ai.connector.testing'), success: null });
    try {
      await invoke(Invokes.TestAIConnectorConnection, { address: aiConnectorAddress });
      setTestStatus({ testing: false, message: t('settings.processing.ai.connector.success'), success: true });
    } catch (err) {
      setTestStatus({ testing: false, message: t('settings.processing.ai.connector.failed'), success: false });
      console.error('AI Connector connection test failed:', err);
    } finally {
      setTimeout(() => setTestStatus({ testing: false, message: '', success: null }), EXECUTE_TIMEOUT);
    }
  };

  const closeConfirmModal = () => {
    setConfirmModalState({ ...confirmModalState, isOpen: false });
  };

  const handleKeybindSave = (action: string, combo: string[]) => {
    const newKeybinds = { ...(appSettings?.keybinds || {}), [action]: combo };
    onSettingsChange({ ...appSettings, keybinds: newKeybinds });
  };

  const conflictingKeys = useMemo(() => {
    const map = new Map<string, Set<string>>();
    const userKb = appSettings?.keybinds || {};
    for (const def of KEYBIND_DEFINITIONS) {
      const userCombo = userKb[def.action];
      const effective = userCombo?.length
        ? userCombo
        : userCombo === undefined
          ? getDefaultCombo(def, osPlatform)
          : null;
      if (!effective) continue;
      const key = effective.join('+');
      if (!map.has(key)) map.set(key, new Set());
      map.get(key)!.add(def.action);
    }
    const keys = new Set<string>();
    for (const [, actions] of map) {
      if (actions.size > 1) actions.forEach((k) => keys.add(k));
    }
    return keys;
  }, [appSettings?.keybinds, osPlatform]);

  return (
    <>
      <ConfirmModal {...confirmModalState} onClose={closeConfirmModal} />
      <LayoutGroup id="settings-panel">
        <div className="flex flex-col h-full w-full text-text-primary">
          <header className="shrink-0 flex flex-wrap items-center justify-between gap-y-4 mb-8 pt-4">
            <div className="flex items-center shrink-0">
              <Button
                className="mr-4 hover:bg-surface text-text-primary rounded-full"
                onClick={onBack}
                size="icon"
                variant="ghost"
                data-tooltip={t('settings.tooltips.goHome')}
              >
                <ArrowLeft />
              </Button>
              <Text variant={TextVariants.display} color={TextColors.accent} className="whitespace-nowrap">
                {t('settings.title')}
              </Text>
            </div>

            <div className="relative flex w-full min-[1200px]:w-112.5 p-2 bg-surface rounded-md">
              {settingCategories.map((category) => (
                <button
                  key={category.id}
                  onClick={() => setActiveCategory(category.id)}
                  className={clsx(
                    'relative flex-1 flex items-center justify-center gap-2 px-3 py-1.5 text-sm font-medium rounded-md transition-colors',
                    {
                      'text-text-primary hover:bg-surface': activeCategory !== category.id,
                      'text-button-text': activeCategory === category.id,
                    },
                  )}
                  style={{ WebkitTapHighlightColor: 'transparent' }}
                >
                  {activeCategory === category.id && (
                    <motion.span
                      layoutId="settings-category-switch-bubble"
                      className="absolute inset-0 z-0 bg-accent"
                      style={{ borderRadius: 6 }}
                      transition={{ type: 'spring', bounce: 0.2, duration: 0.6 }}
                    />
                  )}
                  <span className="relative z-10 flex items-center">
                    <category.icon size={16} className="mr-2 shrink-0" />
                    <span className="truncate">{category.label}</span>
                  </span>
                </button>
              ))}
            </div>
          </header>

          <div className="flex-1 overflow-y-auto overflow-x-hidden pr-2 -mr-2 custom-scrollbar">
            <AnimatePresence mode="wait">
              {activeCategory === 'general' && (
                <motion.div
                  key="general"
                  initial={{ opacity: 0, x: 10 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: -10 }}
                  transition={{ duration: 0.2 }}
                  className="space-y-10"
                >
                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.general.title')}
                    </Text>
                    <div className="space-y-8">
                      <SettingItem label={t('settings.general.theme')} description={t('settings.general.themeDesc')}>
                        <Dropdown
                          onChange={(value: any) => onSettingsChange({ ...appSettings, theme: value })}
                          options={THEMES.map((theme: ThemeProps) => ({
                            value: theme.id,
                            label: t(theme.name as any),
                          }))}
                          value={appSettings?.theme || DEFAULT_THEME_ID}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      <SettingItem label={t('settings.language')} description={t('settings.languageDesc')}>
                        <Dropdown
                          onChange={(value: any) => onSettingsChange({ ...appSettings, language: value })}
                          options={[
                            { value: 'en', label: 'English' },
                            { value: 'ca', label: 'Català' },
                            { value: 'de', label: 'Deutsch' },
                            { value: 'es', label: 'Español' },
                            { value: 'fr', label: 'Français' },
                            { value: 'it', label: 'Italiano' },
                            { value: 'nl', label: 'Nederlands' },
                            { value: 'pl', label: 'Polski' },
                            { value: 'pt', label: 'Português' },
                            { value: 'ru', label: 'Русский' },
                            { value: 'ja', label: '日本語' },
                            { value: 'ko', label: '한국어' },
                            { value: 'cs', label: 'Čeština' },
                            { value: 'zh-CN', label: '简体中文' },
                            { value: 'zh-TW', label: '繁體中文' },
                          ]}
                          value={appSettings?.language || 'en'}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      <div className="space-y-4">
                        <SettingItem
                          label={t('settings.general.xmpSync')}
                          description={t('settings.general.xmpSyncDesc')}
                        >
                          <Switch
                            checked={appSettings?.enableXmpSync ?? true}
                            id="enable-xmp-sync-toggle"
                            label={t('settings.general.enableXmpSync')}
                            onChange={(checked) => {
                              const newSettings = { ...appSettings, enableXmpSync: checked };
                              if (!checked) {
                                newSettings.createXmpIfMissing = false;
                              }
                              onSettingsChange(newSettings);
                            }}
                          />
                        </SettingItem>

                        <AnimatePresence initial={false}>
                          {(appSettings?.enableXmpSync ?? true) && (
                            <motion.div
                              initial={{ height: 0, opacity: 0 }}
                              animate={{ height: 'auto', opacity: 1 }}
                              exit={{ height: 0, opacity: 0 }}
                              transition={{ duration: 0.3, ease: 'easeInOut' }}
                              className="overflow-hidden"
                            >
                              <div className="pl-4 border-l-2 border-border-color ml-1">
                                <SettingItem
                                  label={t('settings.general.createXmp')}
                                  description={t('settings.general.createXmpDesc')}
                                >
                                  <Switch
                                    checked={appSettings?.createXmpIfMissing ?? false}
                                    id="create-xmp-missing-toggle"
                                    label={t('settings.general.createXmpMissing')}
                                    onChange={(checked) =>
                                      onSettingsChange({ ...appSettings, createXmpIfMissing: checked })
                                    }
                                  />
                                </SettingItem>
                              </div>
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>

                      <SettingItem
                        label={t('settings.general.folderImageCounts')}
                        description={t('settings.general.folderImageCountsDesc')}
                      >
                        <Switch
                          checked={appSettings?.enableFolderImageCounts ?? false}
                          id="folder-image-counts-toggle"
                          label={t('settings.general.showImageCounts')}
                          onChange={(checked) => onSettingsChange({ ...appSettings, enableFolderImageCounts: checked })}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.general.neutralGreyCanvas')}
                        description={t('settings.general.neutralGreyCanvasDesc')}
                      >
                        <Switch
                          checked={appSettings?.editorNeutralGreyBg ?? false}
                          id="neutral-grey-bg-toggle"
                          label={t('settings.general.enableNeutralGreyCanvas')}
                          onChange={(checked) => onSettingsChange({ ...appSettings, editorNeutralGreyBg: checked })}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.general.displayEditIcon')}
                        description={t('settings.general.displayEditIconDesc')}
                      >
                        <Switch
                          checked={appSettings?.displayEditIcon ?? true}
                          id="display-edit-icon-toggle"
                          label={t('settings.general.displayEditIcon')}
                          onChange={(checked) => onSettingsChange({ ...appSettings, displayEditIcon: checked })}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.general.focusMode')}
                        description={t('settings.general.focusModeDesc')}
                      >
                        <Switch
                          checked={appSettings?.enableFocusMode ?? false}
                          id="focus-mode-toggle"
                          label={t('settings.general.enableFocusMode')}
                          onChange={(checked) => onSettingsChange({ ...appSettings, enableFocusMode: checked })}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.general.toolFocusMode')}
                        description={t('settings.general.toolFocusModeDesc')}
                      >
                        <Switch
                          checked={appSettings?.enableToolFocusMode ?? false}
                          id="tool-focus-mode-toggle"
                          label={t('settings.general.enableToolFocusMode')}
                          onChange={(checked) => onSettingsChange({ ...appSettings, enableToolFocusMode: checked })}
                        />
                      </SettingItem>

                      <SettingItem label={t('settings.general.font')} description={t('settings.general.fontDesc')}>
                        <Dropdown
                          onChange={(value: any) => onSettingsChange({ ...appSettings, fontFamily: value })}
                          options={fontOptions}
                          value={appSettings?.fontFamily || 'poppins'}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      {osPlatform === 'linux' && (
                        <SettingItem
                          label={t('settings.general.nativeTitlebar')}
                          description={t('settings.general.nativeTitlebarDesc')}
                        >
                          <Switch
                            checked={appSettings?.decorations ?? false}
                            id="native-titlebar-toggle"
                            label={t('settings.general.enableOsTitlebar')}
                            onChange={(checked) => {
                              onSettingsChange({ ...appSettings, decorations: checked });
                              getCurrentWindow().setDecorations(checked).catch(console.error);
                            }}
                          />
                        </SettingItem>
                      )}
                    </div>
                  </div>

                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.processing.ai.title')}
                    </Text>
                    <Text className="mb-4">{t('settings.processing.ai.description')}</Text>

                    <AiProviderSwitch selectedProvider={aiProvider} onProviderChange={handleProviderChange} />

                    <div className="mt-8">
                      <AnimatePresence mode="wait">
                        {aiProvider === 'cpu' && (
                          <motion.div
                            key="cpu"
                            initial={{ opacity: 0, x: 10 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: -10 }}
                            transition={{ duration: 0.2 }}
                          >
                            <Text variant={TextVariants.heading}>{t('settings.processing.ai.cpu.title')}</Text>
                            <Text className="mt-1">{t('settings.processing.ai.cpu.description')}</Text>
                            <Text as="ul" className="mt-3 space-y-1 list-disc list-inside">
                              <li>{t('settings.processing.ai.cpu.feature1')}</li>
                              <li>{t('settings.processing.ai.cpu.feature2')}</li>
                              <li>{t('settings.processing.ai.cpu.feature3')}</li>
                            </Text>
                            {(osPlatform === 'windows' || osPlatform === 'macos') && (
                              <div className="mt-8">
                                <SettingItem
                                  label={t('settings.processing.ai.cpu.gpuAcceleration')}
                                  description={t('settings.processing.ai.cpu.gpuAccelerationDesc')}
                                >
                                  <Switch
                                    checked={appSettings?.aiGpuAcceleration ?? true}
                                    id="ai-gpu-acceleration-toggle"
                                    label={t('settings.processing.ai.cpu.gpuAccelerationLabel')}
                                    onChange={(checked) =>
                                      onSettingsChange({ ...appSettings, aiGpuAcceleration: checked })
                                    }
                                  />
                                </SettingItem>
                              </div>
                            )}
                          </motion.div>
                        )}

                        {aiProvider === 'ai-free' && (
                          <motion.div
                            key="ai-free"
                            initial={{ opacity: 0, x: 10 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: -10 }}
                            transition={{ duration: 0.2 }}
                          >
                            <Text variant={TextVariants.heading}>{t('settings.processing.ai.aiFree.title')}</Text>
                            <Text className="mt-1">{t('settings.processing.ai.aiFree.description')}</Text>
                            <Text as="ul" className="mt-3 space-y-1 list-disc list-inside">
                              <li>{t('settings.processing.ai.aiFree.feature1')}</li>
                              <li>{t('settings.processing.ai.aiFree.feature2')}</li>
                              <li>{t('settings.processing.ai.aiFree.feature3')}</li>
                            </Text>
                          </motion.div>
                        )}

                        {aiProvider === 'ai-connector' && (
                          <motion.div
                            key="ai-connector"
                            initial={{ opacity: 0, x: 10 }}
                            animate={{ opacity: 1, x: 0 }}
                            exit={{ opacity: 0, x: -10 }}
                            transition={{ duration: 0.2 }}
                          >
                            <div className="space-y-8">
                              <div>
                                <Text variant={TextVariants.heading}>
                                  {t('settings.processing.ai.connector.title')}
                                </Text>
                                <Text className="mt-1">{t('settings.processing.ai.connector.description')}</Text>
                                <Text as="ul" className="mt-3 space-y-1 list-disc list-inside">
                                  <li>{t('settings.processing.ai.connector.feature1')}</li>
                                  <li>{t('settings.processing.ai.connector.feature2')}</li>
                                  <li>{t('settings.processing.ai.connector.feature3')}</li>
                                </Text>
                              </div>
                              <SettingItem
                                label={t('settings.processing.ai.connector.address')}
                                description={t('settings.processing.ai.connector.addressDesc')}
                              >
                                <div className="flex items-center gap-2">
                                  <Input
                                    className="grow"
                                    id="ai-connector-address"
                                    onBlur={() =>
                                      onSettingsChange({ ...appSettings, aiConnectorAddress: aiConnectorAddress })
                                    }
                                    onChange={(e: any) => setAiConnectorAddress(e.target.value)}
                                    onKeyDown={(e: any) => e.stopPropagation()}
                                    placeholder="127.0.0.1:8188"
                                    type="text"
                                    value={aiConnectorAddress}
                                    bgClassName="bg-bg-primary"
                                  />
                                  <Button
                                    className="w-32"
                                    disabled={testStatus.testing || !aiConnectorAddress}
                                    onClick={handleTestConnection}
                                  >
                                    {testStatus.testing
                                      ? t('settings.processing.ai.connector.testing')
                                      : t('settings.processing.ai.connector.test')}
                                  </Button>
                                </div>
                                {testStatus.message && (
                                  <Text
                                    color={testStatus.success ? TextColors.success : TextColors.error}
                                    className="mt-2 flex items-center gap-2"
                                  >
                                    {testStatus.success === true && <Wifi size={16} />}
                                    {testStatus.success === false && <WifiOff size={16} />}
                                    {testStatus.message}
                                  </Text>
                                )}
                              </SettingItem>
                            </div>
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  </div>

                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.lenses.title')}
                    </Text>
                    <Text className="mb-6">{t('settings.lenses.description')}</Text>

                    <div className="space-y-8">
                      <div className="bg-bg-primary rounded-lg p-4 border border-border-color">
                        <Text variant={TextVariants.heading} className="mb-3">
                          {t('settings.lenses.addNew')}
                        </Text>
                        <div className="space-y-4">
                          <Dropdown
                            options={lensMakers.map((m) => ({ label: m, value: m }))}
                            value={tempLensMaker}
                            onChange={handleTempMakerChange}
                            placeholder={t('settings.lenses.manufacturerPlaceholder')}
                          />
                          <Dropdown
                            options={lensModels.map((m) => ({ label: m, value: m }))}
                            value={tempLensModel}
                            onChange={setTempLensModel}
                            placeholder={t('settings.lenses.modelPlaceholder')}
                            disabled={!tempLensMaker}
                          />
                          <Button
                            onClick={handleAddLens}
                            disabled={!tempLensMaker || !tempLensModel}
                            className="w-full"
                          >
                            <Plus size={16} className="mr-1" />
                            {t('settings.lenses.addButton')}
                          </Button>
                        </div>
                      </div>

                      <div>
                        <Text variant={TextVariants.heading} className="mb-2">
                          {t('settings.lenses.saved')}
                        </Text>
                        {(!appSettings?.myLenses || appSettings.myLenses.length === 0) && (
                          <Text className="italic">{t('settings.lenses.noLenses')}</Text>
                        )}
                        <div className="divide-y divide-border-color">
                          {(appSettings?.myLenses || []).map((lens: MyLens, index: number) => (
                            <div
                              key={`${lens.maker}-${lens.model}-${index}`}
                              className="flex justify-between items-center py-3 first:pt-0 last:pb-0"
                            >
                              <div className="flex items-center gap-3">
                                <div className="p-2 bg-surface rounded-md text-accent">
                                  <Bookmark size={16} />
                                </div>
                                <div>
                                  <Text color={TextColors.primary} weight={TextWeights.medium}>
                                    {lens.model}
                                  </Text>
                                  <Text variant={TextVariants.small}>{lens.maker}</Text>
                                </div>
                              </div>
                              <button
                                onClick={() => handleRemoveLens(index)}
                                className="p-2 text-text-secondary hover:text-red-400 hover:bg-bg-primary rounded-md transition-colors"
                                data-tooltip={t('settings.lenses.removeTooltip')}
                              >
                                <Trash2 size={16} />
                              </button>
                            </div>
                          ))}
                        </div>
                      </div>
                    </div>
                  </div>


                  


                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-4">
                      {t('settings.thanks.title')}
                    </Text>
                    <Text className="mb-4">{t('settings.thanks.description')}</Text>
                    <Text as="ul" className="space-y-3 list-disc ml-5 pl-1">
                      <li>
                        <a
                          href="https://github.com/dnglab/dnglab/tree/main/rawler"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          rawler
                        </a>
                        : {t('settings.thanks.list.rawler')}
                      </li>
                      <li>
                        <a
                          href="https://lensfun.github.io/"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          lensfun
                        </a>
                        : {t('settings.thanks.list.lensfun')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/andreavolpato/spektrafilm"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          spektrafilm
                        </a>
                        : {t('settings.thanks.list.spektrafilm')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/marcinz606/NegPy"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          NegPy
                        </a>
                        : {t('settings.thanks.list.negpy')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/advimman/lama"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          LaMa
                        </a>
                        : {t('settings.thanks.list.lama')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/facebookresearch/segment-anything"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          Segment Anything
                        </a>
                        : {t('settings.thanks.list.sam2')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/SimonZeng7108/efficientsam3"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          EfficientSAM3
                        </a>
                        : {t('settings.thanks.list.efficientsam3')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/ZhengPeng7/BiRefNet"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          BiRefNet
                        </a>
                        : {t('settings.thanks.list.birefnet')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/google-ai-edge/mediapipe"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          MediaPipe
                        </a>
                        : {t('settings.thanks.list.mediapipe')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/DepthAnything/Depth-Anything-V2"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          Depth Anything V2
                        </a>
                        : {t('settings.thanks.list.depth')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/trougnouf/nind-denoise"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          nind-denoise
                        </a>
                        : {t('settings.thanks.list.nind')}
                      </li>
                      <li>
                        <a
                          href="http://gphoto.org/"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          libgphoto2
                        </a>
                        : {t('settings.thanks.list.libgphoto2')}
                      </li>
                      <li>
                        <a
                          href="https://github.com/darktable-org/darktable"
                          target="_blank"
                          rel="noopener noreferrer"
                          className="font-semibold text-accent hover:underline"
                        >
                          darktable & co.
                        </a>
                        : {t('settings.thanks.list.darktable')}
                      </li>
                      <li>
                        <span className="font-semibold text-accent">{t('settings.thanks.list.youLabel')}</span>:{' '}
                        {t('settings.thanks.list.you')}
                      </li>
                    </Text>
                  </div>
                </motion.div>
              )}
              {activeCategory === 'processing' && (
                <motion.div
                  key="processing"
                  initial={{ opacity: 0, x: 10 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: -10 }}
                  transition={{ duration: 0.2 }}
                  className="space-y-10"
                >
                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.processing.title')}
                    </Text>
                    <div className="space-y-8">
                      <div>
                        <Text variant={TextVariants.heading} className="mb-2">
                          {t('settings.processing.previewStrategy')}
                        </Text>
                        <PreviewModeSwitch
                          mode={appSettings?.enableZoomHifi ? 'dynamic' : 'static'}
                          onModeChange={handlePreviewModeChange}
                        />

                        <div className="mt-3">
                          <AnimatePresence mode="wait">
                            {!(appSettings?.enableZoomHifi ?? true) ? (
                              <motion.div
                                key="static-preview"
                                initial={{ opacity: 0, x: 10 }}
                                animate={{ opacity: 1, x: 0 }}
                                exit={{ opacity: 0, x: -10 }}
                                transition={{ duration: 0.2 }}
                              >
                                <Text variant={TextVariants.small} className="mb-4">
                                  {t('settings.processing.staticDesc')}
                                </Text>
                                <div className="pl-4 border-l-2 border-border-color ml-1">
                                  <SettingItem
                                    description={t('settings.processing.previewResDesc')}
                                    label={t('settings.processing.previewRes')}
                                  >
                                    <Dropdown
                                      onChange={(value: any) =>
                                        handleProcessingSettingChange('editorPreviewResolution', value)
                                      }
                                      options={resolutions}
                                      value={processingSettings.editorPreviewResolution}
                                      triggerClassName="bg-bg-primary"
                                    />
                                  </SettingItem>
                                </div>
                              </motion.div>
                            ) : (
                              <motion.div
                                key="dynamic-preview"
                                initial={{ opacity: 0, x: 10 }}
                                animate={{ opacity: 1, x: 0 }}
                                exit={{ opacity: 0, x: -10 }}
                                transition={{ duration: 0.2 }}
                              >
                                <Text variant={TextVariants.small} className="mb-4">
                                  {t('settings.processing.dynamicDesc')}
                                </Text>
                                <div className="pl-4 border-l-2 border-border-color ml-1 space-y-3">
                                  <SettingItem
                                    description={t('settings.processing.staticPreviewResDesc')}
                                    label={t('settings.processing.staticPreviewRes')}
                                  >
                                    <Dropdown
                                      onChange={(value: any) =>
                                        handleProcessingSettingChange('editorPreviewResolution', value)
                                      }
                                      options={resolutions}
                                      value={processingSettings.editorPreviewResolution}
                                      triggerClassName="bg-bg-primary"
                                    />
                                  </SettingItem>

                                  <SettingItem
                                    label={t('settings.processing.renderScale')}
                                    description={t('settings.processing.renderScaleDesc')}
                                  >
                                    <Dropdown
                                      onChange={(value: any) =>
                                        handleProcessingSettingChange('highResZoomMultiplier', value)
                                      }
                                      options={zoomMultiplierOptions}
                                      value={processingSettings.highResZoomMultiplier}
                                      triggerClassName="bg-bg-primary"
                                    />
                                  </SettingItem>

                                  <SettingItem
                                    label={t('settings.processing.highDpi')}
                                    description={
                                      dpr > 1
                                        ? t('settings.processing.highDpiDesc', { dpr })
                                        : t('settings.processing.highDpiDescStandard')
                                    }
                                  >
                                    <Switch
                                      checked={processingSettings.useFullDpiRendering}
                                      disabled={dpr <= 1}
                                      id="full-dpi-rendering-toggle"
                                      label={t('settings.processing.nativeDpi')}
                                      onChange={(checked) =>
                                        handleProcessingSettingChange('useFullDpiRendering', checked)
                                      }
                                    />
                                  </SettingItem>
                                </div>
                              </motion.div>
                            )}
                          </AnimatePresence>
                        </div>
                      </div>

                      <div className="space-y-4">
                        <SettingItem
                          label={t('settings.processing.livePreviews')}
                          description={t('settings.processing.livePreviewsDesc')}
                        >
                          <Switch
                            checked={appSettings?.enableLivePreviews ?? true}
                            id="live-previews-toggle"
                            label={t('settings.processing.enableLivePreviews')}
                            onChange={(checked) => {
                              setHasInteractedWithLivePreview(true);
                              onSettingsChange({ ...appSettings, enableLivePreviews: checked });
                            }}
                          />
                        </SettingItem>

                        <AnimatePresence>
                          {(appSettings?.enableLivePreviews ?? true) && (
                            <motion.div
                              initial={hasInteractedWithLivePreview ? { height: 0, opacity: 0 } : false}
                              animate={{ height: 'auto', opacity: 1 }}
                              exit={{ height: 0, opacity: 0 }}
                              transition={{ duration: 0.3, ease: 'easeInOut' }}
                            >
                              <div className="pl-4 border-l-2 border-border-color ml-1">
                                <SettingItem
                                  label={t('settings.processing.livePreviewQuality')}
                                  description={t('settings.processing.livePreviewQualityDesc')}
                                >
                                  <Dropdown
                                    onChange={(value: any) =>
                                      onSettingsChange({ ...appSettings, livePreviewQuality: value })
                                    }
                                    options={livePreviewQualityOptions}
                                    value={appSettings?.livePreviewQuality === 'performance' ? 'performance' : 'high'}
                                    triggerClassName="bg-bg-primary"
                                  />
                                </SettingItem>
                              </div>
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>

                      <SettingItem
                        description={t('settings.processing.smallThumbnailResDesc')}
                        label={t('settings.processing.smallThumbnailRes')}
                      >
                        <Dropdown
                          onChange={(value: any) => handleProcessingSettingChange('smallThumbnailResolution', value)}
                          options={smallThumbnailResolutions}
                          value={processingSettings.smallThumbnailResolution}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      <SettingItem
                        description={t('settings.processing.mediumThumbnailResDesc')}
                        label={t('settings.processing.mediumThumbnailRes')}
                      >
                        <Dropdown
                          onChange={(value: any) => handleProcessingSettingChange('mediumThumbnailResolution', value)}
                          options={mediumThumbnailResolutions}
                          value={processingSettings.mediumThumbnailResolution}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.processing.alwaysDecodeRaw')}
                        description={t('settings.processing.alwaysDecodeRawDesc')}
                      >
                        <Switch
                          checked={appSettings?.alwaysDecodeRawThumbnails ?? false}
                          id="always-decode-raw-thumbnails-toggle"
                          label={t('settings.processing.alwaysDecodeRawLabel')}
                          onChange={(checked) => {
                            onSettingsChange({ ...appSettings, alwaysDecodeRawThumbnails: checked });
                          }}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.processing.wgpu')}
                        description={
                          osPlatform === 'linux'
                            ? t('settings.processing.wgpuDescLinux')
                            : osPlatform === 'android'
                              ? t('settings.processing.wgpuDescAndroid')
                              : t('settings.processing.wgpuDescRecommended')
                        }
                      >
                        <Switch
                          checked={processingSettings.useWgpuRenderer}
                          disabled={osPlatform === 'linux' || osPlatform === 'android'}
                          id="wgpu-renderer-toggle"
                          label={t('settings.processing.wgpuLabel')}
                          onChange={(checked) => handleProcessingSettingChange('useWgpuRenderer', checked)}
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.processing.backend')}
                        description={t('settings.processing.backendDesc')}
                      >
                        <Dropdown
                          onChange={(value: any) => handleProcessingSettingChange('processingBackend', value)}
                          options={filteredBackendOptions}
                          value={
                            filteredBackendOptions.some(
                              (option) => option.value === processingSettings.processingBackend,
                            )
                              ? processingSettings.processingBackend
                              : 'auto'
                          }
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      {osPlatform !== 'macos' && osPlatform !== 'windows' && (
                        <SettingItem
                          label={t('settings.processing.linuxCompat')}
                          description={t('settings.processing.linuxCompatDesc')}
                        >
                          <Switch
                            checked={processingSettings.linuxGpuOptimization}
                            id="gpu-compat-toggle"
                            label={t('settings.processing.linuxCompatLabel')}
                            onChange={(checked) => handleProcessingSettingChange('linuxGpuOptimization', checked)}
                          />
                        </SettingItem>
                      )}

                      <SettingItem
                        label={t('settings.processing.advanced.heading')}
                        description={t('settings.processing.advanced.headingDesc')}
                      >
                        <div className="bg-bg-primary rounded-xl overflow-hidden">
                          <button
                            onClick={() => setIsAdvancedProcessingExpanded((expanded) => !expanded)}
                            className="w-full flex items-center justify-between p-3.5 hover:bg-card-active transition-colors"
                          >
                            <Text
                              as="span"
                              variant={TextVariants.label}
                              color={TextColors.primary}
                              className="flex items-center gap-2"
                            >
                              <SettingsIcon size={16} /> {t('settings.processing.advanced.title')}
                            </Text>
                            <Text color={TextColors.secondary}>
                              {isAdvancedProcessingExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                            </Text>
                          </button>
                          <AnimatePresence initial={false}>
                            {isAdvancedProcessingExpanded && (
                              <motion.div
                                initial={{ height: 0, opacity: 0 }}
                                animate={{ height: 'auto', opacity: 1 }}
                                exit={{ height: 0, opacity: 0 }}
                                transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
                                className="overflow-hidden"
                              >
                                <div className="px-4 pb-4 pt-4 border-t border-surface/50 space-y-8">
                                  <SettingItem
                                    label={t('settings.processing.workerThreads')}
                                    description={t('settings.processing.workerThreadsDesc')}
                                  >
                                    <Slider
                                      label={t('settings.processing.threads')}
                                      min={2}
                                      max={10}
                                      step={1}
                                      value={processingSettings.thumbnailWorkerThreads}
                                      defaultValue={4}
                                      onChange={(e: any) =>
                                        handleProcessingSettingChange(
                                          'thumbnailWorkerThreads',
                                          parseInt(e.target.value),
                                        )
                                      }
                                      fillOrigin="min"
                                    />
                                  </SettingItem>

                                  <SettingItem
                                    label={t('settings.processing.imageCache')}
                                    description={t('settings.processing.imageCacheDesc')}
                                  >
                                    <Slider
                                      label={t('settings.processing.images')}
                                      min={2}
                                      max={10}
                                      step={1}
                                      value={processingSettings.imageCacheSize}
                                      defaultValue={5}
                                      onChange={(e: any) =>
                                        handleProcessingSettingChange('imageCacheSize', parseInt(e.target.value))
                                      }
                                      fillOrigin="min"
                                    />
                                  </SettingItem>

                                  <SettingItem
                                    label={t('settings.processing.advanced.exportThreads')}
                                    description={t('settings.processing.advanced.exportThreadsDesc')}
                                  >
                                    <Dropdown
                                      onChange={(value: any) =>
                                        onSettingsChange({ ...appSettings, exportWorkerThreads: value })
                                      }
                                      options={exportWorkerThreadOptions}
                                      value={appSettings?.exportWorkerThreads ?? 0}
                                      triggerClassName="bg-surface"
                                    />
                                  </SettingItem>

                                  <SettingItem
                                    label={t('settings.processing.advanced.exportMemoryLimit')}
                                    description={t('settings.processing.advanced.exportMemoryLimitDesc')}
                                  >
                                    <Dropdown
                                      onChange={(value: any) =>
                                        onSettingsChange({ ...appSettings, exportMemoryLimit: value })
                                      }
                                      options={exportMemoryLimits}
                                      value={appSettings?.exportMemoryLimit ?? 80}
                                      triggerClassName="bg-surface"
                                    />
                                  </SettingItem>
                                </div>
                              </motion.div>
                            )}
                          </AnimatePresence>
                        </div>
                      </SettingItem>

                      {restartRequired && (
                        <>
                          <Text
                            as="div"
                            color={TextColors.info}
                            className="p-3 bg-blue-900/10 border border-blue-500/50 rounded-lg flex items-center gap-3"
                          >
                            <Info size={18} />
                            <p>{t('settings.processing.restartRequired')}</p>
                          </Text>
                          <div className="flex justify-end">
                            <Button onClick={handleSaveAndRelaunch}>{t('settings.processing.saveRelaunch')}</Button>
                          </div>
                        </>
                      )}
                    </div>
                  </div>

                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.processing.preprocessing.title')}
                    </Text>
                    <div className="space-y-8">
                      {/* turn off highlights clipping setting for now - needs clean cleanup across other files.
                      <SettingItem
                        label={t('settings.processing.preprocessing.highlightRecovery')}
                        description={t('settings.processing.preprocessing.highlightRecoveryDesc')}
                      >
                        <Slider
                          label={t('settings.tagging.amount')}
                          min={1}
                          max={10}
                          step={0.1}
                          value={processingSettings.rawHighlightCompression}
                          defaultValue={2.5}
                          onChange={(e: any) =>
                            handleProcessingSettingChange('rawHighlightCompression', parseFloat(e.target.value))
                          }
                          fillOrigin="min"
                        />
                      </SettingItem>
                      */}

                      <SettingItem
                        label={t('settings.processing.preprocessing.colorNr')}
                        description={t('settings.processing.preprocessing.colorNrDesc')}
                      >
                        <Slider
                          label={t('settings.tagging.amount')}
                          min={0}
                          max={1.0}
                          step={0.05}
                          value={processingSettings.rawPreprocessingColorNr}
                          defaultValue={0.5}
                          onChange={(e: any) =>
                            handleProcessingSettingChange('rawPreprocessingColorNr', parseFloat(e.target.value))
                          }
                          fillOrigin="min"
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.processing.preprocessing.sharpening')}
                        description={t('settings.processing.preprocessing.sharpeningDesc')}
                      >
                        <Slider
                          label={t('settings.tagging.amount')}
                          min={0}
                          max={1.0}
                          step={0.05}
                          value={processingSettings.rawPreprocessingSharpening}
                          defaultValue={0.35}
                          onChange={(e: any) =>
                            handleProcessingSettingChange('rawPreprocessingSharpening', parseFloat(e.target.value))
                          }
                          fillOrigin="min"
                        />
                      </SettingItem>

                      <SettingItem
                        label={t('settings.processing.preprocessing.applyPreprocessing')}
                        description={t('settings.processing.preprocessing.applyPreprocessingDesc')}
                      >
                        <Switch
                          checked={processingSettings.applyPreprocessingToNonRaws}
                          id="preprocessing-non-raws-toggle"
                          label={t('settings.processing.preprocessing.enablePreprocessingNonRaws')}
                          onChange={(checked) => handleProcessingSettingChange('applyPreprocessingToNonRaws', checked)}
                        />
                      </SettingItem>

                      {osPlatform === 'macos' && (
                        <SettingItem
                          label={t('settings.processing.preprocessing.appleRaw9')}
                          description={t('settings.processing.preprocessing.appleRaw9Desc')}
                        >
                          <Switch
                            checked={processingSettings.useAppleRaw9}
                            id="apple-raw9-toggle"
                            label={t('settings.processing.preprocessing.enableAppleRaw9')}
                            onChange={(checked) => handleProcessingSettingChange('useAppleRaw9', checked)}
                          />
                        </SettingItem>
                      )}

                      <SettingItem
                        label={t('settings.processing.preprocessing.linearRaw')}
                        description={t('settings.processing.preprocessing.linearRawDesc')}
                      >
                        <Dropdown
                          onChange={(value: any) => onSettingsChange({ ...appSettings, linearRawMode: value })}
                          options={linearRawOptions}
                          value={appSettings?.linearRawMode || 'auto'}
                          triggerClassName="bg-bg-primary"
                        />
                      </SettingItem>

                      <div className="space-y-4">
                        <SettingItem
                          label={t('settings.processing.preprocessing.baselineExposure')}
                          description={t('settings.processing.preprocessing.baselineExposureDesc')}
                        >
                          <Switch
                            checked={appSettings?.enableBaselineExposure ?? false}
                            id="baseline-exposure-toggle"
                            label={t('settings.processing.preprocessing.enableBaselineExposure')}
                            onChange={(checked) =>
                              onSettingsChange({ ...appSettings, enableBaselineExposure: checked })
                            }
                          />
                        </SettingItem>

                        <AnimatePresence>
                          {(appSettings?.enableBaselineExposure ?? false) && (
                            <motion.div
                              initial={{ height: 0, opacity: 0 }}
                              animate={{ height: 'auto', opacity: 1 }}
                              exit={{ height: 0, opacity: 0 }}
                              transition={{ duration: 0.3, ease: 'easeInOut' }}
                            >
                              <div className="pl-4 border-l-2 border-border-color ml-1 space-y-3">
                                <SettingItem
                                  label={t('settings.processing.preprocessing.baselineExposureAmount')}
                                  description={t('settings.processing.preprocessing.baselineExposureAmountDesc')}
                                >
                                  <Slider
                                    label={t('settings.tagging.amount')}
                                    min={0}
                                    max={2}
                                    step={0.05}
                                    value={processingSettings.baselineExposure}
                                    defaultValue={DEFAULT_BASELINE_EXPOSURE}
                                    onChange={(e) =>
                                      handleProcessingSettingChange('baselineExposure', Number(e.target.value))
                                    }
                                    fillOrigin="min"
                                    suffix=" EV"
                                  />
                                </SettingItem>
                              </div>
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>

                      <SettingItem
                        label={t('settings.processing.preprocessing.highlightPreservation')}
                        description={t('settings.processing.preprocessing.highlightPreservationDesc')}
                      >
                        <Switch
                          checked={appSettings?.compensateHighlightPreservation ?? true}
                          id="highlight-preservation-toggle"
                          label={t('settings.processing.preprocessing.enableHighlightPreservation')}
                          onChange={(checked) =>
                            onSettingsChange({ ...appSettings, compensateHighlightPreservation: checked })
                          }
                        />
                      </SettingItem>

                      <div className="space-y-4">
                        <SettingItem
                          label={t('settings.processing.preprocessing.tonemapperOverride')}
                          description={t('settings.processing.preprocessing.tonemapperOverrideDesc')}
                        >
                          <Switch
                            checked={appSettings?.tonemapperOverrideEnabled ?? false}
                            id="tonemapper-override-toggle"
                            label={t('settings.processing.preprocessing.enableTonemapperOverride')}
                            onChange={(checked) =>
                              onSettingsChange({ ...appSettings, tonemapperOverrideEnabled: checked })
                            }
                          />
                        </SettingItem>

                        <AnimatePresence>
                          {(appSettings?.tonemapperOverrideEnabled ?? false) && (
                            <motion.div
                              initial={{ height: 0, opacity: 0 }}
                              animate={{ height: 'auto', opacity: 1 }}
                              exit={{ height: 0, opacity: 0 }}
                              transition={{ duration: 0.3, ease: 'easeInOut' }}
                            >
                              <div className="pl-4 border-l-2 border-border-color ml-1 space-y-3">
                                <SettingItem
                                  label={t('settings.processing.preprocessing.defaultRawTonemapper')}
                                  description={t('settings.processing.preprocessing.defaultRawTonemapperDesc')}
                                >
                                  <Dropdown
                                    onChange={(value: any) =>
                                      onSettingsChange({ ...appSettings, defaultRawTonemapper: value })
                                    }
                                    options={tonemapperOptions}
                                    value={appSettings?.defaultRawTonemapper || 'agx'}
                                    triggerClassName="bg-bg-primary"
                                  />
                                </SettingItem>

                                <SettingItem
                                  label={t('settings.processing.preprocessing.defaultNonRawTonemapper')}
                                  description={t('settings.processing.preprocessing.defaultNonRawTonemapperDesc')}
                                >
                                  <Dropdown
                                    onChange={(value: any) =>
                                      onSettingsChange({ ...appSettings, defaultNonRawTonemapper: value })
                                    }
                                    options={tonemapperOptions}
                                    value={appSettings?.defaultNonRawTonemapper || 'basic'}
                                    triggerClassName="bg-bg-primary"
                                  />
                                </SettingItem>
                              </div>
                            </motion.div>
                          )}
                        </AnimatePresence>
                      </div>
                    </div>
                  </div>

                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.data.title')}
                    </Text>
                    <DataManagementSettings
                      appSettings={appSettings}
                      onLibraryRefresh={onLibraryRefresh}
                      onRequestConfirm={(request) =>
                        setConfirmModalState({ ...request, confirmVariant: 'destructive', isOpen: true })
                      }
                      onSettingsChange={onSettingsChange}
                      rootPaths={effectiveRootPaths}
                    />
                  </div>
                </motion.div>
              )}

              {activeCategory === 'shortcuts' && (
                <motion.div
                  key="shortcuts"
                  initial={{ opacity: 0, x: 10 }}
                  animate={{ opacity: 1, x: 0 }}
                  exit={{ opacity: 0, x: -10 }}
                  transition={{ duration: 0.2 }}
                  className="space-y-10"
                >
                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.controls.title')}
                    </Text>
                    <div className="space-y-8">
                      <div>
                        <Text variant={TextVariants.heading} className="mb-2">
                          {t('settings.controls.optimization')}
                        </Text>
                        <Text variant={TextVariants.small} className="mb-4">
                          {t('settings.controls.optimizationDesc')}
                        </Text>
                        <CanvasInputModeSwitch
                          mode={(appSettings?.canvasInputMode as 'mouse' | 'trackpad') || 'mouse'}
                          onModeChange={(value) => onSettingsChange({ ...appSettings, canvasInputMode: value })}
                        />
                      </div>

                      <SettingItem label={t('settings.controls.zoom')} description={t('settings.controls.zoomDesc')}>
                        <Slider
                          label={t('settings.controls.speed')}
                          min={0.1}
                          max={3.0}
                          step={0.1}
                          value={appSettings?.zoomSpeedMultiplier ?? 1.0}
                          defaultValue={1.0}
                          onChange={(e: any) =>
                            onSettingsChange({ ...appSettings, zoomSpeedMultiplier: parseFloat(e.target.value) })
                          }
                          fillOrigin="min"
                        />
                      </SettingItem>
                      <SettingItem
                        label={t('settings.controls.zoomClick')}
                        description={t('settings.controls.zoomClickInfo')}
                      >
                        <Switch
                          checked={appSettings?.zoomPhotoToPixelClick ?? false}
                          id="zoom-photo-to-pixel-click"
                          label={t('settings.controls.zoomClickDesc')}
                          onChange={(checked) => {
                            onSettingsChange({ ...appSettings, zoomPhotoToPixelClick: checked });
                          }}
                        />
                      </SettingItem>
                    </div>
                  </div>

                  <div className="p-6 bg-surface rounded-xl shadow-md">
                    <Text variant={TextVariants.title} color={TextColors.accent} className="mb-5">
                      {t('settings.controls.keyboardTitle')}
                    </Text>
                    <div className="space-y-8">
                      {' '}
                      {KEYBIND_SECTIONS.map((section) => {
                        const sectionDefs = KEYBIND_DEFINITIONS.filter((d) => d.section === section.id);
                        const userKb = appSettings?.keybinds || {};
                        return (
                          <div key={section.id}>
                            <Text variant={TextVariants.heading}>{t(section.label as any)}</Text>
                            <div className="divide-y divide-border-color">
                              {sectionDefs.map((def) => (
                                <KeybindRow
                                  key={def.action}
                                  def={def}
                                  currentCombo={userKb[def.action]}
                                  osPlatform={osPlatform}
                                  onSave={handleKeybindSave}
                                  recordingAction={recordingAction}
                                  onStartRecording={setRecordingAction}
                                  isConflicting={conflictingKeys.has(def.action)}
                                />
                              ))}
                            </div>
                          </div>
                        );
                      })}
                      <div className="flex justify-end mt-6">
                        <Button variant="ghost" onClick={() => onSettingsChange({ ...appSettings, keybinds: {} })}>
                          {t('settings.controls.resetDefaults')}
                        </Button>
                      </div>
                    </div>
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </LayoutGroup>
    </>
  );
}
