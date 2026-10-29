//! Looking at a compiled PDF without a viewer: its page count and sizes, and pages rendered to
//! PNG, one at a time, as a range, or stitched into a single image. This is how an agent reads
//! what a document actually looks like. It uses `pdfinfo` and `pdftoppm` (Poppler), which ship
//! with MiKTeX and TeX Live, so no PDF engine is bundled.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::tex::TexInstall;
use crate::{Error, Result};

/// Most pages one call will render. A document longer than this is read in ranges.
pub const MAX_PAGES: usize = 30;
/// Largest stitched image, in pixels. Bigger means fewer pages per image, not a blurrier one.
pub const MAX_CANVAS_PIXELS: u64 = 36_000_000;
const GAP: u32 = 16;
const GAP_GREY: u8 = 0x80;

#[derive(Debug, Clone, PartialEq)]
pub struct PdfInfo {
    pub pages: u32,
    /// Width and height in PDF points (1/72 inch) of every page, in order.
    pub sizes: Vec<(f64, f64)>,
    pub title: Option<String>,
}

/// A rendered page: tightly packed 8-bit RGB.
#[derive(Debug, Clone)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One image per page.
    Separate,
    /// Every page in one image, in a grid of this many columns.
    Grid { columns: usize },
}

pub struct PdfTools {
    pdfinfo: PathBuf,
    pdftoppm: PathBuf,
}

impl PdfTools {
    /// Find Poppler's tools in the TeX install, then on PATH.
    pub fn find(install: &TexInstall) -> Result<PdfTools> {
        let locate = |name: &str| -> Result<PathBuf> {
            let exe = format!("{name}.exe");
            let mut dirs = vec![install.bin_dir.clone()];
            if let Some(path) = std::env::var_os("PATH") {
                dirs.extend(std::env::split_paths(&path));
            }
            dirs.iter()
                .flat_map(|d| [d.join(&exe), d.join(name)])
                .find(|p| p.is_file())
                .ok_or_else(|| {
                    Error::Pdf(format!(
                        "`{name}` was not found. It comes with MiKTeX (Poppler utilities) and TeX Live; \
                         install it to look at pages."
                    ))
                })
        };
        Ok(PdfTools { pdfinfo: locate("pdfinfo")?, pdftoppm: locate("pdftoppm")? })
    }

    pub fn info(&self, pdf: &Path) -> Result<PdfInfo> {
        // `-f 1 -l 9999` makes pdfinfo print the size of every page, not only the first.
        let text = run(&self.pdfinfo, &["-f", "1", "-l", "9999"], pdf, None)?;
        parse_info(&text)
    }

    /// Render the 1-based `pages` at `dpi`.
    pub fn render(&self, pdf: &Path, pages: &[u32], dpi: u32) -> Result<Vec<Raster>> {
        let scratch = tempfile::Builder::new()
            .prefix("moss-pages-")
            .tempdir()
            .map_err(|e| Error::io("creating a scratch folder for page images", e))?;
        let mut out = Vec::new();
        for &page in pages {
            let root = scratch.path().join(format!("p{page}"));
            let args = ["-r", &dpi.to_string(), "-f", &page.to_string(), "-l", &page.to_string(), "-singlefile"];
            run(&self.pdftoppm, &args, pdf, Some(&root))?;
            let ppm = std::fs::read(root.with_extension("ppm"))
                .map_err(|e| Error::io(format!("reading rendered page {page}"), e))?;
            out.push(parse_ppm(&ppm)?);
        }
        Ok(out)
    }
}

fn run(exe: &Path, args: &[&str], pdf: &Path, out_root: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).arg(pdf);
    if let Some(root) = out_root {
        cmd.arg(root);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .output()
        .map_err(|e| Error::io(format!("starting {}", exe.display()), e))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Pdf(format!(
            "{} failed: {}",
            exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            why.lines().next().unwrap_or("no message")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `pdfinfo -f 1 -l N` output: a `Pages:` line and one `Page   N size: W x H pts` line per page.
pub fn parse_info(text: &str) -> Result<PdfInfo> {
    let mut pages = None;
    let mut title = None;
    let mut sizes = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            pages = rest.trim().parse::<u32>().ok();
        } else if let Some(rest) = line.strip_prefix("Title:") {
            let t = rest.trim();
            if !t.is_empty() {
                title = Some(t.to_string());
            }
        } else if line.starts_with("Page") && line.contains(" size:") {
            let Some((_, dims)) = line.split_once("size:") else { continue };
            let mut it = dims.split_whitespace();
            let (w, _x, h) = (it.next(), it.next(), it.next());
            if let (Some(w), Some(h)) = (w.and_then(|v| v.parse::<f64>().ok()), h.and_then(|v| v.parse::<f64>().ok())) {
                sizes.push((w, h));
            }
        }
    }
    let pages = pages.ok_or_else(|| Error::Pdf("pdfinfo did not report a page count".into()))?;
    Ok(PdfInfo { pages, sizes, title })
}

/// Which pages a spec means, against a document of `total` pages. Accepts `all`, `last`, `3`,
/// `2-5`, `4-` (to the end), and comma lists such as `1,3,5-7`. Sorted, no repeats.
pub fn parse_pages(spec: &str, total: u32) -> Result<Vec<u32>> {
    let spec = spec.trim().to_ascii_lowercase();
    if total == 0 {
        return Err(Error::Pdf("the PDF has no pages".into()));
    }
    let bad = |part: &str| Error::Pdf(format!("`{part}` is not a page or range; use 3, 2-5, 4-, last, all or 1,3,5-7"));
    let page = |s: &str, part: &str| -> Result<u32> {
        match s.trim() {
            "last" => Ok(total),
            n => n.parse::<u32>().map_err(|_| bad(part)),
        }
    };
    let mut set = std::collections::BTreeSet::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (from, to) = if part == "all" {
            (1, total)
        } else if let Some((a, b)) = part.split_once('-') {
            (page(a, part)?, if b.trim().is_empty() { total } else { page(b, part)? })
        } else {
            let n = page(part, part)?;
            (n, n)
        };
        if from == 0 || from > to {
            return Err(bad(part));
        }
        if to > total {
            return Err(Error::Pdf(format!("page {to} does not exist; the document has {total} page(s)")));
        }
        set.extend(from..=to);
    }
    if set.is_empty() {
        return Err(bad(&spec));
    }
    Ok(set.into_iter().collect())
}

/// A resolution that fits the pages into a `max_width_px`-wide image at `columns` per row, never
/// above `ceiling` (a sharper picture than that adds bytes, not legibility).
pub fn fit_dpi(sizes: &[(f64, f64)], pages: &[u32], columns: usize, max_width_px: u32, ceiling: u32) -> u32 {
    let widest_pt = pages
        .iter()
        .filter_map(|p| sizes.get(*p as usize - 1))
        .map(|s| s.0)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let columns = columns.max(1) as f64;
    let dpi = f64::from(max_width_px) / (columns * widest_pt / 72.0);
    (dpi.floor() as u32).clamp(40, ceiling)
}

/// A reasonable number of columns for `n` pages: one page alone, otherwise a roughly square grid
/// no wider than four, so a whole report reads as a contact sheet.
pub fn default_columns(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        2 => 2,
        _ => ((n as f64).sqrt().ceil() as usize).min(4),
    }
}

/// Lay the pages out in a grid on a mid-grey sheet, page by page, left to right then down.
pub fn grid(pages: &[Raster], columns: usize) -> Result<Raster> {
    if pages.is_empty() {
        return Err(Error::Pdf("no pages to lay out".into()));
    }
    let columns = columns.clamp(1, pages.len());
    let rows = pages.len().div_ceil(columns);
    let cell_w = pages.iter().map(|p| p.width).max().unwrap_or(1);
    let cell_h = pages.iter().map(|p| p.height).max().unwrap_or(1);
    let width = columns as u32 * cell_w + (columns as u32 + 1) * GAP;
    let height = rows as u32 * cell_h + (rows as u32 + 1) * GAP;
    if u64::from(width) * u64::from(height) > MAX_CANVAS_PIXELS {
        return Err(Error::Pdf(format!(
            "{} pages in one image would be {width}x{height} pixels; ask for fewer pages, fewer columns, or a lower dpi",
            pages.len()
        )));
    }
    let mut rgb = vec![GAP_GREY; width as usize * height as usize * 3];
    for (i, page) in pages.iter().enumerate() {
        let (col, row) = ((i % columns) as u32, (i / columns) as u32);
        // Pages of different sizes sit centred in their cell.
        let x0 = GAP + col * (cell_w + GAP) + (cell_w - page.width) / 2;
        let y0 = GAP + row * (cell_h + GAP) + (cell_h - page.height) / 2;
        for y in 0..page.height {
            let src = y as usize * page.width as usize * 3;
            let dst = ((y0 + y) as usize * width as usize + x0 as usize) * 3;
            rgb[dst..dst + page.width as usize * 3].copy_from_slice(&page.rgb[src..src + page.width as usize * 3]);
        }
    }
    Ok(Raster { width, height, rgb })
}

pub fn encode_png(image: &Raster) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, image.width, image.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| Error::Pdf(format!("encoding PNG: {e}")))?;
    writer
        .write_image_data(&image.rgb)
        .map_err(|e| Error::Pdf(format!("encoding PNG: {e}")))?;
    writer.finish().map_err(|e| Error::Pdf(format!("encoding PNG: {e}")))?;
    Ok(out)
}

/// Binary PPM ("P6"), the format `pdftoppm` writes without `-png`.
pub fn parse_ppm(data: &[u8]) -> Result<Raster> {
    let bad = || Error::Pdf("pdftoppm wrote an image Moss cannot read".into());
    let mut pos = 0;
    let mut token = || -> Option<String> {
        loop {
            while data.get(pos).is_some_and(|b| b.is_ascii_whitespace()) {
                pos += 1;
            }
            if data.get(pos) == Some(&b'#') {
                while data.get(pos).is_some_and(|b| *b != b'\n') {
                    pos += 1;
                }
            } else {
                break;
            }
        }
        let start = pos;
        while data.get(pos).is_some_and(|b| !b.is_ascii_whitespace()) {
            pos += 1;
        }
        (pos > start).then(|| String::from_utf8_lossy(&data[start..pos]).into_owned())
    };
    if token().as_deref() != Some("P6") {
        return Err(bad());
    }
    let width: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let height: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let maxval: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    if maxval != 255 || width == 0 || height == 0 {
        return Err(bad());
    }
    // Exactly one whitespace byte separates the header from the pixels.
    let start = pos + 1;
    let len = width as usize * height as usize * 3;
    let rgb = data.get(start..start + len).ok_or_else(bad)?.to_vec();
    Ok(Raster { width, height, rgb })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Raster {
        Raster { width: w, height: h, rgb: vec![v; w as usize * h as usize * 3] }
    }

    #[test]
    fn page_specs_cover_singles_ranges_lists_and_all() {
        assert_eq!(parse_pages("3", 10).unwrap(), vec![3]);
        assert_eq!(parse_pages("2-4", 10).unwrap(), vec![2, 3, 4]);
        assert_eq!(parse_pages("8-", 10).unwrap(), vec![8, 9, 10]);
        assert_eq!(parse_pages("1, 3, 5-6, 6", 10).unwrap(), vec![1, 3, 5, 6]);
        assert_eq!(parse_pages("ALL", 3).unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_pages("last", 7).unwrap(), vec![7]);
        assert_eq!(parse_pages("5-last", 7).unwrap(), vec![5, 6, 7]);
    }

    #[test]
    fn page_specs_reject_nonsense_with_a_hint() {
        for bad in ["", "0", "4-2", "x", "1-y", ",,"] {
            assert!(parse_pages(bad, 10).is_err(), "{bad:?} should be refused");
        }
        let e = parse_pages("11", 10).unwrap_err().to_string();
        assert!(e.contains("10 page"), "{e}");
        assert!(parse_pages("1", 0).is_err());
    }

    #[test]
    fn pdfinfo_text_gives_pages_and_every_size() {
        let text = "Title:          My Report\nPages:          2\nPage    1 size: 595.276 x 841.89 pts (A4)\nPage    1 rot:  0\nPage    2 size: 841.89 x 595.276 pts (A4)\n";
        let info = parse_info(text).unwrap();
        assert_eq!(info.pages, 2);
        assert_eq!(info.title.as_deref(), Some("My Report"));
        assert_eq!(info.sizes, vec![(595.276, 841.89), (841.89, 595.276)]);
        assert!(parse_info("nothing useful").is_err());
    }

    #[test]
    fn the_dpi_fits_the_pages_into_the_width_and_is_capped() {
        let a4 = vec![(595.0, 842.0); 4];
        // Four columns of A4 into 3000 px is about 91 dpi.
        let dpi = fit_dpi(&a4, &[1, 2, 3, 4], 4, 3000, 120);
        assert!((85..=95).contains(&dpi), "{dpi}");
        // One page can go up to the ceiling, not beyond.
        assert_eq!(fit_dpi(&a4, &[1], 1, 3000, 120), 120);
        // Never below something legible-ish.
        assert_eq!(fit_dpi(&a4, &[1, 2, 3, 4], 30, 100, 120), 40);
    }

    #[test]
    fn default_columns_make_a_contact_sheet() {
        assert_eq!(
            [1, 2, 3, 4, 5, 9, 10, 30].map(default_columns),
            [1, 2, 2, 2, 3, 3, 4, 4]
        );
    }

    #[test]
    fn a_grid_places_pages_on_grey_and_centres_odd_sizes() {
        let pages = [solid(100, 140, 255), solid(100, 140, 255), solid(60, 80, 0)];
        let sheet = grid(&pages, 2).unwrap();
        assert_eq!((sheet.width, sheet.height), (2 * 100 + 3 * GAP, 2 * 140 + 3 * GAP));
        let px = |x: u32, y: u32| sheet.rgb[(y as usize * sheet.width as usize + x as usize) * 3];
        assert_eq!(px(0, 0), GAP_GREY, "margin is grey");
        assert_eq!(px(GAP + 50, GAP + 70), 255, "first page is white");
        // The small black page sits in the first cell of row two, centred.
        let (cx, cy) = (GAP + 50, 2 * GAP + 140 + 70);
        assert_eq!(px(cx, cy), 0);
        assert_eq!(px(GAP + 5, 2 * GAP + 140 + 5), GAP_GREY, "outside the small page");
    }

    #[test]
    fn a_grid_too_big_to_send_is_refused_with_advice() {
        let pages: Vec<Raster> = (0..2).map(|_| solid(5000, 5000, 255)).collect();
        let e = grid(&pages, 2).unwrap_err().to_string();
        assert!(e.contains("fewer pages"), "{e}");
        assert!(grid(&[], 1).is_err());
    }

    #[test]
    fn ppm_round_trips_through_png() {
        let mut ppm = b"P6\n# made by a test\n3 2\n255\n".to_vec();
        ppm.extend((0..18).map(|i| i as u8 * 10));
        let raster = parse_ppm(&ppm).unwrap();
        assert_eq!((raster.width, raster.height), (3, 2));
        let bytes = encode_png(&raster).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buf).unwrap();
        assert_eq!((frame.width, frame.height), (3, 2));
        assert_eq!(&buf[..18], &raster.rgb[..]);
        assert!(parse_ppm(b"P5\n1 1\n255\n\0").is_err());
        assert!(parse_ppm(b"P6\n2 2\n255\nshort").is_err());
    }

    /// A one-page PDF written by hand; Poppler rebuilds the cross-reference table itself.
    const TINY_PDF: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]>>endobj\n\
trailer<</Root 1 0 R/Size 4>>\n%%EOF\n";

    #[test]
    fn real_poppler_reports_and_renders_a_page_when_installed() {
        let Ok(install) = crate::tex::detect(None) else {
            eprintln!("skipped: no TeX installation");
            return;
        };
        let Ok(tools) = PdfTools::find(&install) else {
            eprintln!("skipped: no Poppler tools");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("tiny.pdf");
        std::fs::write(&pdf, TINY_PDF).unwrap();
        let info = tools.info(&pdf).unwrap();
        assert_eq!(info.pages, 1);
        assert_eq!(info.sizes, vec![(200.0, 100.0)]);
        let pages = tools.render(&pdf, &[1], 72).unwrap();
        assert_eq!((pages[0].width, pages[0].height), (200, 100));
        assert!(tools.render(&pdf, &[2], 72).is_err(), "page 2 does not exist");
    }
}
