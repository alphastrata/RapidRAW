import { Progress } from './AppProperties';

export enum FileFormats {
  Jpeg = 'jpeg',
  Png = 'png',
  Tiff = 'tiff',
  Webp = 'webp',
  Jxl = 'jxl',
  Avif = 'avif',
  Cube = 'cube',
}

export const FILE_FORMATS: Array<FileFormat> = [
  { id: FileFormats.Jpeg, name: 'JPEG', extensions: ['jpg', 'jpeg'] },
  { id: FileFormats.Png, name: 'PNG', extensions: ['png'] },
  { id: FileFormats.Tiff, name: 'TIFF', extensions: ['tiff'] },
  { id: FileFormats.Webp, name: 'WebP', extensions: ['webp'] },
  { id: FileFormats.Jxl, name: 'JPEG XL', extensions: ['jxl'] },
  { id: FileFormats.Avif, name: 'AVIF', extensions: ['avif'] },
  { id: FileFormats.Cube, name: 'CUBE LUT', extensions: ['cube'] },
];

export const FILENAME_VARIABLES: Array<string> = [
  '{original_filename}',
  '{original_filename_number}',
  '{sequence}',
  '{YYYY}',
  '{MM}',
  '{DD}',
  '{hh}',
  '{mm}',
];

const UNIQUE_FILENAME_VARIABLES: Array<string> = ['{original_filename}', '{original_filename_number}', '{sequence}'];

export const isUniqueFilenameTemplate = (template: string): boolean =>
  UNIQUE_FILENAME_VARIABLES.some((variable) => template.includes(variable));

export type TiffBitDepth = 8 | 16;

export const DEFAULT_EXPORT_DPI = 300;
export const MAX_EXPORT_DPI = 65535;

export interface ExportSettings {
  filenameTemplate: string | null;
  jpegQuality: number;
  tiffBitDepth: TiffBitDepth;
  keepMetadata: boolean;
  preserveTimestamps: boolean;
  stripGps: boolean;
  exportMasks?: boolean;
  destinationType?: string;
  subfolder?: string;
  dpi?: number;
}

export interface ExportState {
  destination?: string | null;
  errorMessage: string;
  progress: Progress;
  status: Status;
}

export interface FileFormat {
  extensions: Array<string>;
  id: string;
  name: string;
}

export interface ImportState {
  destination?: string | null;
  errorMessage: string;
  path?: string;
  progress?: Progress;
  status: Status;
}

export enum Status {
  Cancelled = 'cancelled',
  Cancelling = 'cancelling',
  Exporting = 'exporting',
  Error = 'error',
  Idle = 'idle',
  Importing = 'importing',
  Success = 'success',
}
