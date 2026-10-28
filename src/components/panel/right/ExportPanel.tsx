import { useState, useEffect, useRef, useMemo } from 'react';
import { save, open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { FileInput, CheckCircle, XCircle, Loader, Ban, ChevronDown, ChevronRight, Settings, X } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import debounce from 'lodash.debounce';
import Switch from '../../ui/Switch';
import Button from '../../ui/Button';
import Dropdown from '../../ui/Dropdown';
import Slider from '../../ui/Slider';
import { ParsedTextField, useParsedTextField } from '../../ui/ColorField';
import {
  ExportSettings,
  FileFormat,
  FILE_FORMATS,
  FILENAME_VARIABLES,
  Status,
  ExportState,
  FileFormats,
  MAX_EXPORT_DPI,
  TiffBitDepth,
} from '../../ui/ExportImportProperties';
import { Invokes, SelectedImage, Panel } from '../../ui/AppProperties';
import { useExportSettings } from '../../../hooks/useExportSettings';
import Text from '../../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { useEditorStore } from '../../../store/useEditorStore';
import { useUIStore } from '../../../store/useUIStore';
import { formatBytes } from '../../../utils/formatBytes';

interface ExportPanelProps {
  exportState: ExportState;
  selectedImage: SelectedImage | null;
  setExportState(state: any): void;
  isVisible?: boolean;
}

function parseDpi(value: string): number | null {
  const parsed = Number(value.trim());
  return Number.isInteger(parsed) && parsed > 0 && parsed <= MAX_EXPORT_DPI ? parsed : null;
}

interface ValidatedNumberInputProps {
  ariaLabel: string;
  disabled: boolean;
  field: ParsedTextField<number>;
  max?: number;
}

function ValidatedNumberInput({ ariaLabel, disabled, field, max }: ValidatedNumberInputProps) {
  return (
    <input
      aria-label={ariaLabel}
      className={`w-20 bg-surface text-center rounded-md p-2 border focus:ring-accent text-text-secondary focus:text-text-primary ${
        field.parsed === null ? 'border-red-500' : 'border-surface focus:border-accent'
      }`}
      disabled={disabled}
      max={max}
      min="0"
      onChange={(e) => field.handleChange(e.target.value)}
      step="any"
      type="number"
      value={field.text}
    />
  );
}

interface SectionProps {
  children: any;
  title: string;
}

function Section({ title, children }: SectionProps) {
  return (
    <div>
      <Text variant={TextVariants.heading} className="mb-2">
        {title}
      </Text>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

export default function ExportPanel({ selectedImage, exportState, setExportState, isVisible = true }: ExportPanelProps) {
  const { t } = useTranslation();

  const tiffBitDepthOptions = useMemo(
    () => [
      { label: t('export.file.tiffBitDepth8'), value: '8' },
      { label: t('export.file.tiffBitDepth16'), value: '16' },
    ],
    [t],
  );

  const {
    fileFormat,
    setFileFormat,
    jpegQuality,
    setJpegQuality,
    tiffBitDepth,
    setTiffBitDepth,
    dpi,
    setDpi,
    keepMetadata,
    setKeepMetadata,
    preserveTimestamps,
    setPreserveTimestamps,
    stripGps,
    setStripGps,
    exportMasks,
    setExportMasks,
    filenameTemplate,
    setFilenameTemplate,
    destinationType,
    setDestinationType,
    subfolder,
    setSubfolder,
  } = useExportSettings();

  const dpiField = useParsedTextField(dpi, setDpi, parseDpi);
  const [estimatedSize, setEstimatedSize] = useState<number | null>(null);
  const [isEstimating, setIsEstimating] = useState<boolean>(false);
  const [isAdvancedExpanded, setIsAdvancedExpanded] = useState(false);
  const filenameInputRef = useRef<HTMLInputElement>(null);
  const activePanels = useUIStore((state) => state.activePanels);
  const isPanelReallyActive = Object.values(activePanels).includes(Panel.Export);

  const { status, errorMessage } = exportState;
  const isExporting = [Status.Exporting, Status.Cancelling].includes(status);
  const isCancelling = status === Status.Cancelling;

  const pathsToExport = useMemo(
    () => (selectedImage ? [selectedImage.path] : []),
    [selectedImage?.path],
  );

  const numImages = pathsToExport.length;
  const adjustmentsRef = useRef(useEditorStore.getState().adjustments);

  const debouncedEstimateSize = useMemo(
    () =>
      debounce(async (paths, currentAdj, currentPath, exportSettings, format) => {
        if (paths.length === 0 || !isVisible) {
          setEstimatedSize(null);
          return;
        }
        setIsEstimating(true);
        try {
          const size: number = await invoke(Invokes.EstimateExportSizes, {
            paths,
            exportSettings,
            outputFormat: format,
            currentEditPath: currentPath || null,
            currentEditAdjustments: currentAdj || null,
          });
          setEstimatedSize(size);
        } catch (err) {
          setEstimatedSize(null);
        } finally {
          setIsEstimating(false);
        }
      }, 500),
    [isVisible],
  );

  useEffect(() => {
    if (!isPanelReallyActive) return;

    const exportSettings: ExportSettings = {
      filenameTemplate,
      jpegQuality,
      tiffBitDepth,
      keepMetadata,
      preserveTimestamps,
      destinationType,
      subfolder,
      stripGps,
      exportMasks,
      dpi,
    };
    const format = FILE_FORMATS.find((f: FileFormat) => f.id === fileFormat)?.extensions[0] || 'jpeg';
    const runEstimate = () =>
      debouncedEstimateSize(pathsToExport, adjustmentsRef.current, selectedImage?.path, exportSettings, format);

    runEstimate();

    let prevAdjustments = useEditorStore.getState().adjustments;
    const unsubscribe = useEditorStore.subscribe((state) => {
      if (state.adjustments !== prevAdjustments) {
        prevAdjustments = state.adjustments;
        adjustmentsRef.current = state.adjustments;
        runEstimate();
      }
    });

    return () => {
      unsubscribe();
      debouncedEstimateSize.cancel();
    };
  }, [
    isPanelReallyActive,
    pathsToExport,
    selectedImage?.path,
    fileFormat,
    jpegQuality,
    tiffBitDepth,
    keepMetadata,
    preserveTimestamps,
    stripGps,
    filenameTemplate,
    debouncedEstimateSize,
    exportMasks,
    destinationType,
    subfolder,
    dpi,
  ]);

  const handleVariableClick = (variable: string) => {
    if (!filenameInputRef.current) return;
    const input: HTMLInputElement = filenameInputRef.current;
    const start = Number(input.selectionStart);
    const end = Number(input.selectionEnd);
    const currentValue = input.value;
    const newValue = currentValue.substring(0, start) + variable + currentValue.substring(end);
    setFilenameTemplate(newValue);
    setTimeout(() => {
      input.focus();
      input.setSelectionRange(start + variable.length, start + variable.length);
    }, 0);
  };

  const handleExport = async () => {
    if (numImages === 0 || isExporting) return;

    const exportSettings: ExportSettings = {
      filenameTemplate,
      jpegQuality,
      tiffBitDepth,
      keepMetadata,
      preserveTimestamps,
      destinationType,
      subfolder,
      stripGps,
      exportMasks,
      dpi,
    };

    try {
      const selectedFormat: any = FILE_FORMATS.find((f) => f.id === fileFormat);

      let outputFolderOrFile = '';
      const isOriginalFolder = destinationType === 'originalFolder';
      const shouldChooseOutputFile = !isOriginalFolder;

      if (isOriginalFolder) {
        outputFolderOrFile = 'originalFolderDummy';
      } else {
        const originalFilename = pathsToExport[0].split(/[\\/]/).pop() || '';
        const stem = originalFilename.substring(0, originalFilename.lastIndexOf('.')) || originalFilename;
        const suggestedName = (filenameTemplate || '').replace('{original_filename}', stem);
        const outputFileName = `${suggestedName}.${selectedFormat.extensions[0]}`;

        outputFolderOrFile = ((await save({
              title: t('export.dialog.saveEditedImageTitle'),
              defaultPath: outputFileName,
              filters: [
                { name: selectedFormat.name, extensions: selectedFormat.extensions },
                ...FILE_FORMATS.filter((f: FileFormat) => f.id !== fileFormat).map((f: FileFormat) => ({
                  name: f.name,
                  extensions: f.extensions,
                })),
              ],
            })) as string);
      }

      if (outputFolderOrFile) {
        setExportState({
          destination: isOriginalFolder ? null : outputFolderOrFile,
          errorMessage: '',
          progress: { current: 0, total: numImages },
          status: Status.Exporting,
        });
        await invoke(Invokes.ExportImages, {
          paths: pathsToExport,
          outputFolderOrFile: outputFolderOrFile,
          isExplicitFilePath: shouldChooseOutputFile,
          exportSettings,
          outputFormat: selectedFormat.extensions[0],
          currentEditPath: selectedImage?.path || null,
          currentEditAdjustments: adjustmentsRef.current || null,
        });
      }
    } catch (error) {
      setExportState({
        errorMessage: typeof error === 'string' ? error : t('export.status.failed'),
        progress: { current: 0, total: numImages },
        status: Status.Error,
      });
    }
  };

  const handleCancel = async () => {
    setExportState((current: ExportState) =>
      current.status === Status.Exporting ? { status: Status.Cancelling } : {},
    );
    try {
      await invoke(Invokes.CancelExport);
    } catch (error) {
      console.error('Failed to cancel:', error);
      setExportState((current: ExportState) =>
        current.status === Status.Cancelling ? { status: Status.Exporting } : {},
      );
    }
  };

  const isLut = fileFormat === FileFormats.Cube;
  const itemLabel = isLut ? t('export.labels.lut') : t('export.labels.image');

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('export.title')}</Text>
      </div>
      <div className="grow overflow-y-auto p-3 space-y-8">
        {numImages > 0 ? (
          <>
            <Section title={t('export.sections.fileSettings')}>
              <div className="grid grid-cols-3 gap-2">
                {FILE_FORMATS.map((format: FileFormat) => (
                  <button
                    className={`px-2 py-1.5 rounded-md transition-colors ${fileFormat === format.id ? 'bg-accent' : 'bg-surface hover:bg-card-active'} disabled:opacity-50`}
                    disabled={isExporting}
                    key={format.id}
                    onClick={() => setFileFormat(format.id)}
                  >
                    <Text color={fileFormat === format.id ? TextColors.button : TextColors.secondary}>
                      {format.name}
                    </Text>
                  </button>
                ))}
              </div>
              {[FileFormats.Jpeg, FileFormats.Webp, FileFormats.Jxl].includes(fileFormat as FileFormats) && (
                <div className={isExporting ? 'opacity-50 pointer-events-none' : ''}>
                  <Slider
                    defaultValue={90}
                    label={
                      fileFormat === FileFormats.Jxl && jpegQuality === 100
                        ? t('export.file.qualityLossless')
                        : t('export.file.quality')
                    }
                    max={100}
                    min={1}
                    onChange={(e) => setJpegQuality(Number(e.target.value))}
                    step={1}
                    value={jpegQuality}
                    fillOrigin="min"
                  />
                </div>
              )}
              {fileFormat === FileFormats.Tiff && (
                <div className={isExporting ? 'opacity-50 pointer-events-none' : ''}>
                  <Text variant={TextVariants.label} className="mb-1">
                    {t('export.file.tiffBitDepth')}
                  </Text>
                  <Dropdown
                    options={tiffBitDepthOptions}
                    value={String(tiffBitDepth)}
                    onChange={(value) => setTiffBitDepth(Number(value) as TiffBitDepth)}
                    disabled={isExporting}
                    className="w-full"
                  />
                </div>
              )}
            </Section>

            {!isLut && (
              <Section title={t('export.sections.destination')}>
                <div className="space-y-3">
                  <Dropdown
                    options={[
                      { label: t('export.destination.customFolder'), value: 'customFolder' },
                      { label: t('export.destination.originalFolder'), value: 'originalFolder' },
                    ]}
                    value={destinationType || 'customFolder'}
                    onChange={(val) => setDestinationType(val as string)}
                    disabled={isExporting}
                    className="w-full"
                  />

                  {destinationType === 'originalFolder' && (
                    <div className="flex items-center gap-3 pl-2 border-l-2 border-surface">
                      <Text variant={TextVariants.label} className="whitespace-nowrap min-w-[70px]">
                        {t('export.destination.subfolder')}
                      </Text>
                      <input
                        className="w-full bg-surface border border-transparent rounded-md px-3 py-2 text-sm text-text-primary focus:outline-hidden truncate"
                        disabled={isExporting}
                        onChange={(e) => setSubfolder(e.target.value)}
                        type="text"
                        value={subfolder || ''}
                        placeholder={t('export.destination.subfolderPlaceholder')}
                      />
                    </div>
                  )}
                </div>
              </Section>
            )}

            <Section title={t('export.sections.fileNaming')}>
              <input
                className="w-full bg-surface border border-surface rounded-md p-2 text-sm text-text-primary focus:ring-accent focus:border-accent"
                disabled={isExporting}
                onChange={(e) => setFilenameTemplate(e.target.value)}
                ref={filenameInputRef}
                type="text"
                value={filenameTemplate}
              />
              <div className="flex flex-wrap gap-2 mt-2">
                {FILENAME_VARIABLES.map((variable: string) => (
                  <button
                    className="px-2 py-1 bg-surface text-text-secondary text-xs rounded-md hover:bg-card-active transition-colors disabled:opacity-50"
                    disabled={isExporting}
                    key={variable}
                    onClick={() => handleVariableClick(variable)}
                  >
                    {variable}
                  </button>
                ))}
              </div>
            </Section>

            {!isLut && (
              <>
                {[FileFormats.Jpeg, FileFormats.Png, FileFormats.Webp].includes(fileFormat as FileFormats) && (
                  <Section title={t('export.sections.metadata')}>
                    <Switch
                      checked={keepMetadata}
                      disabled={isExporting}
                      label={t('export.metadata.saveWithMetadata')}
                      onChange={setKeepMetadata}
                      trackClassName="bg-surface"
                    />
                    {keepMetadata && (
                      <div className="pl-2 border-l-2 border-surface">
                        <Switch
                          label={t('export.metadata.removeGps')}
                          checked={stripGps}
                          onChange={setStripGps}
                          disabled={isExporting}
                          trackClassName="bg-surface"
                        />
                      </div>
                    )}
                  </Section>
                )}

                <div>
                  <Text variant={TextVariants.heading} className="mb-2">
                    {t('export.sections.advanced')}
                  </Text>
                  <div className="bg-surface rounded-xl overflow-hidden">
                    <button
                      onClick={() => setIsAdvancedExpanded(!isAdvancedExpanded)}
                      disabled={isExporting}
                      className="w-full flex items-center justify-between p-3.5 hover:bg-card-active transition-colors"
                    >
                      <Text
                        as="span"
                        variant={TextVariants.label}
                        color={TextColors.primary}
                        className="flex items-center gap-2"
                      >
                        <Settings size={16} /> {t('export.advanced.title')}
                      </Text>
                      <Text color={TextColors.secondary}>
                        {isAdvancedExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                      </Text>
                    </button>
                    <AnimatePresence initial={false}>
                      {isAdvancedExpanded && (
                        <motion.div
                          initial={{ height: 0, opacity: 0 }}
                          animate={{ height: 'auto', opacity: 1 }}
                          exit={{ height: 0, opacity: 0 }}
                          transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
                          className="overflow-hidden"
                        >
                          <div className="px-4 pb-4 pt-2 border-t border-surface/50 flex flex-col gap-4">
                            <Switch
                              checked={preserveTimestamps}
                              disabled={isExporting}
                              label={t('export.advanced.preserveTimestamps')}
                              onChange={setPreserveTimestamps}
                            />
                            <Switch
                              label={t('export.advanced.exportMasks')}
                              checked={exportMasks}
                              onChange={setExportMasks}
                              disabled={isExporting}
                            />
                            {[FileFormats.Jpeg, FileFormats.Png, FileFormats.Tiff].includes(fileFormat as FileFormats) && (
                              <div className="flex items-center gap-2">
                                <Text variant={TextVariants.label} className="flex-1">
                                  {t('export.file.resolution')}
                                </Text>
                                <ValidatedNumberInput
                                  ariaLabel={t('export.file.resolution')}
                                  disabled={isExporting}
                                  field={dpiField}
                                  max={MAX_EXPORT_DPI}
                                />
                                <Text variant={TextVariants.label}>{t('export.file.dpi')}</Text>
                              </div>
                            )}
                          </div>
                        </motion.div>
                      )}
                    </AnimatePresence>
                  </div>
                </div>
              </>
            )}
          </>
        ) : (
          <div className="flex items-center justify-center h-full">
            <Text
              variant={TextVariants.heading}
              color={TextColors.secondary}
              weight={TextWeights.normal}
              className="text-center"
            >
              {t('export.status.noImageSelected')}
            </Text>
          </div>
        )}
      </div>

      <div className="p-3 border-t border-surface shrink-0 space-y-2">
        <Text as="div" variant={TextVariants.small} color={TextColors.primary} className="text-center">
          {numImages > 0 && !isLut ? (
            isEstimating ? (
              <span className="italic">{t('export.status.estimatingSize')}</span>
            ) : estimatedSize !== null ? (
              <span>{t('export.status.estimatedSize', { size: formatBytes(estimatedSize, t) })}</span>
            ) : null
          ) : null}
        </Text>
        <motion.div
          whileTap={!(isCancelling || (status !== Status.Exporting && numImages === 0)) ? { scale: 0.98 } : undefined}
          transition={{ type: 'spring', stiffness: 400, damping: 17 }}
          className="w-full"
        >
          <Button
            className={`group rounded-md h-11 w-full flex items-center text-md font-bold! justify-center transition-colors ${
              status === Status.Exporting
                ? 'bg-red-500/15 hover:bg-red-500/25 text-red-400 border border-red-500/30 shadow-none'
                : status === Status.Cancelling
                  ? 'bg-yellow-500/20 text-yellow-400 shadow-none'
                  : status === Status.Success
                    ? 'bg-green-500/70 text-white shadow-none'
                    : status === Status.Error
                      ? 'bg-red-500/20 text-red-400 shadow-none'
                      : status === Status.Cancelled
                        ? 'bg-yellow-500/20 text-yellow-400 shadow-none'
                        : ''
            }`}
            disabled={isCancelling || (status !== Status.Exporting && numImages === 0)}
            onClick={status === Status.Exporting ? handleCancel : handleExport}
            size="lg"
          >
            {status === Status.Exporting ? (
              <>
                <span className="flex items-center group-hover:hidden">
                  <Loader size={18} className="animate-spin mr-2" />
                  {t('export.status.exporting')}
                </span>
                <span className="hidden items-center group-hover:flex">
                  <X size={18} className="mr-2" />
                  {t('export.status.cancelExport')}
                </span>
              </>
            ) : status === Status.Cancelling ? (
              <>
                <Loader size={18} className="animate-spin mr-2" /> {t('export.status.cancelling')}
              </>
            ) : status === Status.Success ? (
              <>
                <CheckCircle size={18} className="mr-2" /> {t('export.status.success')}
              </>
            ) : status === Status.Error ? (
              <>
                <XCircle size={18} className="mr-2" /> {errorMessage || t('export.status.failed')}
              </>
            ) : status === Status.Cancelled ? (
              <>
                <Ban size={18} className="mr-2" /> {t('export.status.cancelled')}
              </>
            ) : (
              <>
                <FileInput size={18} className="mr-2" />{' '}
                {t('export.status.exportSingle', { label: itemLabel })}
              </>
            )}
          </Button>
        </motion.div>
      </div>
    </div>
  );
}