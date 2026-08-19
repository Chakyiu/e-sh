//! Install system fonts as egui fallbacks for scripts the bundled faces lack.
//!
//! egui ships Hack / Ubuntu / emoji only (Latin, Cyrillic, emoji). Everything
//! else — CJK, Arabic, Hebrew, Indic, Thai, etc. — needs a system face or it
//! renders as ◻ tofu. We load a complementary stack of `.ttf` / `.otf` / `.ttc`
//! files from well-known OS locations, plus a light directory scan.

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Context, FontData, FontFamily, FontTweak};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Soft cap so we do not pull hundreds of megabytes of faces into the atlas.
const MAX_FALLBACK_FONTS: usize = 24;

/// One coverage group: load the first readable file, then move on.
struct FontGroup {
    /// Stable name used in logs / egui's font map.
    key: &'static str,
    /// `(path, collection_index)` — `.ttc` face 0 is usually Regular.
    candidates: &'static [(&'static str, u32)],
}

fn groups() -> &'static [FontGroup] {
    #[cfg(target_os = "macos")]
    {
        &[
            FontGroup {
                key: "unicode-wide",
                candidates: &[
                    ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
                    ("/Library/Fonts/Arial Unicode.ttf", 0),
                ],
            },
            FontGroup {
                key: "cjk",
                candidates: &[
                    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
                    ("/System/Library/Fonts/STHeiti Light.ttc", 0),
                    (
                        "/System/Library/PrivateFrameworks/FontServices.framework/Versions/A/Resources/Reserved/PingFangUI.ttc",
                        0,
                    ),
                    ("/System/Library/Fonts/CJKSymbolsFallback.ttc", 0),
                ],
            },
            FontGroup {
                key: "japanese",
                candidates: &[("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0)],
            },
            FontGroup {
                key: "korean",
                candidates: &[
                    ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/AppleGothic.ttf", 0),
                ],
            },
            FontGroup {
                key: "arabic",
                candidates: &[
                    ("/System/Library/Fonts/GeezaPro.ttc", 0),
                    ("/System/Library/Fonts/SFArabic.ttf", 0),
                    ("/System/Library/Fonts/NotoNastaliq.ttc", 0),
                ],
            },
            FontGroup {
                key: "hebrew",
                candidates: &[("/System/Library/Fonts/SFHebrew.ttf", 0)],
            },
            FontGroup {
                key: "thai",
                candidates: &[("/System/Library/Fonts/ThonburiUI.ttc", 0)],
            },
            FontGroup {
                key: "devanagari",
                candidates: &[
                    ("/System/Library/Fonts/Kohinoor.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/ITFDevanagari.ttc", 0),
                ],
            },
            FontGroup {
                key: "bengali",
                candidates: &[
                    ("/System/Library/Fonts/KohinoorBangla.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Bangla Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "tamil",
                candidates: &[("/System/Library/Fonts/Supplemental/Tamil Sangam MN.ttc", 0)],
            },
            FontGroup {
                key: "telugu",
                candidates: &[
                    ("/System/Library/Fonts/KohinoorTelugu.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Telugu Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "gujarati",
                candidates: &[
                    ("/System/Library/Fonts/KohinoorGujarati.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Gujarati Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "kannada",
                candidates: &[
                    ("/System/Library/Fonts/NotoSansKannada.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Kannada Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "malayalam",
                candidates: &[("/System/Library/Fonts/Supplemental/Malayalam Sangam MN.ttc", 0)],
            },
            FontGroup {
                key: "gurmukhi",
                candidates: &[
                    ("/System/Library/Fonts/Supplemental/Gurmukhi Sangam MN.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Gurmukhi.ttf", 0),
                ],
            },
            FontGroup {
                key: "oriya",
                candidates: &[
                    ("/System/Library/Fonts/NotoSansOriya.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Oriya Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "sinhala",
                candidates: &[("/System/Library/Fonts/Supplemental/Sinhala Sangam MN.ttc", 0)],
            },
            FontGroup {
                key: "khmer",
                candidates: &[
                    ("/System/Library/Fonts/Supplemental/Khmer Sangam MN.ttf", 0),
                    ("/System/Library/Fonts/Supplemental/Khmer MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "lao",
                candidates: &[
                    ("/System/Library/Fonts/Supplemental/Lao Sangam MN.ttf", 0),
                    ("/System/Library/Fonts/Supplemental/Lao MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "myanmar",
                candidates: &[
                    ("/System/Library/Fonts/NotoSansMyanmar.ttc", 0),
                    ("/System/Library/Fonts/Supplemental/Myanmar Sangam MN.ttc", 0),
                ],
            },
            FontGroup {
                key: "armenian",
                candidates: &[("/System/Library/Fonts/NotoSansArmenian.ttc", 0)],
            },
            FontGroup {
                key: "symbols",
                candidates: &[
                    ("/System/Library/Fonts/Apple Symbols.ttf", 0),
                    ("/System/Library/Fonts/Symbol.ttf", 0),
                ],
            },
        ]
    }
    #[cfg(target_os = "windows")]
    {
        &[
            FontGroup {
                key: "unicode-wide",
                candidates: &[
                    (r"C:\Windows\Fonts\arialuni.ttf", 0),
                    (r"C:\Windows\Fonts\ARIALUNI.TTF", 0),
                    (r"C:\Windows\Fonts\segoeui.ttf", 0),
                    (r"C:\Windows\Fonts\tahoma.ttf", 0),
                ],
            },
            FontGroup {
                key: "cjk",
                candidates: &[
                    (r"C:\Windows\Fonts\msyh.ttc", 0),
                    (r"C:\Windows\Fonts\msyh.ttf", 0),
                    (r"C:\Windows\Fonts\simhei.ttf", 0),
                    (r"C:\Windows\Fonts\simsun.ttc", 0),
                    (r"C:\Windows\Fonts\msjh.ttc", 0),
                    (r"C:\Windows\Fonts\msjh.ttf", 0),
                ],
            },
            FontGroup {
                key: "japanese",
                candidates: &[
                    (r"C:\Windows\Fonts\YuGothM.ttc", 0),
                    (r"C:\Windows\Fonts\yugothm.ttf", 0),
                    (r"C:\Windows\Fonts\msgothic.ttc", 0),
                    (r"C:\Windows\Fonts\meiryo.ttc", 0),
                ],
            },
            FontGroup {
                key: "korean",
                candidates: &[
                    (r"C:\Windows\Fonts\malgun.ttf", 0),
                    (r"C:\Windows\Fonts\malgunsl.ttf", 0),
                    (r"C:\Windows\Fonts\gulim.ttc", 0),
                ],
            },
            FontGroup {
                key: "arabic",
                candidates: &[(r"C:\Windows\Fonts\segoeui.ttf", 0)],
            },
            FontGroup {
                key: "indic",
                candidates: &[
                    (r"C:\Windows\Fonts\Nirmala.ttf", 0),
                    (r"C:\Windows\Fonts\nirmala.ttf", 0),
                    (r"C:\Windows\Fonts\aparaj.ttf", 0),
                    (r"C:\Windows\Fonts\latha.ttf", 0),
                    (r"C:\Windows\Fonts\vrinda.ttf", 0),
                    (r"C:\Windows\Fonts\gautami.ttf", 0),
                ],
            },
            FontGroup {
                key: "thai",
                candidates: &[
                    (r"C:\Windows\Fonts\LeelawUI.ttf", 0),
                    (r"C:\Windows\Fonts\leelawad.ttf", 0),
                    (r"C:\Windows\Fonts\tahoma.ttf", 0),
                ],
            },
            FontGroup {
                key: "african",
                candidates: &[
                    (r"C:\Windows\Fonts\ebrima.ttf", 0),
                    (r"C:\Windows\Fonts\gadugi.ttf", 0),
                ],
            },
            FontGroup {
                key: "symbols",
                candidates: &[
                    (r"C:\Windows\Fonts\seguisym.ttf", 0),
                    (r"C:\Windows\Fonts\seguili.ttf", 0),
                ],
            },
        ]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        &[
            FontGroup {
                key: "unicode-wide",
                candidates: &[
                    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
                    ("/usr/share/fonts/TTF/DejaVuSans.ttf", 0),
                    (
                        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
                        0,
                    ),
                    ("/usr/share/fonts/truetype/freefont/FreeSans.ttf", 0),
                    ("/usr/share/fonts/truetype/unifont/unifont.ttf", 0),
                ],
            },
            FontGroup {
                key: "cjk",
                candidates: &[
                    (
                        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                        0,
                    ),
                    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
                    (
                        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
                        0,
                    ),
                    (
                        "/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/opentype/noto/NotoSansCJKkr-Regular.otf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/opentype/noto/NotoSansCJKtc-Regular.otf",
                        0,
                    ),
                    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
                    (
                        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
                        0,
                    ),
                    ("/usr/share/fonts/truetype/arphic/uming.ttc", 0),
                    (
                        "/usr/share/fonts/opentype/source-han-sans/SourceHanSans-Regular.ttc",
                        0,
                    ),
                ],
            },
            FontGroup {
                key: "arabic",
                candidates: &[
                    (
                        "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/opentype/noto/NotoNaskhArabic-Regular.otf",
                        0,
                    ),
                ],
            },
            FontGroup {
                key: "hebrew",
                candidates: &[(
                    "/usr/share/fonts/truetype/noto/NotoSansHebrew-Regular.ttf",
                    0,
                )],
            },
            FontGroup {
                key: "thai",
                candidates: &[
                    ("/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf", 0),
                    ("/usr/share/fonts/truetype/tlwg/TlwgTypo.ttf", 0),
                    ("/usr/share/fonts/truetype/tlwg/Garuda.ttf", 0),
                ],
            },
            FontGroup {
                key: "indic",
                candidates: &[
                    (
                        "/usr/share/fonts/truetype/noto/NotoSansDevanagari-Regular.ttf",
                        0,
                    ),
                    ("/usr/share/fonts/truetype/lohit-devanagari/Lohit-Devanagari.ttf", 0),
                    ("/usr/share/fonts/truetype/noto/NotoSansBengali-Regular.ttf", 0),
                    ("/usr/share/fonts/truetype/noto/NotoSansTamil-Regular.ttf", 0),
                    ("/usr/share/fonts/truetype/noto/NotoSansTelugu-Regular.ttf", 0),
                ],
            },
            FontGroup {
                key: "symbols",
                candidates: &[
                    (
                        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
                        0,
                    ),
                    (
                        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
                        0,
                    ),
                ],
            },
        ]
    }
}

fn scan_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "macos")]
    {
        dirs.extend([
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/System/Library/Fonts/Supplemental"),
            PathBuf::from("/Library/Fonts"),
        ]);
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs.extend([
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            PathBuf::from("/usr/share/fonts/truetype"),
            PathBuf::from("/usr/share/fonts/opentype"),
            PathBuf::from("/usr/share/fonts/TTF"),
            PathBuf::from("/usr/share/fonts/noto"),
            PathBuf::from("/usr/share/fonts/noto-cjk"),
        ]);
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

/// File-name stems we will pick up during the directory scan if groups missed them.
fn extra_name_interesting(stem_lower: &str) -> bool {
    const HINTS: &[&str] = &[
        "arial unicode",
        "arialuni",
        "notosans-regular",
        "notosanscjk",
        "notosanscjksc",
        "notosanscjkjp",
        "notosanscjkkr",
        "notosanscjktc",
        "notosansarabic",
        "notonaskharabic",
        "notosanshebrew",
        "notosansthai",
        "notosansdevanagari",
        "notosansbengali",
        "notosanstamil",
        "notosanstelugu",
        "notosanssymbols",
        "sourcehansans",
        "sourcehanmono",
        "wqy-microhei",
        "wqy-zenhei",
        "droidsansfallback",
        "dejavusans",
        "freesans",
        "liberationsans-regular",
        "unifont",
        "symbola",
    ];
    HINTS.iter().any(|h| stem_lower.contains(h))
}

fn is_supported_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("ttf" | "otf" | "ttc")
    )
}

fn skip_face(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // LastResort is tofu by design; color-bitmap emoji often fails in egui.
    name.contains("lastresort")
        || name.contains("color emoji")
        || name.contains("apple color emoji")
        || name.contains("seguiemj")
        || name.contains("noto-coloremoji")
        || name.contains("notocoloremoji")
        || name.contains("emoji.ttf") && name.contains("apple")
}

fn read_font(path: &Path, index: u32, key: &str) -> Option<(String, FontData)> {
    if !path.is_file() || skip_face(path) || !is_supported_font_file(path) {
        return None;
    }
    match std::fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(key);
            let name = format!("{key}:{stem}:{index}");
            Some((
                name,
                FontData {
                    font: std::borrow::Cow::Owned(bytes),
                    index,
                    tweak: FontTweak {
                        scale: 0.92,
                        ..Default::default()
                    },
                },
            ))
        }
        Ok(_) => None,
        Err(err) => {
            tracing::debug!(path = %path.display(), error = %err, "skip font candidate");
            None
        }
    }
}

fn collect_scan_extras(already: &HashSet<PathBuf>, remaining: usize) -> Vec<(String, FontData)> {
    if remaining == 0 {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut seen_stems = HashSet::new();
    for dir in scan_dirs() {
        if found.len() >= remaining {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if found.len() >= remaining {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                // One extra level (e.g. /usr/share/fonts/truetype/noto).
                if let Ok(nested) = std::fs::read_dir(&path) {
                    for nested_entry in nested.flatten() {
                        if found.len() >= remaining {
                            break;
                        }
                        push_scan_hit(
                            nested_entry.path(),
                            already,
                            &mut seen_stems,
                            &mut found,
                            remaining,
                        );
                    }
                }
                continue;
            }
            push_scan_hit(path, already, &mut seen_stems, &mut found, remaining);
        }
    }
    found
}

fn push_scan_hit(
    path: PathBuf,
    already: &HashSet<PathBuf>,
    seen_stems: &mut HashSet<String>,
    found: &mut Vec<(String, FontData)>,
    remaining: usize,
) {
    if found.len() >= remaining || already.contains(&path) {
        return;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if stem.is_empty() || !extra_name_interesting(&stem) || !seen_stems.insert(stem.clone()) {
        return;
    }
    if let Some(loaded) = read_font(&path, 0, "scan") {
        tracing::info!(path = %path.display(), "loaded scanned fallback font");
        found.push(loaded);
    }
}

fn load_fallback_fonts() -> Vec<(String, FontData)> {
    let mut loaded = Vec::new();
    let mut used_paths = HashSet::new();

    for group in groups() {
        if loaded.len() >= MAX_FALLBACK_FONTS {
            break;
        }
        for &(path, index) in group.candidates {
            let p = PathBuf::from(path);
            if !used_paths.insert(p.clone()) {
                continue;
            }
            if let Some(font) = read_font(&p, index, group.key) {
                tracing::info!(group = group.key, path, index, "loaded fallback font");
                loaded.push(font);
                break;
            }
        }
    }

    let extras = collect_scan_extras(&used_paths, MAX_FALLBACK_FONTS.saturating_sub(loaded.len()));
    loaded.extend(extras);
    loaded
}

fn insert_as_fallback(ctx: &Context, name: String, data: FontData) {
    ctx.add_font(FontInsert::new(
        &name,
        data,
        vec![
            InsertFontFamily {
                family: FontFamily::Monospace,
                priority: FontPriority::Lowest,
            },
            InsertFontFamily {
                family: FontFamily::Proportional,
                priority: FontPriority::Lowest,
            },
        ],
    ));
}

/// Register system fallback fonts for both Monospace and Proportional families.
///
/// Safe to call once at startup; subsequent calls are no-ops.
pub fn install_fallback_fonts(ctx: &Context) {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    if INSTALLED.get().copied().unwrap_or(false) {
        return;
    }

    let fonts = load_fallback_fonts();
    if fonts.is_empty() {
        tracing::warn!(
            "no system fallback fonts found; non-Latin glyphs may render as boxes"
        );
    } else {
        tracing::info!(count = fonts.len(), "installed system fallback fonts");
        for (name, data) in fonts {
            insert_as_fallback(ctx, name, data);
        }
    }
    let _ = INSTALLED.set(true);
}

/// Back-compat alias used by older call sites.
pub fn install_cjk_fonts(ctx: &Context) {
    install_fallback_fonts(ctx);
}
