import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore } from '../store/useSettingsStore';
import { useUIStore, reconcileWorkspace } from '../store/useUIStore';
import { useEditorStore } from '../store/useEditorStore';
import { useProcessStore } from '../store/useProcessStore';
import { THEMES, DEFAULT_THEME_ID, ThemeProps } from '../utils/themes';
import { COPYABLE_ADJUSTMENT_KEYS, withAdjustmentLayout } from '../utils/adjustments';
import { Theme } from '../components/ui/AppProperties';
import { useTranslation } from 'react-i18next'

const getDefaultLanguage = (i18nInstance: any): string => {
  const browserLang = navigator.language || (navigator as any).userLanguage || 'en';
  const shortLang = browserLang.split('-')[0].toLowerCase();
  const supportedLanguages = Object.keys(i18nInstance.options.resources || {});
  const fallbackLang =
    typeof i18nInstance.options.fallbackLng === 'string'
      ? i18nInstance.options.fallbackLng
      : i18nInstance.options.fallbackLng?.[0] || 'en';

  return supportedLanguages.includes(browserLang)
    ? browserLang
    : supportedLanguages.includes(shortLang)
      ? shortLang
      : fallbackLang;
};

export const useAppInitialization = () => {
  const isInitialMount = useRef(true);
  const { i18n } = useTranslation();

  const {
    appSettings,
    theme,
    osPlatform,
    setAppSettings,
    setTheme,
    setSupportedTypes,
    initPlatform,
    handleSettingsChange,
  } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      theme: state.theme,
      osPlatform: state.osPlatform,
      setAppSettings: state.setAppSettings,
      setTheme: state.setTheme,
      setSupportedTypes: state.setSupportedTypes,
      initPlatform: state.initPlatform,
      handleSettingsChange: state.handleSettingsChange,
    })),
  );

  const { uiVisibility, collapsibleSectionsState, setUI } = useUIStore(
    useShallow((state) => ({
      uiVisibility: state.uiVisibility,
      collapsibleSectionsState: state.collapsibleSectionsState,
      setUI: state.setUI,
    })),
  );

  const workspaceProps = useUIStore(
    useShallow((state) => ({
      leftPanelWidth: state.leftPanelWidth,
      rightPanelWidth: state.rightPanelWidth,
      leftTopHeight: state.leftTopHeight,
      rightTopHeight: state.rightTopHeight,
      panelLayout: state.panelLayout,
      activePanels: state.activePanels,
      panelSwitcherPlacement: state.panelSwitcherPlacement,
    })),
  );

  const { setEditor } = useEditorStore(
    useShallow((state) => ({
      setEditor: state.setEditor,
    })),
  );

  useEffect(() => {
    initPlatform();
  }, [initPlatform]);

  useEffect(() => {
    invoke('get_supported_file_types')
      .then((types: any) => setSupportedTypes(types))
      .catch((err) => console.error('Failed to load supported file types:', err));
  }, [setSupportedTypes]);

  useEffect(() => {
    invoke('load_settings')
      .then(async (settings: any) => {
        if (
          !settings.copyPasteSettings ||
          !settings.copyPasteSettings.includedAdjustments ||
          settings.copyPasteSettings.includedAdjustments.length === 0
        ) {
          settings.copyPasteSettings = { mode: 'merge', includedAdjustments: COPYABLE_ADJUSTMENT_KEYS };
        }

        if (!settings.language) {
          settings.language = getDefaultLanguage(i18n);
          handleSettingsChange(settings);
        }

        const reconciledWorkspace = reconcileWorkspace(settings?.workspace);
        settings.workspace = reconciledWorkspace;

        setAppSettings(settings);
        i18n.changeLanguage(settings.language);

        if (settings?.theme) setTheme(settings.theme);

        if (settings?.uiVisibility) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, ...settings.uiVisibility } }));
        }

        if (settings?.adjustmentLayout?.openSections) {
          setUI((state) => ({
            collapsibleSectionsState: { ...state.collapsibleSectionsState, ...settings.adjustmentLayout.openSections },
          }));
        }

        setUI({
          leftPanelWidth: reconciledWorkspace.leftPanelWidth,
          rightPanelWidth: reconciledWorkspace.rightPanelWidth,
          leftTopHeight: reconciledWorkspace.leftTopHeight,
          rightTopHeight: reconciledWorkspace.rightTopHeight,
          panelLayout: reconciledWorkspace.panelLayout,
          activePanels: reconciledWorkspace.activePanels,
          panelSwitcherPlacement: reconciledWorkspace.panelSwitcherPlacement,
        });

        if (settings?.isWaveformVisible !== undefined) setEditor({ isWaveformVisible: settings.isWaveformVisible });
        if (settings?.activeWaveformChannel) setEditor({ activeWaveformChannel: settings.activeWaveformChannel });
        if (typeof settings?.waveformHeight === 'number') setEditor({ waveformHeight: settings.waveformHeight });

        invoke('frontend_ready')
          .then((launch: any) => {
            if (launch?.editSession) {
              useProcessStore.getState().setProcess({ externalEditSession: launch.editSession });
            } else if (launch?.openWithFile) {
              useProcessStore.getState().setProcess({ initialFileToOpen: launch.openWithFile });
            }
          })
          .catch((e) => console.error('Failed to notify backend of readiness:', e));
      })
      .catch((err) => {
        console.error('Failed to load settings:', err);
        setAppSettings({
          theme: DEFAULT_THEME_ID as Theme,
        });
      })
      .finally(() => {
        isInitialMount.current = false;
      });
  }, [
    setAppSettings,
    setTheme,
    setUI,
    setEditor,
    handleSettingsChange,
  ]);

  useEffect(() => {
    if (isInitialMount.current || !appSettings) return;

    const currentWorkspaceStr = JSON.stringify(appSettings.workspace || {});
    const newWorkspaceStr = JSON.stringify(workspaceProps);

    if (currentWorkspaceStr !== newWorkspaceStr) {
      const timeoutId = setTimeout(() => {
        handleSettingsChange({ ...appSettings, workspace: workspaceProps });
      }, 500);

      return () => clearTimeout(timeoutId);
    }
  }, [workspaceProps, appSettings, handleSettingsChange]);

  useEffect(() => {
    if (isInitialMount.current || !appSettings) return;
    if (JSON.stringify(appSettings.uiVisibility) !== JSON.stringify(uiVisibility)) {
      handleSettingsChange({ ...appSettings, uiVisibility });
    }
  }, [uiVisibility, appSettings, handleSettingsChange]);

  useEffect(() => {
    if (isInitialMount.current || !appSettings) return;
    if (JSON.stringify(appSettings.adjustmentLayout?.openSections) !== JSON.stringify(collapsibleSectionsState)) {
      handleSettingsChange(withAdjustmentLayout(appSettings, { openSections: collapsibleSectionsState }));
    }
  }, [collapsibleSectionsState, appSettings, handleSettingsChange]);

  useEffect(() => {
    if (isInitialMount.current || !appSettings) return;
    if (appSettings.language && appSettings.language !== i18n.language) {
      i18n.changeLanguage(appSettings.language);
    }
  }, [appSettings?.language, i18n.language]);

  useEffect(() => {
    const root = document.documentElement;
    const currentThemeId = theme || DEFAULT_THEME_ID;

    const baseTheme =
      THEMES.find((t: ThemeProps) => t.id === currentThemeId) ||
      THEMES.find((t: ThemeProps) => t.id === DEFAULT_THEME_ID);
    if (!baseTheme) return;

    const finalCssVariables: any = { ...baseTheme.cssVariables };

    Object.entries(finalCssVariables).forEach(([key, value]) => {
      root.style.setProperty(key, value as string);
    });

    const fontFamily = appSettings?.fontFamily || 'poppins';
    const fontStack =
      fontFamily === 'system'
        ? '-apple-system, BlinkMacSystemFont, system-ui, sans-serif'
        : "'Poppins', system-ui, sans-serif";
    root.style.setProperty('--font-family', fontStack);
  }, [theme, appSettings?.fontFamily]);
};