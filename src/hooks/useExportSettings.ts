import { useState } from 'react';
import { DEFAULT_EXPORT_DPI, TiffBitDepth } from '../components/ui/ExportImportProperties';

export function useExportSettings() {
  const [fileFormat, setFileFormat] = useState('jpeg');
  const [jpegQuality, setJpegQuality] = useState(90);
  const [tiffBitDepth, setTiffBitDepth] = useState<TiffBitDepth>(16);
  const [dpi, setDpi] = useState(DEFAULT_EXPORT_DPI);
  const [keepMetadata, setKeepMetadata] = useState(true);
  const [preserveTimestamps, setPreserveTimestamps] = useState(false);
  const [stripGps, setStripGps] = useState(true);
  const [exportMasks, setExportMasks] = useState(false);
  const [filenameTemplate, setFilenameTemplate] = useState('{original_filename}_edited');
  const [destinationType, setDestinationType] = useState<string>('customFolder');
  const [subfolder, setSubfolder] = useState<string>('');

  return {
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
  };
}