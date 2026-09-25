//! Drift contract for the design system reference kit (TSK-070). The kit ships
//! byte-identical under `resources/design-system/` in `cf-docs-portal` and
//! `cf-present`. `tokens.json` is the value source: `tokens.css` must carry its
//! values per role, the measured contrast and skin separation must reproduce
//! from it, the chrome and figure sheets must draw with tokens only, and the
//! reference pages must stand alone in the kit folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use regex::Regex;
use sha2::{Digest, Sha256};

const KIT_FILES: [&str; 8] = [
    "README.md",
    "tokens.json",
    "tokens.css",
    "chrome.css",
    "figure.css",
    "chrome.js",
    "portal.reference.html",
    "present.reference.html",
];
const SKILLS: [&str; 2] = ["cf-docs-portal", "cf-present"];
const SKILL_ROOTS: [&str; 5] = [
    "assets/base/agents/skills",
    ".claude/skills",
    ".agents/skills",
    ".codeflow/.baseline/.claude/skills",
    ".codeflow/.baseline/.agents/skills",
];
const PAIRS: [&str; 6] = [
    "graphite-light",
    "graphite-dark",
    "slate-light",
    "slate-dark",
    "sage-light",
    "sage-dark",
];

/// The contrast columns of `design-r2/measured.md`, foreground then ground.
const CONTRAST_COLUMNS: [(&str, &str); 10] = [
    ("text", "canvas"),
    ("text", "surface"),
    ("text-muted", "canvas"),
    ("text-muted", "surface"),
    ("accent", "surface"),
    ("accent-strong", "surface"),
    ("positive", "surface"),
    ("warning", "surface"),
    ("danger", "surface"),
    ("diagram-line", "surface"),
];
/// The measured WCAG contrast table the design primary approved.
const MEASURED_CONTRAST: [(&str, [f64; 10]); 6] = [
    (
        "graphite-light",
        [
            15.94, 17.22, 6.00, 6.48, 5.47, 7.72, 4.94, 5.52, 5.76, 12.45,
        ],
    ),
    (
        "graphite-dark",
        [
            14.45, 13.18, 6.99, 6.38, 8.10, 9.92, 7.36, 7.29, 6.58, 10.60,
        ],
    ),
    (
        "slate-light",
        [
            14.13, 15.74, 5.50, 6.13, 4.91, 6.82, 4.72, 5.36, 5.69, 10.52,
        ],
    ),
    (
        "slate-dark",
        [
            15.01, 13.66, 7.45, 6.78, 7.92, 9.98, 7.67, 8.47, 7.04, 11.06,
        ],
    ),
    (
        "sage-light",
        [
            14.88, 15.74, 5.69, 6.02, 5.80, 8.12, 4.91, 5.31, 5.55, 10.62,
        ],
    ),
    (
        "sage-dark",
        [
            14.45, 13.25, 7.33, 6.72, 8.53, 10.30, 8.62, 7.41, 6.70, 11.24,
        ],
    ),
];
/// The measured CIEDE2000 separation between skins in the same mode:
/// (mode, skin, skin, canvas dE, surface dE).
const MEASURED_SEPARATION: [(&str, &str, &str, f64, f64); 6] = [
    ("light", "graphite", "slate", 3.27, 1.62),
    ("light", "graphite", "sage", 3.43, 1.97),
    ("light", "slate", "sage", 5.74, 2.58),
    ("dark", "graphite", "slate", 4.73, 5.62),
    ("dark", "graphite", "sage", 4.43, 4.81),
    ("dark", "slate", "sage", 6.88, 7.86),
];
/// Two-decimal table values reproduce within half a unit of the last place.
const TABLE_TOLERANCE: f64 = 0.005_1;

const NAMED_COLOURS: &str = "aliceblue antiquewhite aqua aquamarine azure beige bisque black \
    blanchedalmond blue blueviolet brown burlywood cadetblue chartreuse chocolate coral \
    cornflowerblue cornsilk crimson cyan darkblue darkcyan darkgoldenrod darkgray darkgreen \
    darkgrey darkkhaki darkmagenta darkolivegreen darkorange darkorchid darkred darksalmon \
    darkseagreen darkslateblue darkslategray darkslategrey darkturquoise darkviolet deeppink \
    deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro \
    ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink indianred indigo \
    ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral lightcyan \
    lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen \
    lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen \
    magenta maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen \
    mediumslateblue mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream \
    mistyrose moccasin navajowhite navy oldlace olive olivedrab orange orangered orchid \
    palegoldenrod palegreen paleturquoise palevioletred papayawhip peachpuff peru pink plum \
    powderblue purple rebeccapurple red rosybrown royalblue saddlebrown salmon sandybrown \
    seagreen seashell sienna silver skyblue slateblue slategray slategrey snow springgreen \
    steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke yellow \
    yellowgreen accentcolor accentcolortext activetext buttonborder buttonface buttontext \
    canvas canvastext field fieldtext graytext highlight highlighttext linktext mark marktext \
    selecteditem selecteditemtext visitedtext";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kit_dir(root: &Path, skill_root: &str, skill: &str) -> PathBuf {
    root.join(skill_root)
        .join(skill)
        .join("resources/design-system")
}

/// The canonical kit: the `assets/base` source of the `cf-present` copy. The
/// copies test proves every other copy equals it byte for byte.
fn read_kit(file: &str) -> String {
    let path = kit_dir(&repo_root(), SKILL_ROOTS[0], "cf-present").join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
}

fn tokens() -> BTreeMap<String, BTreeMap<String, String>> {
    serde_json::from_str(&read_kit("tokens.json")).expect("tokens.json is pair -> role -> hex")
}

fn rgb(hex: &str) -> [f64; 3] {
    assert!(
        hex.len() == 7 && hex.starts_with('#'),
        "token value {hex} is not #rrggbb"
    );
    let channel = |at: usize| {
        f64::from(u8::from_str_radix(&hex[at..at + 2], 16).expect("hex channel parses"))
    };
    [channel(1), channel(3), channel(5)]
}

fn linear(channel: f64) -> f64 {
    let c = channel / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(hex: &str) -> f64 {
    let [r, g, b] = rgb(hex).map(linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn lab(hex: &str) -> [f64; 3] {
    let [red, green, blue] = rgb(hex).map(linear);
    let pivot = |value: f64| {
        if value > 0.008_856 {
            value.cbrt()
        } else {
            7.787 * value + 16.0 / 116.0
        }
    };
    let fx = pivot((red * 0.412_456_4 + green * 0.357_576_1 + blue * 0.180_437_5) / 0.950_47);
    let fy = pivot(red * 0.212_672_9 + green * 0.715_152_2 + blue * 0.072_175_0);
    let fz = pivot((red * 0.019_333_9 + green * 0.119_192_0 + blue * 0.950_304_1) / 1.088_83);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn hue_degrees(a: f64, b: f64) -> f64 {
    if a == 0.0 && b == 0.0 {
        return 0.0;
    }
    let hue = b.atan2(a).to_degrees();
    if hue < 0.0 {
        hue + 360.0
    } else {
        hue
    }
}

/// CIEDE2000, the same formula as the design round's `tokens_check.py`.
fn delta_e_2000(first: &str, second: &str) -> f64 {
    let [l1, a1, b1] = lab(first);
    let [l2, a2, b2] = lab(second);
    let seventh = |c: f64| (c.powi(7) / (c.powi(7) + 25f64.powi(7))).sqrt();
    let chroma_mean = f64::midpoint(a1.hypot(b1), a2.hypot(b2));
    let g = 0.5 * (1.0 - seventh(chroma_mean));
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let (h1p, h2p) = (hue_degrees(a1p, b1), hue_degrees(a2p, b2));
    let chroma_zero = c1p * c2p == 0.0;
    let mut hue_diff = if chroma_zero { 0.0 } else { h2p - h1p };
    if hue_diff > 180.0 {
        hue_diff -= 360.0;
    } else if hue_diff < -180.0 {
        hue_diff += 360.0;
    }
    let hue_step = 2.0 * (c1p * c2p).sqrt() * (hue_diff / 2.0).to_radians().sin();
    let (l_mean, prime_chroma_mean) = (f64::midpoint(l1, l2), f64::midpoint(c1p, c2p));
    let mut hp_mean = if chroma_zero {
        h1p + h2p
    } else {
        f64::midpoint(h1p, h2p)
    };
    if !chroma_zero && (h1p - h2p).abs() > 180.0 {
        hp_mean += if hp_mean < 180.0 { 180.0 } else { -180.0 };
    }
    let cos = |degrees: f64| degrees.to_radians().cos();
    let t = 1.0 - 0.17 * cos(hp_mean - 30.0)
        + 0.24 * cos(2.0 * hp_mean)
        + 0.32 * cos(3.0 * hp_mean + 6.0)
        - 0.20 * cos(4.0 * hp_mean - 63.0);
    let d_theta = 30.0 * (-((hp_mean - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * seventh(prime_chroma_mean);
    let sl = 1.0 + 0.015 * (l_mean - 50.0).powi(2) / (20.0 + (l_mean - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * prime_chroma_mean;
    let sh = 1.0 + 0.015 * prime_chroma_mean * t;
    let rt = -(2.0 * d_theta).to_radians().sin() * rc;
    let (light_term, chroma_term, hue_term) = ((l2 - l1) / sl, (c2p - c1p) / sc, hue_step / sh);
    (light_term * light_term
        + chroma_term * chroma_term
        + hue_term * hue_term
        + rt * chroma_term * hue_term)
        .sqrt()
}

/// Every declaration value in a stylesheet, as (property, value). Only text
/// that closes with `}` is a declaration block, so selectors never count.
fn declarations(css: &str) -> Vec<(String, String)> {
    let comment = Regex::new(r"(?s)/\*.*?\*/").expect("comment pattern");
    let stripped = comment.replace_all(css, "");
    let mut out = Vec::new();
    let mut segment = String::new();
    for character in stripped.chars() {
        match character {
            '{' => segment.clear(),
            '}' => {
                for declaration in segment.split(';') {
                    if let Some((property, value)) = declaration.split_once(':') {
                        out.push((property.trim().to_owned(), value.trim().to_owned()));
                    }
                }
                segment.clear();
            }
            _ => segment.push(character),
        }
    }
    out
}

/// The literal colours in one declaration value: hex, colour functions and
/// CSS named or system colours. `transparent`, `currentColor` and `inherit`
/// are not colours of the palette and stay allowed.
fn literal_colours(value: &str) -> Vec<String> {
    let lower = value.to_ascii_lowercase();
    let hex = Regex::new(r"#[0-9a-f]{3,8}\b").expect("hex pattern");
    let function =
        Regex::new(r"\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(").expect("function pattern");
    let var = Regex::new(r"var\([^)]*\)").expect("var pattern");
    let word = Regex::new(r"[a-z0-9_-]+").expect("word pattern");
    let named: BTreeSet<&str> = NAMED_COLOURS.split_whitespace().collect();
    let mut hits: Vec<String> = hex
        .find_iter(&lower)
        .chain(function.find_iter(&lower))
        .map(|found| found.as_str().to_owned())
        .collect();
    let without_vars = var.replace_all(&lower, "");
    for found in word.find_iter(&without_vars) {
        let is_call = without_vars[found.end()..].starts_with('(');
        if !is_call && named.contains(found.as_str()) {
            hits.push(found.as_str().to_owned());
        }
    }
    hits
}

/// `tokens.css` carries exactly the six pair blocks of `tokens.json`, and in
/// each block every role carries the JSON value. A colour role the JSON does
/// not name is drift too.
#[test]
fn tokens_css_carries_the_json_value_of_every_role_in_every_pair() {
    let tokens = tokens();
    let css = read_kit("tokens.css");
    let block = Regex::new(r#"(?s)\[data-cf-skin="(\w+)"\]\[data-cf-theme="(\w+)"\] \{(.*?)\}"#)
        .expect("block pattern");
    let declaration = Regex::new(r"--cf-([a-z-]+):\s*([^;]+);").expect("declaration pattern");
    let mut blocks = BTreeMap::new();
    for found in block.captures_iter(&css) {
        let pair = format!("{}-{}", &found[1], &found[2]);
        assert!(
            blocks.insert(pair.clone(), found[3].to_owned()).is_none(),
            "tokens.css declares {pair} twice"
        );
    }
    let expected: BTreeSet<&str> = PAIRS.into_iter().collect();
    assert_eq!(
        tokens.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected,
        "tokens.json must carry exactly the six skin and mode pairs"
    );
    assert_eq!(
        blocks.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected,
        "tokens.css must carry exactly the six pair blocks"
    );
    let first_roles: BTreeSet<&String> = tokens[PAIRS[0]].keys().collect();
    let mut compared = 0;
    for (pair, roles) in &tokens {
        assert_eq!(
            roles.keys().collect::<BTreeSet<_>>(),
            first_roles,
            "{pair} names a different role set from {}",
            PAIRS[0]
        );
        let declared: BTreeMap<String, String> = declaration
            .captures_iter(&blocks[pair])
            .map(|found| (found[1].to_owned(), found[2].trim().to_owned()))
            .collect();
        for (role, value) in roles {
            assert_eq!(
                declared.get(role),
                Some(value),
                "{pair} --cf-{role} drifted from tokens.json"
            );
            compared += 1;
        }
        for (role, value) in &declared {
            assert!(
                !value.starts_with('#') || roles.contains_key(role),
                "{pair} --cf-{role}: {value} is a colour role tokens.json does not name"
            );
        }
    }
    assert!(compared >= 6 * 20, "only {compared} role values compared");
}

/// The approved contrast table reproduces from `tokens.json`, and every
/// measured pairing stays at or above 4.5:1.
#[test]
fn token_contrast_reproduces_the_measured_table() {
    let tokens = tokens();
    for (pair, expected) in MEASURED_CONTRAST {
        let roles = &tokens[pair];
        for ((foreground, ground), want) in CONTRAST_COLUMNS.into_iter().zip(expected) {
            let got = contrast(&roles[foreground], &roles[ground]);
            assert!(
                (got - want).abs() <= TABLE_TOLERANCE,
                "{pair} {foreground}/{ground} contrast {got:.3}, measured {want:.2}"
            );
            assert!(
                got >= 4.5,
                "{pair} {foreground}/{ground} contrast {got:.2} is under 4.5:1"
            );
        }
    }
}

/// Skins stay distinguishable in each mode (canvases at least 3.0 apart and
/// surfaces at least 1.5 apart in CIEDE2000) and Graphite stays neutral.
#[test]
fn skins_stay_apart_in_each_mode_and_graphite_stays_neutral() {
    let tokens = tokens();
    for (mode, first, second, canvas_want, surface_want) in MEASURED_SEPARATION {
        let a = &tokens[&format!("{first}-{mode}")];
        let b = &tokens[&format!("{second}-{mode}")];
        let canvas = delta_e_2000(&a["canvas"], &b["canvas"]);
        let surface = delta_e_2000(&a["surface"], &b["surface"]);
        assert!(
            (canvas - canvas_want).abs() <= TABLE_TOLERANCE
                && (surface - surface_want).abs() <= TABLE_TOLERANCE,
            "{mode} {first} vs {second}: canvas dE {canvas:.3} (measured {canvas_want}), \
             surface dE {surface:.3} (measured {surface_want})"
        );
        assert!(
            canvas >= 3.0 && surface >= 1.5,
            "{mode} {first} vs {second} are too close: canvas {canvas:.2}, surface {surface:.2}"
        );
    }
    for mode in ["light", "dark"] {
        let canvas = &tokens[&format!("graphite-{mode}")]["canvas"];
        let [r, g, b] = rgb(canvas);
        assert!(
            (r - g).abs() < f64::EPSILON && (g - b).abs() < f64::EPSILON,
            "graphite {mode} canvas {canvas} is not a neutral grey"
        );
    }
}

/// The chrome and figure sheets draw with tokens only. A literal colour in a
/// declaration value (hex, a colour function or a named colour) would fork
/// the palette away from `tokens.json`.
#[test]
fn chrome_and_figure_sheets_carry_no_literal_colour() {
    for sheet in ["chrome.css", "figure.css"] {
        let css = read_kit(sheet);
        let declared = declarations(&css);
        assert!(
            declared.len() > 20,
            "{sheet} parsed to only {} declarations",
            declared.len()
        );
        let hits: Vec<String> = declared
            .iter()
            .flat_map(|(property, value)| {
                literal_colours(value)
                    .into_iter()
                    .map(move |colour| format!("{property}: {value} ({colour})"))
            })
            .collect();
        assert!(
            hits.is_empty(),
            "{sheet} carries literal colours:\n  {}",
            hits.join("\n  ")
        );
    }
    let probe = "a{color:#fff;background:rgb(0 0 0);border-color:red;fill:currentColor}";
    let found: Vec<String> = declarations(probe)
        .iter()
        .flat_map(|(_, value)| literal_colours(value))
        .collect();
    assert_eq!(
        found,
        ["#fff", "rgb(", "red"],
        "the literal colour probe must catch hex, functions and names"
    );
}

/// The kit is one set of eight files, byte-identical in both skills, their
/// harness mirrors and the update baselines.
#[test]
fn kit_copies_are_byte_identical_across_skills_and_mirrors() {
    let root = repo_root();
    let expected: BTreeSet<String> = KIT_FILES.iter().map(ToString::to_string).collect();
    let canonical = kit_dir(&root, SKILL_ROOTS[0], "cf-present");
    let mut problems = Vec::new();
    for skill_root in SKILL_ROOTS {
        for skill in SKILLS {
            let dir = kit_dir(&root, skill_root, skill);
            let present: BTreeSet<String> = std::fs::read_dir(&dir)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", dir.display()))
                .map(|entry| {
                    entry
                        .expect("kit entry is readable")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            if present != expected {
                problems.push(format!("{}: file set {present:?}", dir.display()));
                continue;
            }
            for file in KIT_FILES {
                let copy = std::fs::read(dir.join(file)).expect("kit copy is readable");
                let source = std::fs::read(canonical.join(file)).expect("kit source is readable");
                if copy != source {
                    problems.push(format!("{}: byte drift at {file}", dir.display()));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "design kit copies drifted:\n  {}",
        problems.join("\n  ")
    );
}

/// One figure runtime (TSK-059): the portal, its managed starter and present
/// carry the kit's figure sheet byte for byte, and present and the starter
/// carry the portal's grammar module byte for byte, since present's esbuild
/// entry reaches only `web/src`.
#[test]
fn figure_runtime_copies_match_the_kit_sheet_and_the_portal_module() {
    let root = repo_root();
    let sheet = read_kit("figure.css");
    let module = std::fs::read_to_string(root.join("docs-portal/scripts/figure-grammar.mjs"))
        .expect("the portal grammar module is readable");
    let mut problems = Vec::new();
    for (copy, source, label) in [
        (
            "docs-portal/src/styles/figure.css",
            &sheet,
            "the kit figure.css",
        ),
        (
            "assets/docs-portal/starter/src/styles/figure.css",
            &sheet,
            "the kit figure.css",
        ),
        (
            "crates/codeflow-present/web/src/figure.css",
            &sheet,
            "the kit figure.css",
        ),
        (
            "crates/codeflow-present/web/src/figure-grammar.mjs",
            &module,
            "docs-portal/scripts/figure-grammar.mjs",
        ),
        (
            "assets/docs-portal/starter/scripts/figure-grammar.mjs",
            &module,
            "docs-portal/scripts/figure-grammar.mjs",
        ),
    ] {
        match std::fs::read_to_string(root.join(copy)) {
            Ok(bytes) if &bytes == source => {}
            Ok(_) => problems.push(format!("{copy} differs from {label}")),
            Err(error) => problems.push(format!("{copy} is unreadable: {error}")),
        }
    }
    assert!(
        problems.is_empty(),
        "figure runtime copies drifted:\n  {}",
        problems.join("\n  ")
    );
}

/// Once the figure block renders (TSK-059), the skills teach it as the default
/// carrier and keep `cf-stage` only as the flow interim. Pinned in every skill
/// root so a mirror cannot keep the old default.
#[test]
fn skills_teach_the_figure_block_as_the_default_carrier() {
    let root = repo_root();
    let pins: [(&str, &[&str]); 6] = [
        (
            "cf-docs-portal/SKILL.md",
            &["lead every altitude panel", "drawn by the figure block"],
        ),
        (
            "cf-docs-portal/references/visual-craft.md",
            &[
                "a figure in every panel",
                "### The figure block (the default carrier)",
                "### `cf-stage`: the flow interim",
            ],
        ),
        (
            "cf-docs-portal/resources/portal-page-shape.example.md",
            &[
                "\"panel\": \"concept\"",
                "\"panel\": \"architecture\"",
                "\"panel\": \"technical\"",
                "\"family\": \"structure\"",
                "\"family\": \"derivation\"",
                "\"family\": \"extent\"",
            ],
        ),
        (
            "cf-present/references/document-authoring.md",
            &[
                "| a relationship the reader must see | `figure` |",
                "### Figure",
            ],
        ),
        (
            "cf-present/resources/how-presentation-works.md",
            &["**author a `figure` block**", "flow family's interim form"],
        ),
        (
            "cf-present/resources/utility-presentation-system.md",
            &[
                "### Page classes in configuration (portal)",
                "`figure` block carrying the declaration",
            ],
        ),
    ];
    let mut problems = Vec::new();
    for skill_root in SKILL_ROOTS {
        for (file, required) in &pins {
            let path = root.join(skill_root).join(file);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
            for marker in *required {
                if !text.contains(marker) {
                    problems.push(format!("{}: missing {marker}", path.display()));
                }
            }
        }
        for skill in SKILLS {
            let dir = root.join(skill_root).join(skill);
            for entry in walk(&dir) {
                let text = std::fs::read_to_string(&entry).unwrap_or_default();
                for retired in ["Prefer a `cf-stage` fence", "Not yet a rendered carrier"] {
                    if text.contains(retired) {
                        problems.push(format!("{}: still says {retired}", entry.display()));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the figure block is not the taught default:\n  {}",
        problems.join("\n  ")
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", dir.display()))
    {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
    files
}

/// Every kit file ships to both harnesses from its own skill source, and the
/// repository manifest records the hash of the bytes it installed.
#[test]
fn kit_is_manifested_for_both_harnesses_with_matching_hashes() {
    let root = repo_root();
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(".codeflow/manifest.json")).expect("manifest is readable"),
    )
    .expect("manifest is JSON");
    let scaffold = std::fs::read_to_string(root.join("assets/base/scaffold-manifest.toml"))
        .expect("scaffold manifest is readable");
    for harness in [".claude", ".agents"] {
        for skill in SKILLS {
            for file in KIT_FILES {
                let src = format!("agents/skills/{skill}/resources/design-system/{file}");
                let dest = format!("{harness}/skills/{skill}/resources/design-system/{file}");
                assert!(
                    scaffold.contains(&format!("src = \"{src}\"\ndest = \"{dest}\"\n")),
                    "scaffold-manifest.toml lacks the entry for {dest}"
                );
                let entry = &manifest["files"][&dest];
                assert_eq!(entry["src"], src.as_str(), "{dest} manifest source");
                let digest = Sha256::digest(
                    std::fs::read(root.join("assets/base").join(&src)).expect("kit source"),
                );
                let hex = digest.iter().fold(String::new(), |mut out, byte| {
                    write!(out, "{byte:02x}").expect("writing to a String cannot fail");
                    out
                });
                assert_eq!(entry["sha256"], hex.as_str(), "{dest} manifest hash");
            }
        }
    }
}

/// Each reference page loads the three sheets and the script from its own
/// folder and nothing else; no kit file names a remote URL.
#[test]
fn reference_pages_stand_alone_in_the_kit_folder() {
    let attribute = Regex::new(r#"\b(?:href|src|srcset|xlink:href)\s*=\s*["']([^"']*)["']"#)
        .expect("attribute pattern");
    let remote = Regex::new(r"(?i)(?:https?:)?//[a-z0-9.-]+\.[a-z]{2,}").expect("remote pattern");
    let kit_assets: BTreeSet<&str> = ["tokens.css", "chrome.css", "figure.css", "chrome.js"]
        .into_iter()
        .collect();
    for page in ["portal.reference.html", "present.reference.html"] {
        let html = read_kit(page);
        let references: Vec<String> = attribute
            .captures_iter(&html)
            .map(|found| found[1].to_owned())
            .filter(|reference| !reference.starts_with('#'))
            .collect();
        let outside: Vec<&String> = references
            .iter()
            .filter(|reference| !kit_assets.contains(reference.as_str()))
            .collect();
        assert!(
            outside.is_empty(),
            "{page} references files outside the kit: {outside:?}"
        );
        let loaded: BTreeSet<&str> = references.iter().map(String::as_str).collect();
        assert_eq!(
            loaded, kit_assets,
            "{page} must load exactly the three sheets and chrome.js"
        );
    }
    for file in KIT_FILES {
        let text = read_kit(file);
        let is_sheet = Path::new(file)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("css"));
        let urls: Vec<&str> = remote
            .find_iter(&text)
            .filter(|found| {
                let namespace = &text[found.end()..];
                !(found.as_str().ends_with("//www.w3.org")
                    && (namespace.starts_with("/2000/svg") || namespace.starts_with("/1999/xlink")))
            })
            .map(|found| found.as_str())
            .collect();
        assert!(urls.is_empty(), "{file} names remote URLs: {urls:?}");
        for sheet in ["@import", "url("] {
            assert!(
                !is_sheet || !text.contains(sheet),
                "{file} loads a resource through {sheet}"
            );
        }
    }
}

/// The written content policy forbids en and em dashes in every kit file.
#[test]
fn kit_files_carry_no_en_or_em_dash() {
    for file in KIT_FILES {
        let text = read_kit(file);
        let lines: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(['\u{2013}', '\u{2014}']))
            .map(|(index, _)| index + 1)
            .collect();
        assert!(
            lines.is_empty(),
            "{file} has en or em dashes on lines {lines:?}"
        );
    }
}

/// Product CSS carries every kit role, rather than a second palette table.
#[test]
fn product_sheets_match_every_kit_role_and_measured_floor() {
    let root = repo_root();
    let expected = tokens();
    let pair = Regex::new(r#"(?s)\[data-(?:cfp-skin|cf-theme)="(graphite|slate|sage)"\]\[data-(?:theme|cf-mode-resolved)="(light|dark)"\] \{(.*?)\}"#).unwrap();
    let property = Regex::new(r"--cf-([a-z-]+):\s*([^;]+);").unwrap();
    for path in [
        "docs-portal/src/styles/utility-tokens.css",
        "assets/docs-portal/starter/src/styles/utility-tokens.css",
        "crates/codeflow-present/web/src/styles.css",
    ] {
        let css = std::fs::read_to_string(root.join(path)).unwrap();
        for retired in ["instrument", "editorial", "ink", "technical"] {
            assert!(
                !css.contains(&format!("=\"{retired}\"")),
                "{path} keeps a retired skin"
            );
        }
        let mut actual = BTreeMap::new();
        for capture in pair.captures_iter(&css) {
            let key = format!("{}-{}", &capture[1], &capture[2]);
            let roles: BTreeMap<String, String> = property
                .captures_iter(&capture[3])
                .map(|value| (value[1].to_owned(), value[2].trim().to_owned()))
                .collect();
            assert!(
                actual.insert(key, roles).is_none(),
                "{path} has duplicate pair blocks"
            );
        }
        assert_eq!(actual.len(), 6, "{path} must carry six pairs");
        for (key, roles) in &expected {
            for (role, value) in roles {
                assert_eq!(actual[key].get(role), Some(value), "{path} {key} {role}");
            }
            for foreground in ["text", "text-muted"] {
                for ground in ["canvas", "surface"] {
                    let ratio = contrast(&actual[key][foreground], &actual[key][ground]);
                    println!("{path} | {key} | {foreground}/{ground} | {ratio:.2}:1");
                    assert!(ratio >= 4.5);
                }
            }
        }
        for mode in ["light", "dark"] {
            let [red, green, blue] = rgb(&actual[&format!("graphite-{mode}")]["canvas"]);
            assert!((red - green).abs() < f64::EPSILON && (green - blue).abs() < f64::EPSILON);
            for (first, second) in [
                ("graphite", "slate"),
                ("graphite", "sage"),
                ("slate", "sage"),
            ] {
                let a = &actual[&format!("{first}-{mode}")];
                let b = &actual[&format!("{second}-{mode}")];
                let canvas = delta_e_2000(&a["canvas"], &b["canvas"]);
                let surface = delta_e_2000(&a["surface"], &b["surface"]);
                println!("{path} | {mode} | {first}/{second} | canvas {canvas:.2} | surface {surface:.2}");
                assert!(canvas >= 3.0 && surface >= 1.5);
            }
        }
    }
}
