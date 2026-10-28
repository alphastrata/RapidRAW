import { CopyPasteSettings } from '../../utils/adjustments';
import { ToolType } from '../panel/right/Masks';
import type { WhiteBalance, WhiteBalanceMode } from '../../utils/whiteBalance';

export const GLOBAL_KEYS = [
  ' ',
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'f',
  'b',
  'a',
  's',
  'd',
  'r',
  'm',
  'k',
  'p',
  'i',
  'e',
  '0',
  '1',
  '2',
  '3',
  '4',
  '5',
  'Enter',
];
export const OPTION_SEPARATOR = 'separator';

export enum Invokes {
  ApplyAdjustments = 'apply_adjustments',
  ApplyAdjustmentsToPaths = 'apply_adjustments_to_paths',
  ApplyAutoStraightenToPaths = 'apply_auto_straighten_to_paths',
  CalculateAutoAdjustments = 'calculate_auto_adjustments',
  SampleWhiteBalance = 'sample_white_balance',
  CalculateAutoStraighten = 'calculate_auto_straighten',
  CancelAiTask = 'cancel_ai_task',
  CancelExport = 'cancel_export',
  CheckAIConnectorStatus = 'check_ai_connector_status',
  ClearAiDenoiseCache = 'clear_ai_denoise_cache',
  EstimateExportSizes = 'estimate_export_sizes',
  ExportImages = 'export_images',
  FrontendLog = 'frontend_log',
  GeneratePreviewForPath = 'generate_preview_for_path',
  GenerateMaskOverlay = 'generate_mask_overlay',
  GenerateUncroppedPreview = 'generate_uncropped_preview',
  GetStorageUsage = 'get_storage_usage',
  ClearAiMapsCache = 'clear_ai_maps_cache',
  ClearAiModels = 'clear_ai_models',
  ClearMetadataCache = 'clear_metadata_cache',
  GetLogFilePath = 'get_log_file_path',
  GetSupportedFileTypes = 'get_supported_file_types',
  InvokeGenerativeReplaceWithMaskDef = 'invoke_generative_replace_with_mask_def',
  LoadImage = 'load_image',
  LoadMetadata = 'load_metadata',
  LoadSettings = 'load_settings',
  ResetAdjustmentsForPaths = 'reset_adjustments_for_paths',
  SaveMetadataAndUpdateThumbnail = 'save_metadata_and_update_thumbnail',
  SaveSettings = 'save_settings',
  StitchFocusStack = 'stitch_focus_stack',
  SubscribeAnalytics = 'subscribe_analytics',
  SaveFocusStack = 'save_focus_stack',
  TestAIConnectorConnection = 'test_ai_connector_connection',
  UpdateWgpuTransform = 'update_wgpu_transform',
  ReleaseOriginalPreview = 'release_original_preview',
  SaveTempFile = 'save_temp_file',
}

export enum ExifOverlay {
  Off = 'off',
  Hover = 'hover',
  Always = 'always',
}

export enum Panel {
  Adjustments = 'adjustments',
  Ai = 'ai',
  Crop = 'crop',
  Export = 'export',
  Masks = 'masks',
}

export type PanelRegion = 'leftTop' | 'leftBottom' | 'rightTop' | 'rightBottom';

export enum Theme {
  Arctic = 'arctic',
  Blue = 'blue',
  Dark = 'dark',
  Grey = 'grey',
  Light = 'light',
  MutedGreen = 'muted-green',
  Sepia = 'sepia',
  Snow = 'snow',
}

export interface WorkspaceState {
  leftPanelWidth: number;
  rightPanelWidth: number;
  leftTopHeight: number;
  rightTopHeight: number;
  panelLayout: Record<PanelRegion, Panel[]>;
  activePanels: Record<PanelRegion, Panel | null>;
  panelSwitcherPlacement: Record<PanelRegion, 'left' | 'right' | 'top' | 'bottom'>;
}

export interface CustomAspectRatio {
  width: number;
  height: number;
}

export type GroupPreference = 'jpeg' | 'raw';
export type GroupingMode = 'off' | GroupPreference;
export type CompareView = 'off' | 'vertical' | 'horizontal' | 'sideBySide';
export type CompareMode = 'toggle' | Exclude<CompareView, 'off'>;

export interface AppSettings {
  aiCacheLimitGb?: number;
  aiMapsCacheLimitGb?: number;
  aiConnectorAddress?: string;
  aiGpuAcceleration?: boolean;
  aiProvider?: string;
  decorations?: any;
  editorPreviewResolution?: number;
  enableZoomHifi?: boolean;
  useFullDpiRendering?: boolean;
  highResZoomMultiplier?: number;
  enableLivePreviews?: boolean;
  livePreviewQuality?: string;
  exportWorkerThreads?: number;
  exportMemoryLimit?: number;
  theme: Theme;
  uiVisibility?: UiVisibility;
  rawHighlightCompression?: number;
  processingBackend?: string;
  linuxGpuOptimization?: boolean;
  myLenses?: any;
  customAspectRatios?: CustomAspectRatio[];
  adjustmentLayout?: AdjustmentLayout;
  linearRawMode?: string;
  enableXmpSync?: boolean;
  createXmpIfMissing?: boolean;
  isWaveformVisible?: boolean;
  waveformHeight?: number;
  activeWaveformChannel?: string;
  useWgpuRenderer?: boolean;
  editorNeutralGreyBg?: boolean;
  canvasInputMode?: 'mouse' | 'trackpad';
  compareMode?: CompareMode;
  whiteBalanceMode?: WhiteBalanceMode;
  zoomSpeedMultiplier?: number;
  zoomPhotoToPixelClick?: boolean;
  keybinds?: { [action: string]: string[] };
  enableBaselineExposure?: boolean;
  baselineExposure?: number;
  compensateHighlightPreservation?: boolean;
  tonemapperOverrideEnabled?: boolean;
  defaultRawTonemapper?: string;
  defaultNonRawTonemapper?: string;
  copyPasteSettings?: CopyPasteSettings;
  enableFocusMode?: boolean;
  enableToolFocusMode?: boolean;
  exifOverlay?: ExifOverlay;
  language?: string;
  fontFamily?: string;
  imageCacheSize?: number;
  alwaysDecodeRawThumbnails?: boolean;
  workspace?: WorkspaceState;
}

export interface BrushSettings {
  feather: number;
  size: number;
  tool: ToolType;
}

export interface Option {
  color?: string;
  disabled?: boolean;
  icon?: any;
  isDestructive?: boolean;
  label?: string;
  onClick?(): void;
  onRightClick?(): void;
  submenu?: any;
  type?: string;
}

export enum Orientation {
  Horizontal = 'horizontal',
  Vertical = 'vertical',
}

export interface Progress {
  completed?: number;
  current?: number;
  total: number;
}

export interface SelectedImage {
  asShotWhiteBalance?: WhiteBalance;
  exif: any;
  group_id?: string | null;
  height: number;
  isRaw: boolean;
  isReady: boolean;
  metadata?: any;
  original_base64?: string;
  path: string;
  thumbnailUrl: string | null;
  width: number;
}

export interface SupportedTypes {
  nonRaw: Array<string>;
  raw: Array<string>;
}

export enum LibraryDisplayMode {
  Grid = 'grid',
  List = 'list',
}

export interface TransformState {
  positionX: number;
  positionY: number;
  scale: number;
}

export interface UiVisibility {
  leftPanel: boolean;
  rightPanel: boolean;
}

export interface CollapsibleSectionsState {
  basic: boolean;
  color: boolean;
  curves: boolean;
  details: boolean;
  effects: boolean;
}

export interface AdjustmentLayout {
  collapsedTools?: string[];
  hiddenSections?: string[];
  hiddenTools?: string[];
  openSections?: Partial<CollapsibleSectionsState>;
  sectionOrder?: string[];
  toolOrder?: Record<string, string[]>;
}

export interface WaveformData {
  height: number;
  luma: ImageBitmap | null;
  rgb: ImageBitmap | null;
  parade: ImageBitmap | null;
  vectorscope: ImageBitmap | null;
  width: number;
}

