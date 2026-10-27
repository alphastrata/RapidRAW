use crate::panorama::{
    ImageInfo, MIN_INLIERS_FOR_CONNECTION, MatchInfo, compute_homography, find_homography_ransac,
    match_features,
};
use anyhow::{Result, anyhow, bail};
use image::{GrayImage, Rgb, Rgb32FImage};
use log::debug;
use nalgebra::{Matrix3, Point3};
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

const FEATHER_WIDTH: f64 = 100.0;
const LOW_DETAIL_FEATHER_SCALE: f64 = 5.0;
const SEMICIRCLE_PI: f64 = std::f64::consts::PI;

pub fn find_pairwise_matches(image_data: &[ImageInfo]) -> HashMap<(usize, usize), MatchInfo> {
    (0..image_data.len())
        .flat_map(|i| (i + 1..image_data.len()).map(move |j| (i, j)))
        .collect::<Vec<_>>()
        .par_iter()
        .filter_map(|&(i, j)| pair_homography(&image_data[i], &image_data[j]).map(|m| ((i, j), m)))
        .collect()
}

fn pair_homography(info1: &ImageInfo, info2: &ImageInfo) -> Option<MatchInfo> {
    let initial_matches = match_features(&info1.features, &info2.features);
    if initial_matches.len() < MIN_INLIERS_FOR_CONNECTION {
        return None;
    }

    let keypoints_of = |info: &ImageInfo| {
        info.features
            .iter()
            .map(|feature| feature.keypoint)
            .collect::<Vec<_>>()
    };
    let (keypoints1, keypoints2) = (keypoints_of(info1), keypoints_of(info2));

    let inliers = find_homography_ransac(&initial_matches, &keypoints1, &keypoints2)?.1;
    if inliers.len() < MIN_INLIERS_FOR_CONNECTION {
        return None;
    }

    let inlier_points: Vec<(nalgebra::Point2<f64>, nalgebra::Point2<f64>)> = inliers
        .iter()
        .map(|m| {
            let p1 = keypoints1[m.index1];
            let p2 = keypoints2[m.index2];
            (
                nalgebra::Point2::new(f64::from(p1.x), f64::from(p1.y)),
                nalgebra::Point2::new(f64::from(p2.x), f64::from(p2.y)),
            )
        })
        .collect();

    let h_refined = compute_homography(&inlier_points)?;
    let (s1, s2) = (info1.scale_factor, info2.scale_factor);
    let scale_mat_i_inv = Matrix3::new(1.0 / s1, 0.0, 0.0, 0.0, 1.0 / s1, 0.0, 0.0, 0.0, 1.0);
    let scale_mat_j = Matrix3::new(s2, 0.0, 0.0, 0.0, s2, 0.0, 0.0, 0.0, 1.0);

    Some(MatchInfo {
        homography: scale_mat_j * h_refined * scale_mat_i_inv,
        inliers: inliers.len(),
    })
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        if self.parent[i] == i {
            i
        } else {
            self.parent[i] = self.find(self.parent[i]);
            self.parent[i]
        }
    }

    fn union(&mut self, i: usize, j: usize) {
        let (root_i, root_j) = (self.find(i), self.find(j));
        if root_i != root_j {
            self.parent[root_i] = root_j;
        }
    }
}

pub fn build_stitching_order(
    image_count: usize,
    matches: &HashMap<(usize, usize), MatchInfo>,
) -> Result<(Vec<usize>, Vec<Matrix3<f64>>)> {
    match image_count {
        0 => Ok((Vec::new(), Vec::new())),
        1 => Ok((vec![0], vec![Matrix3::identity()])),
        _ => order_by_minimum_spanning_tree(image_count, matches),
    }
}

fn order_by_minimum_spanning_tree(
    n: usize,
    matches: &HashMap<(usize, usize), MatchInfo>,
) -> Result<(Vec<usize>, Vec<Matrix3<f64>>)> {
    let mst_adj = build_minimum_spanning_tree(n, matches);

    let start_node = (0..n)
        .filter(|i| mst_adj.contains_key(i))
        .min_by_key(|&i| mst_adj.get(&i).map_or(usize::MAX, |v| v.len()))
        .unwrap_or(0);

    let mut ordered_indices = Vec::with_capacity(n);
    let mut global_homographies = Vec::with_capacity(n);
    let mut queue = VecDeque::with_capacity(n);
    let mut visited = vec![false; n];

    queue.push_back((start_node, Matrix3::identity()));
    visited[start_node] = true;

    while let Some((u, h_u_global)) = queue.pop_front() {
        ordered_indices.push(u);
        global_homographies.push(h_u_global);

        let neighbours = mst_adj.get(&u).cloned().unwrap_or_default();
        let unvisited: Vec<usize> = neighbours.into_iter().filter(|&v| !visited[v]).collect();
        unvisited.into_iter().try_for_each(|v| -> Result<()> {
            visited[v] = true;
            let h_vu = neighbour_homography(u, v, matches)?;
            queue.push_back((v, h_u_global * h_vu));
            Ok(())
        })?;
    }

    Ok((ordered_indices, global_homographies))
}

fn neighbour_homography(
    u: usize,
    v: usize,
    matches: &HashMap<(usize, usize), MatchInfo>,
) -> Result<Matrix3<f64>> {
    match (matches.get(&(v, u)), matches.get(&(u, v))) {
        (Some(m), _) => Ok(m.homography),
        (None, Some(m)) => m
            .homography
            .try_inverse()
            .ok_or_else(|| anyhow!("failed to invert homography for edge {} -> {}", u, v)),
        (None, None) => bail!("missing match for edge {} -> {}", u, v),
    }
}

fn build_minimum_spanning_tree(
    n: usize,
    matches: &HashMap<(usize, usize), MatchInfo>,
) -> HashMap<usize, Vec<usize>> {
    let mut edges: Vec<(usize, usize, usize)> = matches
        .iter()
        .map(|(&(i, j), m)| (m.inliers, i, j))
        .collect();
    edges.sort_by_key(|&(inliers, _, _)| std::cmp::Reverse(inliers));

    let (mst_edges, _dsu) = edges.iter().fold(
        (Vec::with_capacity(n - 1), Dsu::new(n)),
        |(mut mst_edges, mut dsu), &(_, i, j)| {
            if mst_edges.len() < n - 1 && dsu.find(i) != dsu.find(j) {
                dsu.union(i, j);
                mst_edges.push((i, j));
            }
            (mst_edges, dsu)
        },
    );

    mst_edges
        .iter()
        .flat_map(|&(i, j)| [(i, j), (j, i)])
        .fold(HashMap::new(), |mut adj, (a, b)| {
            adj.entry(a).or_default().push(b);
            adj
        })
}

struct OverlapCtx<'a> {
    pano: &'a Rgb32FImage,
    pano_mask: &'a GrayImage,
    img_to_add: &'a Rgb32FImage,
    h_inv: &'a Matrix3<f64>,
    offset_x: f64,
    offset_y: f64,
}

struct SeamAlignment {
    vertical: bool,
    coords: Vec<i32>,
    dx: f64,
    dy: f64,
}

struct BlendOptions {
    seam_coords: Option<Vec<i32>>,
    vertical: bool,
    is_dominant: bool,
}

pub fn progressive_seam_stitcher(
    images: &[&ImageInfo],
    global_homographies: &[Matrix3<f64>],
    on_progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Rgb32FImage> {
    if images.len() != global_homographies.len() {
        bail!("homography count must match image count");
    }
    if images.is_empty() {
        return Ok(Rgb32FImage::new(0, 0));
    }

    let (min_x, max_x, min_y, max_y) = canvas_extents(images, global_homographies);
    let (offset_x, offset_y) = (-min_x, -min_y);
    let out_dims = ((max_x - min_x).ceil() as u32, (max_y - min_y).ceil() as u32);
    let out_width_u = usize::try_from(out_dims.0).unwrap_or(0);
    debug!("output canvas size: {}x{}", out_dims.0, out_dims.1);

    let mut panorama = Rgb32FImage::new(out_dims.0, out_dims.1);
    let mut panorama_mask = GrayImage::new(out_dims.0, out_dims.1);

    place_base_image(
        &mut panorama,
        &mut panorama_mask,
        images[0],
        &global_homographies[0],
        offset_x,
        offset_y,
        out_width_u * 3,
    );

    images
        .iter()
        .zip(global_homographies.iter())
        .skip(1)
        .enumerate()
        .try_for_each(|(i, (img_info, h_add))| -> Result<()> {
            (on_progress)(i, images.len());
            debug!(
                "progressively stitching image {} of {}",
                i + 2,
                images.len()
            );

            let Some(h_add_inv) = h_add.try_inverse() else {
                bail!("failed to invert homography for image {}", i + 1);
            };
            let seam_alignment = {
                let seam_ctx = OverlapCtx {
                    pano: &panorama,
                    pano_mask: &panorama_mask,
                    img_to_add: &img_info.image,
                    h_inv: &h_add_inv,
                    offset_x,
                    offset_y,
                };
                find_adaptive_seam(&seam_ctx)
            };
            let blend_options = match seam_alignment {
                Some(SeamAlignment { coords, .. }) if coords.is_empty() => {
                    debug!("could not find seam; using simple overwrite.");
                    BlendOptions {
                        seam_coords: None,
                        vertical: true,
                        is_dominant: true,
                    }
                }
                Some(SeamAlignment {
                    vertical,
                    coords,
                    dx,
                    dy,
                }) => {
                    debug!(
                        "new image is on the {} side of the seam.",
                        if vertical { "right" } else { "bottom" }
                    );
                    BlendOptions {
                        seam_coords: Some(coords),
                        vertical,
                        is_dominant: if vertical { dx > 0.0 } else { dy > 0.0 },
                    }
                }
                None => {
                    debug!("could not find seam; using simple overwrite.");
                    BlendOptions {
                        seam_coords: None,
                        vertical: true,
                        is_dominant: true,
                    }
                }
            };
            blend_image(
                &mut panorama,
                &mut panorama_mask,
                &img_info.image,
                &img_info.low_detail_mask,
                &h_add_inv,
                offset_x,
                offset_y,
                blend_options,
            );
            Ok(())
        })?;

    Ok(panorama)
}

fn canvas_extents(
    images: &[&ImageInfo],
    global_homographies: &[Matrix3<f64>],
) -> (f64, f64, f64, f64) {
    images
        .iter()
        .zip(global_homographies.iter())
        .flat_map(|(info, h)| {
            let (w, h_img) = info.image.dimensions();
            [
                (0.0, 0.0),
                (f64::from(w), 0.0),
                (f64::from(w), f64::from(h_img)),
                (0.0, f64::from(h_img)),
            ]
            .map(|(x, y)| {
                let mapped = h * Point3::new(x, y, 1.0);
                (mapped.x / mapped.z, mapped.y / mapped.z)
            })
        })
        .fold(
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, max_x, min_y, max_y), (tx, ty)| {
                (min_x.min(tx), max_x.max(tx), min_y.min(ty), max_y.max(ty))
            },
        )
}

fn place_base_image(
    panorama: &mut Rgb32FImage,
    panorama_mask: &mut GrayImage,
    base_info: &ImageInfo,
    h_base: &Matrix3<f64>,
    offset_x: f64,
    offset_y: f64,
    row_pixels: usize,
) {
    let Some(h_base_inv) = h_base.try_inverse() else {
        debug!("failed to invert base homography; skipping base placement");
        return;
    };
    debug!("placing base image");
    let base_image = &base_info.image;
    let mask_row_width = usize::try_from(panorama.width()).unwrap_or(0);
    panorama
        .par_chunks_mut(row_pixels)
        .zip(panorama_mask.par_chunks_mut(mask_row_width))
        .enumerate()
        .for_each(|(y, (row, mask_row))| {
            row.chunks_exact_mut(3)
                .zip(mask_row.iter_mut())
                .enumerate()
                .for_each(|(x, (chunk, mask_px))| {
                    let x_f = x as f64;
                    let y_f = y as f64;
                    let source = h_base_inv * Point3::new(x_f - offset_x, y_f - offset_y, 1.0);
                    let (sx, sy) = (source.x / source.z, source.y / source.z);
                    if source_contained(base_image, sx, sy) {
                        chunk.copy_from_slice(&get_interpolated_pixel(base_image, sx, sy).0);
                        *mask_px = 255;
                    }
                });
        });
}

fn blend_image(
    panorama: &mut Rgb32FImage,
    panorama_mask: &mut GrayImage,
    img_to_add: &Rgb32FImage,
    low_detail_mask: &GrayImage,
    h_add_inv: &Matrix3<f64>,
    offset_x: f64,
    offset_y: f64,
    options: BlendOptions,
) {
    let BlendOptions {
        seam_coords,
        vertical,
        is_dominant,
    } = options;
    let use_seam = seam_coords.is_some();
    let out_width_u = usize::try_from(panorama.width()).unwrap_or(0);

    panorama
        .par_chunks_mut(out_width_u * 3)
        .zip(panorama_mask.par_chunks_mut(out_width_u))
        .enumerate()
        .for_each(|(y, (row, mask_row))| {
            row.chunks_exact_mut(3)
                .zip(mask_row.iter_mut())
                .enumerate()
                .for_each(|(x, (chunk, mask_px))| {
                    let x_f = x as f64;
                    let y_f = y as f64;
                    let target = Point3::new(x_f - offset_x, y_f - offset_y, 1.0);
                    let source = h_add_inv * target;
                    let (sx, sy) = (source.x / source.z, source.y / source.z);
                    let on_add = source_contained(img_to_add, sx, sy);
                    let on_pano = *mask_px > 0;

                    match (&seam_coords, on_add, on_pano && use_seam) {
                        (Some(coords), true, true) => {
                            let seam_pixel = &mut SeamPixel {
                                chunk,
                                pos: if vertical { x_f } else { y_f },
                                index: if vertical { y } else { x },
                                sx,
                                sy,
                                coords,
                                img_to_add,
                                low_detail_mask,
                                is_dominant,
                            };
                            blend_seam_pixel(seam_pixel);
                        }
                        (_, true, _) => {
                            chunk.copy_from_slice(&get_interpolated_pixel(img_to_add, sx, sy).0);
                            *mask_px = 255;
                        }
                        _ => {}
                    }
                });
        });
}

struct SeamPixel<'a> {
    chunk: &'a mut [f32],
    pos: f64,
    index: usize,
    sx: f64,
    sy: f64,
    coords: &'a [i32],
    img_to_add: &'a Rgb32FImage,
    low_detail_mask: &'a GrayImage,
    is_dominant: bool,
}

fn blend_seam_pixel(px: &mut SeamPixel) {
    let seam_val = f64::from(px.coords[px.index]);
    let dist_to_seam = px.pos - seam_val;
    let feather_width = if is_low_detail(px.low_detail_mask, px.sx, px.sy) {
        FEATHER_WIDTH * LOW_DETAIL_FEATHER_SCALE
    } else {
        FEATHER_WIDTH
    };

    if dist_to_seam.abs() < feather_width / 2.0 {
        let half_width = feather_width / 2.0;
        let alpha = (if px.is_dominant {
            dist_to_seam + half_width
        } else {
            -dist_to_seam + half_width
        }) / feather_width;
        let weight_add = (1.0 - (alpha.clamp(0.0, 1.0) * SEMICIRCLE_PI).cos()) / 2.0;
        let weight_pano = 1.0 - weight_add;
        let (w_p, w_a) = (weight_pano as f32, weight_add as f32);
        let colour_to_add = get_interpolated_pixel(px.img_to_add, px.sx, px.sy);
        let blended = [
            px.chunk[0] * w_p + colour_to_add[0] * w_a,
            px.chunk[1] * w_p + colour_to_add[1] * w_a,
            px.chunk[2] * w_p + colour_to_add[2] * w_a,
        ];
        px.chunk.copy_from_slice(&blended);
    } else if (px.pos > seam_val) == px.is_dominant {
        let colour_to_add = get_interpolated_pixel(px.img_to_add, px.sx, px.sy);
        px.chunk.copy_from_slice(&colour_to_add.0);
    }
}

fn find_adaptive_seam(ctx: &OverlapCtx) -> Option<SeamAlignment> {
    let (w_add, h_add_img) = ctx.img_to_add.dimensions();
    let (w_add_f, h_add_img_f) = (f64::from(w_add), f64::from(h_add_img));

    let bounds: Option<(u32, u32, u32, u32)> = ctx
        .pano_mask
        .enumerate_pixels()
        .filter(|&(_, _, mask)| mask[0] > 0)
        .filter_map(|(x, y, _)| {
            let target = Point3::new(
                f64::from(x) - ctx.offset_x,
                f64::from(y) - ctx.offset_y,
                1.0,
            );
            let source = ctx.h_inv * target;
            let (sx, sy) = (source.x / source.z, source.y / source.z);
            (sx >= 0.0 && sx < w_add_f && sy >= 0.0 && sy < h_add_img_f).then_some((x, y))
        })
        .fold(None, |acc, (x, y)| match acc {
            Some((min_ox, max_ox, min_oy, max_oy)) => {
                Some((min_ox.min(x), max_ox.max(x), min_oy.min(y), max_oy.max(y)))
            }
            None => Some((x, x, y, y)),
        });
    let Some((min_ox, max_ox, min_oy, max_oy)) = bounds else {
        debug!("no overlap found; skipping seam search");
        return None;
    };

    let centre_p_source = Point3::new(f64::from(w_add) / 2.0, f64::from(h_add_img) / 2.0, 1.0);
    let centre_p_target = ctx.h_inv * centre_p_source;
    let (centre_add_x, centre_add_y) = (
        centre_p_target.x / centre_p_target.z + ctx.offset_x,
        centre_p_target.y / centre_p_target.z + ctx.offset_y,
    );
    let (centre_overlap_x, centre_overlap_y) = (
        f64::from(min_ox + max_ox) / 2.0,
        f64::from(min_oy + max_oy) / 2.0,
    );

    let (dx, dy) = (
        centre_add_x - centre_overlap_x,
        centre_add_y - centre_overlap_y,
    );
    let vertical = dx.abs() > dy.abs();
    debug!(
        "overlap is {}; finding {} seam...",
        if vertical { "vertical" } else { "horizontal" },
        if vertical { "vertical" } else { "horizontal" }
    );

    let coords = if vertical {
        find_pairwise_seam_dp_vertical(ctx)
    } else {
        find_pairwise_seam_dp_horizontal(ctx)
    };
    Some(SeamAlignment {
        vertical,
        coords,
        dx,
        dy,
    })
}

fn seam_energy(ctx: &OverlapCtx, x: u32, y: u32) -> Option<(usize, usize, f64)> {
    let (w_add, h_add_img) = ctx.img_to_add.dimensions();
    let (w_add_f, h_add_img_f) = (f64::from(w_add) - 1.0, f64::from(h_add_img) - 1.0);
    let target = Point3::new(
        f64::from(x) - ctx.offset_x,
        f64::from(y) - ctx.offset_y,
        1.0,
    );
    let source = ctx.h_inv * target;
    let (sx, sy) = (source.x / source.z, source.y / source.z);
    if !(sx >= 0.0 && sx < w_add_f && sy >= 0.0 && sy < h_add_img_f) {
        return None;
    }
    let p_add = get_interpolated_pixel(ctx.img_to_add, sx, sy);
    let p_pano = ctx.pano.get_pixel(x, y);
    let energy = ((f64::from(p_pano[0]) - f64::from(p_add[0])).powi(2)
        + (f64::from(p_pano[1]) - f64::from(p_add[1])).powi(2)
        + (f64::from(p_pano[2]) - f64::from(p_add[2])).powi(2))
    .sqrt();
    Some((
        usize::try_from(x).unwrap_or(0),
        usize::try_from(y).unwrap_or(0),
        energy,
    ))
}

fn find_pairwise_seam_dp_vertical(ctx: &OverlapCtx) -> Vec<i32> {
    let (out_width, out_height) = ctx.pano.dimensions();
    let (out_w, out_h) = (
        usize::try_from(out_width).unwrap_or(0),
        usize::try_from(out_height).unwrap_or(0),
    );
    if out_w == 0 || out_h == 0 {
        return Vec::new();
    }

    let mut cost = vec![vec![f64::INFINITY; out_w]; out_h];
    let mut path = vec![vec![0i32; out_w]; out_h];

    let energies: Vec<(usize, usize, f64)> = ctx
        .pano_mask
        .enumerate_pixels()
        .filter(|&(_, _, mask)| mask[0] > 0)
        .filter_map(|(x, y, _)| seam_energy(ctx, x, y))
        .collect();

    energies.iter().for_each(|&(x, y, energy)| {
        cost[y][x] = energy;
    });
    let (first_row, last_row) = energies
        .iter()
        .fold((usize::MAX, 0), |(min_row, max_row), &(_, y, _)| {
            (min_row.min(y), max_row.max(y))
        });
    if first_row == usize::MAX {
        return Vec::new();
    }

    (first_row + 1..=last_row).for_each(|y| {
        let (prev_rows, cur_rows) = cost.split_at_mut(y);
        let (prev, cur) = (&prev_rows[y - 1], &mut cur_rows[0]);
        let path_cur = &mut path.split_at_mut(y).1[0];
        let active: Vec<usize> = (0..out_w).filter(|&x| cur[x].is_finite()).collect();
        active.into_iter().for_each(|x| {
            let up_left = if x > 0 { prev[x - 1] } else { f64::INFINITY };
            let up = prev[x];
            let up_right = if x + 1 < out_w {
                prev[x + 1]
            } else {
                f64::INFINITY
            };
            let min_cost = up.min(up_left).min(up_right);
            if min_cost.is_finite() {
                cur[x] += min_cost;
                path_cur[x] = if min_cost == up {
                    0
                } else if min_cost == up_left {
                    -1
                } else {
                    1
                };
            }
        });
    });

    let Some((current_x, min_cost)) = cost[last_row]
        .iter()
        .copied()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return Vec::new();
    };
    if !min_cost.is_finite() {
        return Vec::new();
    }

    let mut seam = vec![0i32; out_h];
    let mut x_pos = current_x;
    (first_row..=last_row).rev().for_each(|y| {
        seam[y] = x_pos as i32;
        let next = x_pos as i64 + i64::from(path[y][x_pos]);
        let max_index = i64::from(u32::try_from(out_w).unwrap_or(1)) - 1;
        x_pos = usize::try_from(next.clamp(0, max_index)).unwrap_or(0);
    });
    let first_seam = seam[first_row];
    (0..first_row).rev().for_each(|y| seam[y] = first_seam);
    let last_seam = seam[last_row];
    (last_row + 1..out_h).for_each(|y| seam[y] = last_seam);
    seam
}

fn find_pairwise_seam_dp_horizontal(ctx: &OverlapCtx) -> Vec<i32> {
    let (out_width, out_height) = ctx.pano.dimensions();
    let (out_w, out_h) = (
        usize::try_from(out_width).unwrap_or(0),
        usize::try_from(out_height).unwrap_or(0),
    );
    if out_w == 0 || out_h == 0 {
        return Vec::new();
    }

    let mut cost = vec![vec![f64::INFINITY; out_w]; out_h];
    let mut path = vec![vec![0i32; out_w]; out_h];

    let energies: Vec<(usize, usize, f64)> = ctx
        .pano_mask
        .enumerate_pixels()
        .filter(|&(_, _, mask)| mask[0] > 0)
        .filter_map(|(x, y, _)| seam_energy(ctx, x, y))
        .collect();

    energies.iter().for_each(|&(x, y, energy)| {
        cost[y][x] = energy;
    });
    let (first_col, last_col) = energies
        .iter()
        .fold((usize::MAX, 0), |(min_col, max_col), &(x, _, _)| {
            (min_col.min(x), max_col.max(x))
        });
    if first_col == usize::MAX {
        return Vec::new();
    }

    (first_col + 1..=last_col).for_each(|x| {
        let eligible: Vec<usize> = (0..out_h).filter(|&y| cost[y][x].is_finite()).collect();
        eligible.into_iter().for_each(|y| {
            let left_up = if y > 0 {
                cost[y - 1][x - 1]
            } else {
                f64::INFINITY
            };
            let left = cost[y][x - 1];
            let left_down = if y + 1 < out_h {
                cost[y + 1][x - 1]
            } else {
                f64::INFINITY
            };
            let min_cost = left.min(left_up).min(left_down);
            if min_cost.is_finite() {
                cost[y][x] += min_cost;
                path[y][x] = if min_cost == left {
                    0
                } else if min_cost == left_up {
                    -1
                } else {
                    1
                };
            }
        });
    });

    let Some((current_y, min_cost)) = cost
        .iter()
        .enumerate()
        .min_by(|a, b| a.1[last_col].total_cmp(&b.1[last_col]))
        .map(|(y, row)| (y, row[last_col]))
    else {
        return Vec::new();
    };
    if !min_cost.is_finite() {
        return Vec::new();
    }

    let mut seam = vec![0i32; out_w];
    let mut y_pos = current_y;
    (first_col..=last_col).rev().for_each(|x| {
        seam[x] = y_pos as i32;
        let next = y_pos as i64 + i64::from(path[y_pos][x]);
        let max_index = i64::from(u32::try_from(out_h).unwrap_or(1)) - 1;
        y_pos = usize::try_from(next.clamp(0, max_index)).unwrap_or(0);
    });
    let first_seam = seam[first_col];
    (0..first_col).rev().for_each(|x| seam[x] = first_seam);
    let last_seam = seam[last_col];
    (last_col + 1..out_w).for_each(|x| seam[x] = last_seam);
    seam
}

pub fn warp_image_homography(
    source: &Rgb32FImage,
    homography: &Matrix3<f64>,
    width: u32,
    height: u32,
) -> Result<Rgb32FImage> {
    let (width_u, height_u) = (
        usize::try_from(width).unwrap_or(0),
        usize::try_from(height).unwrap_or(0),
    );
    if width_u == 0 || height_u == 0 {
        bail!("warp output must be non-empty");
    }

    let mut buffer = vec![0.0f32; width_u * height_u * 3];
    buffer
        .par_chunks_mut(width_u * 3)
        .enumerate()
        .for_each(|(y, row)| {
            row.chunks_exact_mut(3).enumerate().for_each(|(x, chunk)| {
                let x_f = x as f64;
                let y_f = y as f64;
                let mapped = homography * Point3::new(x_f, y_f, 1.0);
                let pixel = if mapped.z.abs() < 1e-8 {
                    [0.0f32; 3]
                } else {
                    get_interpolated_pixel(source, mapped.x / mapped.z, mapped.y / mapped.z).0
                };
                chunk.copy_from_slice(&pixel);
            });
        });
    Rgb32FImage::from_raw(width, height, buffer)
        .ok_or_else(|| anyhow!("warp buffer dimensions must match output image"))
}

fn source_contained(img: &Rgb32FImage, x: f64, y: f64) -> bool {
    let (width, height) = img.dimensions();
    (0.0..f64::from(width)).contains(&x) && (0.0..f64::from(height)).contains(&y)
}

fn is_low_detail(mask: &GrayImage, x: f64, y: f64) -> bool {
    mask.get_pixel(
        clamp_to_u32(x, mask.width()),
        clamp_to_u32(y, mask.height()),
    )[0] > 0
}

fn clamp_to_u32(value: f64, max: u32) -> u32 {
    let clamped = value.max(0.0).round().min(f64::from(max) - 1.0);
    clamped as u32
}

fn u32_floor(value: f64) -> u32 {
    value as u32
}

fn get_interpolated_pixel(img: &Rgb32FImage, x: f64, y: f64) -> Rgb<f32> {
    let (width, height) = img.dimensions();
    let (w_f, h_f) = (f64::from(width), f64::from(height));
    let (x_floor, y_floor) = (x.floor(), y.floor());
    let uninterpolatable = x < 0.0 || y < 0.0 || x_floor + 1.0 >= w_f || y_floor + 1.0 >= h_f;
    if uninterpolatable {
        let (sx, sy) = (x.max(0.0).min(w_f - 1.0), y.max(0.0).min(h_f - 1.0));
        return *img.get_pixel(u32_floor(sx), u32_floor(sy));
    }

    let (dx, dy) = (x - x_floor, y - y_floor);
    let (x_u, y_u) = (u32_floor(x_floor), u32_floor(y_floor));
    let (p00, p10, p01, p11) = (
        *img.get_pixel(x_u, y_u),
        *img.get_pixel(x_u + 1, y_u),
        *img.get_pixel(x_u, y_u + 1),
        *img.get_pixel(x_u + 1, y_u + 1),
    );

    let blended = (0..3).fold([0.0f32; 3], |mut acc, channel| {
        let top = f64::from(p00[channel]) * (1.0 - dx) + f64::from(p10[channel]) * dx;
        let bottom = f64::from(p01[channel]) * (1.0 - dx) + f64::from(p11[channel]) * dx;
        acc[channel] = (top * (1.0 - dy) + bottom * dy) as f32;
        acc
    });
    Rgb(blended)
}
