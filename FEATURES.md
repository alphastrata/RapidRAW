# FEATURES

Mark `X` in the **Keep** column for anything that stays; unmarked rows are candidates for deletion.

## Library & file management
| Feature | Touch points | Keep |
|---|---|---|
| Folder tree browsing (pinned roots, recursive listing) | `get_folder_tree`, `get_pinned_folder_trees`, `list_images_in_dir`, `list_images_recursive`, FolderTree.tsx |  |
| Create/rename/move/copy/delete folders (hierarchy, associated files) | `create_folder`, `rename_folder`, `delete_folder`, `move_files`, `copy_files`, CreateFolderModal, RenameFolderModal |  |
| Create/rename/duplicate/virtual-copy files | `create_virtual_copy`, `duplicate_file`, `rename_files`, RenameFileModal |  |
| Delete with associated sidecars/cache | `delete_files_from_disk`, `delete_files_with_associated` |  |
| Albums (create, add images, save, list) | `get_albums`, `add_to_album`, `save_albums`, `get_album_images` |  |
| Flag / rating / colour labels / internal library metadata | `set_flag_for_paths`, `set_rating_for_paths`, `set_color_label_for_paths`, `load_metadata`, `save_metadata_and_update_thumbnail` |  |
| EXIF read + field editing | `read_exif_for_paths`, `update_exif_fields` |  |
| Show in Finder/Explorer | `show_in_finder` |  |
| Legacy preset import | `handle_import_legacy_presets_from_file` |  |
| Presets import/export (file + community) | `handle_import_presets_from_file(s)`, `handle_export_presets_to_file`, `save_community_preset`, `load_presets`, `save_presets`, PresetsPanel, ConfigurePresetModal, CommunityPage |  |
| Import settings from file | `ImportSettingsModal`, `handle_import_presets_from_file` variants |  |
| Thumbnail generation/queue/cache/prune | thumbnail queue, `clear_thumbnail_cache`, `update_thumbnail_queue`, ThumbnailManager |  |
| Session image caches | `clear_session_caches`, `clear_image_caches`, ImageLRUCache |  |
| Settings panel / app settings persistence | `load_settings`, `save_settings`, SettingsPanel, DataManagementSettings |  |
| Storage usage dashboard + per-cache clearing | `get_storage_usage`, `clear_*_cache`, `clear_ai_models` |  |

## Core editing (image pipeline)
| Feature | Touch points | Keep |
|---|---|---|
| Basic adjustments (exposure, contrast, WB, vibrance, …) | Basic.tsx, engine `image_processing` |X|
| Colour mixer / colour grading / curves | Color.tsx, Curves.tsx |X|
| Details (sharpness, denoise BM3D/AI, clarity) | Details.tsx, engine `bm3d`, `ai_processing` |X|
| Effects (fade, split tone, glow, …) + effect maps (depth/normal) | Effects.tsx, engine `effect_maps`, `fog`, `relight` |X|
| Crop / aspect / straighten (manual) | CropPanel.tsx, engine crop/geometry |X|
| Automatic adjustments (auto exposure/WB analysis) | `calculate_auto_adjustments` |X|
| Auto white balance (sample) | `sample_white_balance` |X|
| Auto straighten (vanishing-point detection) | `calculate_auto_straighten`, `apply_auto_straighten_to_paths`, engine `auto_straighten` |X|
| Auto lens correction (autodetect, lensfun DB) | `autodetect_lens`, `get_lens_distortion_params`, `get_lensfun_*`, engine `lens_correction` |X|
| Masking (brush, gradient, parametric, masks panel) | Masks.tsx, MasksPanel.tsx, engine `mask_generation` |X|
| Mask overlays preview | `generate_mask_overlay` |X|
| Negative conversion (film scans) | NegativeConversionModal, engine `negative_conversion` |X|
| LUT import/apply/preview | `import_luts`, `list_luts`, `load_and_parse_lut`, `generate_lut_previews`, `remove_lut` |X|
| Guided perspective (two-line perspective correction) | `calculate_guided_perspective`, engine `guided_perspective` |X|
| Liquify (push/pinch/expand/twirl) | `generate_liquify_patch`, engine `inpainting` |X|
| Manual cleanup / heal / clone | `generate_manual_cleanup_patch`, engine `inpainting` |X|
| Retouch (bilateral smoothen) | `generate_retouch_patch` |X|
| Generative replace (cloud inpainting) | `invoke_generative_replace_with_mask_def`, `ai_connector::process_inpainting` |X|
| Relight (normal-map relight) | `generate_relight_normal_map`, engine `relight` |X|
| Highlight reconstruction | engine `highlight_reconstruction` |X|
| Focus stacking (pyramid + guided filter) | `stitch_focus_stack`, `save_focus_stack`, FocusStackModal, engine `focus_stacking` |X|
| Waveform / histogram analytics | `request_analytics`, Waveform.tsx, ConnectedWaveform.tsx, engine histogram + `AnalyticsFrame` |X|
| Fog removal | engine `fog` |X|
| Apple RAW 9 develop (CoreImage) | engine `apple_raw` ||

## AI features
| Feature | Touch points | Keep |
|---|---|---|
| AI denoise (ONNX pipeline, progress, cache) | DenoiseModal, `ai_denoise`, engine `ai_processing` |  |
| AI subject mask (SAM/BiRefNet) | `generate_ai_subject_mask`, `precompute_ai_subject_mask` |  |
| AI foreground/sky/text/face/depth masks | `generate_ai_*_mask` commands, AIPanel.tsx |  |
| Face analysis + face-part masking | engine `face_masking`, FacePartsIllustration.tsx |  |
| AI background indexing + tag generation (CLIP + HSV) | `start_background_indexing`, `add_tag_for_paths`, `remove_tag_for_paths`, `clear_ai_tags`, `clear_all_tags`, engine `tagging` |  |
| AI connector (remote inference server) | `check_ai_connector_status`, `test_ai_connector_connection`, engine `ai_connector` |  |
| AI job queue (async batch, pause/resume/retry, journal) | `queue_ai_jobs`, `pause_ai_jobs`, `resume_ai_jobs`, `retry_failed_ai_jobs`, `discard_ai_jobs`, `get_ai_jobs_status`, `get_pending_ai_paths` |  |

## Batch / special workflows
| Feature | Touch points | Keep |
|---|---|---|
| Culling view (blur/exposure scoring, groups) | `cull_images`, CullingModal, CullingView.tsx, engine `culling` |  |
| Panorama stitching | `stitch_panorama`, `save_panorama`, PanoramaModal, engine `panorama` |  |
| HDR merge + deghosting | HdrModal, engine `hdr_deghosting` |  |
| Batch export (queue, presets, sizes estimate, cancel) | `export_images`, `estimate_export_sizes`, `cancel_export`, ExportPanel.tsx |  |
| Collage export | CollageModal.tsx, engine collage variants |  |
| Export via LUT/watermark/border/canvas (geometry) | engine `export_geometry`, apply_watermark |  |
| Tethering (gphoto2 capture, settings, preview, autofocus) | `tether_*` commands, TetheringPanel.tsx, engine `tethering` |  |
| Multi-exposure merge | engine `multi_exposure` |  |

## External integrations (recommended for deletion)
| Feature | Touch points | Keep |
|---|---|---|
| Immich sync/timeline/albums (9 commands, whole UI dir) | `commands::immich_*`, src/components/immich/*, engine immich |  |
| Android JNI integration (content URIs) | `resolve_android_content_uri_name`, android_integration.rs, engine `android`, `best_effort` jni |  |

## Platform & infrastructure
| Feature | Touch points | Keep |
|---|---|---|
| Wayland/X11 GTK window handling + run scripts (SSH dev) | justfile run recipes |  |
| ONNX runtime dylib download/build + model dirs (HF models) | build.rs, ORT_DYLIB_PATH, MODELS_DIR |  |
| Headless export launcher request | launch_request.rs |  |
| Window customizer | window_customizer.rs |  |
| Tauri plugin set (dialog/os/process/shell/opener) | Cargo.toml plugins |  |
| Community preset server fetch | `save_community_preset`, CommunityPage |  |
| Tethering compile gate (`tethering` feature) | engine/src-tauri features |  |
