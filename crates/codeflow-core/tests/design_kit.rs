//! Drift contract for the design system reference kit (TSK-070). The kit ships
//! byte-identical under `resources/design-system/` in `cf-docs-portal` and
//! `cf-present`. `source/*.json` is the authored value source and `tokens.json` its resolved output: `tokens.css` must carry its
//! values per role, the measured contrast and skin separation must reproduce
//! from it, the chrome and figure sheets must draw with tokens only, and the
//! reference pages must stand alone in the kit folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use regex::Regex;
use sha2::{Digest, Sha256};

const KIT_FILES: [&str; 19] = [
    "counts.reference.html",
    "count-charts.js",
    "tokens.ts",
    "tokens.js",
    "settings.js",
    "stage-geometry.js",
    "present.css",
    "export-fallback.css",
    "portal.css",
    "utility-tokens.css",
    "figure-roles.css",
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
/// The measured CIEDE2000 separation between skins in the same mode:
/// (mode, skin, skin, canvas dE, surface dE).
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

// Compile the pure private maths directly to avoid exposing a public API or
// adding the present server dependency graph solely for this palette test.
#[path = "../../codeflow-present/src/colour.rs"]
mod colour;
use colour::{contrast, delta_e_2000, rgb};

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
    let declaration = Regex::new(r"--cf-([a-z0-9-]+):\s*([^;]+);").expect("declaration pattern");
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
    for pair in PAIRS {
        let roles = &tokens[pair];
        for (foreground, ground) in CONTRAST_COLUMNS {
            let got = contrast(&roles[foreground], &roles[ground]);
            println!("{pair} {foreground}/{ground} contrast {got:.3}");
            assert!(
                got >= colour::floor("textContrast"),
                "{pair} {foreground}/{ground} contrast {got:.3}"
            );
        }
    }
}

/// Skins stay distinguishable in each mode (canvases at least 3.0 apart and
/// surfaces at least 1.5 apart in CIEDE2000) and Graphite stays neutral.
#[test]
fn skins_stay_apart_in_each_mode_and_graphite_stays_neutral() {
    let tokens = tokens();
    for (mode, first, second) in ["light", "dark"].into_iter().flat_map(|mode| {
        [
            (mode, "graphite", "slate"),
            (mode, "graphite", "sage"),
            (mode, "slate", "sage"),
        ]
    }) {
        let a = &tokens[&format!("{first}-{mode}")];
        let b = &tokens[&format!("{second}-{mode}")];
        let canvas = delta_e_2000(&a["canvas"], &b["canvas"]);
        let surface = delta_e_2000(&a["surface"], &b["surface"]);
        assert!(
            canvas >= colour::floor("skinCanvasSeparation")
                && surface >= colour::floor("skinSurfaceSeparation"),
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

/// The kit has generated files and a source directory, byte-identical in both skills, their
/// harness mirrors and the update baselines.
#[test]
fn kit_copies_are_byte_identical_across_skills_and_mirrors() {
    let root = repo_root();
    let expected: BTreeSet<String> = KIT_FILES
        .iter()
        .copied()
        .chain(["source"])
        .map(ToString::to_string)
        .collect();
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
    let kit_assets: BTreeSet<&str> = [
        "tokens.css",
        "chrome.css",
        "figure.css",
        "chrome.js",
        "settings.js",
    ]
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
            "{page} must load exactly the three sheets, settings.js and chrome.js"
        );
    }
    let css_url = Regex::new(r#"url\(\s*["']?([^"')]+)"#).unwrap();
    for file in KIT_FILES {
        let text = read_kit(file);
        // Only these sheets are loaded by the standalone kit pages.
        // Product adapters use their products' bundled font resources.
        let is_sheet = matches!(file, "tokens.css" | "chrome.css" | "figure.css");
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
        if is_sheet {
            assert!(!text.contains("@import"), "{file} imports another sheet");
            for value in css_url.captures_iter(&text) {
                assert!(
                    value[1].starts_with("data:"),
                    "{file} loads an external resource: {}",
                    &value[1]
                );
            }
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
    let property = Regex::new(r"--cf-([a-z0-9-]+):\s*([^;]+);").unwrap();
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
            if let Some(previous) = actual.insert(key, roles.clone()) {
                assert_eq!(previous, roles, "{path} adapter selectors disagree");
            }
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
                    assert!(ratio >= colour::floor("textContrast"));
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
                assert!(
                    canvas >= colour::floor("skinCanvasSeparation")
                        && surface >= colour::floor("skinSurfaceSeparation")
                );
            }
        }
    }
}

/// In both kits (cf-present and cf-docs-portal), every companion ink carries
/// text at WCAG AA (4.5:1) on the surface, the
/// canvas, the figure fill and its own tint, and the figure ink keeps 4.5:1
/// on each tint a toned region can carry, in all six skin and mode pairs.
#[test]
fn companion_inks_and_tints_meet_the_contrast_floor_in_every_pair() {
    let root = repo_root();
    let mut table = Vec::new();
    for (skill, pair) in SKILLS
        .iter()
        .flat_map(|skill| PAIRS.iter().map(move |pair| (*skill, *pair)))
    {
        let path = kit_dir(&root, SKILL_ROOTS[0], skill).join("tokens.json");
        let tokens: BTreeMap<String, BTreeMap<String, String>> =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("kit tokens are readable"))
                .expect("tokens.json is pair -> role -> hex");
        let roles = &tokens[pair];
        let source: serde_json::Value =
            serde_json::from_str(&read_kit("source/colour.json")).unwrap();
        for role in ["fig-cat1", "fig-cat1-soft", "fig-cat2", "fig-cat2-soft"] {
            assert_eq!(
                roles[role],
                source["palettes"][pair][role].as_str().unwrap(),
                "{skill} {pair} {role} differs from the single token source"
            );
        }
        let mut checks: Vec<(String, f64)> = Vec::new();
        for ink in ["fig-cat1", "fig-cat2"] {
            let tint = format!("{ink}-soft");
            for ground in ["surface", "canvas", "fig-fill", tint.as_str()] {
                checks.push((
                    format!("{ink} on {ground}"),
                    contrast(&roles[ink], &roles[ground]),
                ));
            }
        }
        for tint in ["fig-cat1-soft", "fig-cat2-soft", "fig-accent-soft"] {
            checks.push((
                format!("fig-line on {tint}"),
                contrast(&roles["fig-line"], &roles[tint]),
            ));
        }
        for (name, ratio) in checks {
            table.push(format!("{skill} | {pair} | {name} | {ratio:.2}:1"));
            assert!(
                ratio >= colour::floor("textContrast"),
                "{skill} {pair} {name} is {ratio:.2}:1, under the 4.5:1 floor"
            );
        }
    }
    println!("{}", table.join("\n"));
}

/// The companions read as hues of their own: apart from each other and from
/// the accent, and clear of the reserved warn and stop hues, in CIEDE2000.
#[test]
fn companion_inks_stay_apart_from_the_accent_and_the_reserved_hues() {
    let tokens = tokens();
    for pair in PAIRS {
        let roles = &tokens[pair];
        for (first, second, floor) in [
            ("fig-cat1", "fig-cat2", colour::floor("companionSeparation")),
            (
                "fig-cat1",
                "fig-accent",
                colour::floor("companionSeparation"),
            ),
            (
                "fig-cat2",
                "fig-accent",
                colour::floor("companionSeparation"),
            ),
            ("fig-cat1", "fig-warn", colour::floor("companionSeparation")),
            ("fig-cat2", "fig-warn", colour::floor("companionSeparation")),
            ("fig-cat1", "fig-stop", colour::floor("companionSeparation")),
            (
                "fig-cat2",
                "fig-stop",
                colour::floor("companionStopSeparation"),
            ),
        ] {
            let distance = delta_e_2000(&roles[first], &roles[second]);
            println!("{pair} | {first}/{second} | dE {distance:.2}");
            assert!(
                distance >= floor,
                "{pair} {first} and {second} are only dE {distance:.2} apart (floor {floor})"
            );
        }
    }
}

/// The S2 type scale (TSK-444): the value each size role takes at the
/// default size, and the floor it keeps at any size step.
const TYPE_SCALE: [(&str, f64, Option<f64>); 8] = [
    ("display", 34.0, None),
    ("lede", 19.0, None),
    ("h2", 24.0, None),
    ("h3", 18.0, None),
    ("title", 15.0, Some(15.0)),
    ("body", 15.0, Some(15.0)),
    ("caption", 13.0, Some(13.0)),
    ("micro", 12.5, Some(12.5)),
];
const SIZE_STEPS: [(&str, f64); 3] = [("compact", 0.94), ("default", 1.0), ("large", 1.12)];

/// The computed px of a `--cf-fs-*` value: `calc(Npx * var(--cf-ui-scale))`,
/// optionally inside `max(Fpx, ...)`.
fn font_size(value: &str, scale: f64) -> f64 {
    let pattern =
        Regex::new(r"^(?:max\(([\d.]+)px, )?calc\(([\d.]+)px \* var\(--cf-ui-scale\)\)\)?$")
            .expect("size pattern compiles");
    let captures = pattern
        .captures(value.trim())
        .unwrap_or_else(|| panic!("unexpected size value {value}"));
    let scaled = captures[2].parse::<f64>().expect("size is a number") * scale;
    captures.get(1).map_or(scaled, |floor| {
        scaled.max(floor.as_str().parse().expect("floor is a number"))
    })
}

fn size_roles(css: &str) -> BTreeMap<String, String> {
    let pattern = Regex::new(r"--cf-fs-([a-z0-9]+):\s*([^;]+);").expect("role pattern compiles");
    let mut roles = BTreeMap::new();
    for capture in pattern.captures_iter(css) {
        assert!(
            roles
                .insert(capture[1].to_owned(), capture[2].trim().to_owned())
                .is_none(),
            "--cf-fs-{} is declared twice",
            &capture[1]
        );
    }
    roles
}

/// One type scale in the kit and every product sheet: the S2 values at the
/// default size, the floors at every size step, and title > section >
/// subsection > block title >= body at compact, default and large.
#[test]
fn the_type_scale_keeps_its_order_and_floors_at_every_size_step() {
    let root = repo_root();
    for path in [
        "assets/base/agents/skills/cf-present/resources/design-system/tokens.css",
        "docs-portal/src/styles/utility-tokens.css",
        "assets/docs-portal/starter/src/styles/utility-tokens.css",
        "crates/codeflow-present/web/src/styles.css",
    ] {
        let css = std::fs::read_to_string(root.join(path)).expect("sheet is readable");
        let roles = size_roles(&css);
        for (role, at_default, floor) in TYPE_SCALE {
            let value = roles
                .get(role)
                .unwrap_or_else(|| panic!("{path} lacks --cf-fs-{role}"));
            assert!(
                (font_size(value, 1.0) - at_default).abs() < 1e-9,
                "{path} --cf-fs-{role} is {value}, S2 sets {at_default}px"
            );
            if let Some(floor) = floor {
                assert!(
                    (font_size(value, 0.5) - floor).abs() < 1e-9,
                    "{path} --cf-fs-{role} must keep its {floor}px floor"
                );
            }
        }
        for (step, scale) in SIZE_STEPS {
            let size = |role: &str| font_size(&roles[role], scale);
            let ladder = [
                ("display", size("display")),
                ("h2", size("h2")),
                ("h3", size("h3")),
                ("title", size("title")),
            ];
            println!("{path} | {step} | {ladder:?}");
            for window in ladder.windows(2) {
                assert!(
                    window[0].1 >= window[1].1 + 1.0,
                    "{path} at {step}: {} ({:.2}px) must exceed {} ({:.2}px) by 1px or more",
                    window[0].0,
                    window[0].1,
                    window[1].0,
                    window[1].1
                );
            }
            assert!(
                size("title") >= size("body") && size("lede") > size("body"),
                "{path} at {step}: a block title and the lede never fall below body text"
            );
        }
    }
}

/// The present runtime and the portal draw their headings from the scale
/// roles, so the order the tokens hold is the order the page shows: the
/// document title on display, a section on h2, a subsection on h3 and every
/// block title (comparison column, decision, form, callout, tree, figure and
/// frame title) on the block title role.
#[test]
fn present_and_portal_headings_take_the_scale_roles() {
    let root = repo_root();
    let present = std::fs::read_to_string(root.join("crates/codeflow-present/web/src/styles.css"))
        .expect("present styles are readable");
    let rule = |css: &str, selector: &str| -> String {
        let start = css
            .find(&format!("{selector} {{"))
            .unwrap_or_else(|| panic!("no rule for {selector}"));
        let body = &css[start..];
        body[..body.find('}').expect("rule closes")].to_owned()
    };
    for (selector, role) in [
        ("#cf-present-document h1", "display"),
        ("#cf-present-document .cf-doc-summary", "lede"),
        ("#cf-present-document .cf-doc-meta", "micro"),
        ("#cf-present-document h2", "h2"),
        ("#cf-present-document h3", "h3"),
        ("#cf-present-document .block--narrative h2 + p", "lede"),
        (
            "#cf-present-document .comparison h2, #cf-present-document .decision h2, #cf-present-document .tabs > h2",
            "title",
        ),
        ("#cf-present-document .cf-form__title", "title"),
        (
            "#cf-present-document .callout > h2, #cf-present-document .block--tree h2",
            "title",
        ),
        (".cf-frame-title", "title"),
    ] {
        let body = rule(&present, selector);
        assert!(
            body.contains(&format!("font-size: var(--cf-fs-{role});")),
            "present {selector} must size with --cf-fs-{role}: {body}"
        );
    }
    for path in [
        "docs-portal/src/styles/portal.css",
        "assets/docs-portal/starter/src/styles/portal.css",
    ] {
        let css = std::fs::read_to_string(root.join(path)).expect("portal styles are readable");
        for (selector, role) in [
            (".cf-title", "display"),
            (".cf-lede", "lede"),
            (".cf-main .sl-markdown-content h2", "h2"),
            (".cf-main .sl-markdown-content h3", "h3"),
            (
                ".cf-main .sl-markdown-content :is(h2, .sl-heading-wrapper:has(> h2)) + p",
                "lede",
            ),
        ] {
            let body = rule(&css, selector);
            assert!(
                body.contains(&format!("font-size: var(--cf-fs-{role});")),
                "{path} {selector} must size with --cf-fs-{role}: {body}"
            );
        }
    }
    let figure = read_kit("figure.css");
    assert!(
        rule(&figure, ".cf-fig-title")
            .contains("font-size: max(15px, calc(15px * var(--cf-ui-scale, 1)));"),
        "the figure title line sits on the 15px block title step"
    );
}

/// The figure sheet's tones change hue only (rule 3): every tone rule sets
/// fill or stroke from a tone role, and nothing else, so a toned mark keeps
/// the kind, width, dash and shape its state draws with.
#[test]
fn figure_tones_change_hue_only() {
    let figure = read_kit("figure.css");
    let tone_rules: Vec<&str> = figure
        .lines()
        .filter(|line| line.starts_with("[data-cf-tone"))
        .collect();
    assert!(
        tone_rules.len() >= 6,
        "the figure sheet keys three tones and their marks"
    );
    for tone in ["accent", "cat1", "cat2"] {
        assert!(
            figure.contains(&format!("[data-cf-tone=\"{tone}\"] {{ --cf-fig-tone: var(--cf-fig-{tone}); --cf-fig-tone-soft: var(--cf-fig-{tone}-soft); }}")),
            "tone {tone} maps to its ink and tint"
        );
    }
    for line in tone_rules {
        let body =
            &line[line.find('{').expect("rule opens") + 1..line.rfind('}').expect("rule closes")];
        for declaration in body
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            let (property, value) = declaration.split_once(':').expect("a declaration");
            assert!(
                matches!(
                    property.trim(),
                    "fill" | "stroke" | "--cf-fig-tone" | "--cf-fig-tone-soft"
                ) && value.trim().starts_with("var(--cf-fig-"),
                "a tone rule may set only a hue from a figure role: {declaration}"
            );
        }
    }
}

#[test]
fn every_figure_ink_pair_meets_the_simulated_floor() {
    let palettes = tokens();
    let source: serde_json::Value = serde_json::from_str(&read_kit("source/colour.json")).unwrap();
    let floor = colour::floor("simulatedSeparation");
    for pair in PAIRS {
        let (theme, mode) = pair.rsplit_once('-').unwrap();
        for accent in source["themes"][theme].as_array().unwrap() {
            let accent = accent.as_str().unwrap();
            let mut roles = palettes[pair].clone();
            roles.insert(
                "fig-accent".to_owned(),
                source["accents"][accent][mode].as_str().unwrap().to_owned(),
            );
            let names = [
                "fig-line",
                "fig-accent",
                "fig-cat1",
                "fig-cat2",
                "fig-warn",
                "fig-stop",
            ];
            for simulation in ["protan", "deutan", "tritan"] {
                let mut worst = (f64::MAX, "", "");
                for (index, first) in names.iter().enumerate() {
                    for second in &names[index + 1..] {
                        let distance = colour::simulated_separation(
                            &roles[*first],
                            &roles[*second],
                            simulation,
                        );
                        if distance < worst.0 {
                            worst = (distance, first, second);
                        }
                        assert!(distance >= floor, "{pair}/{accent}/{simulation} {first}/{second}: {distance:.4} < {floor}");
                    }
                }
                println!(
                    "{pair}/{accent}/{simulation}: worst {}/{} {:.4}",
                    worst.1, worst.2, worst.0
                );
            }
        }
    }
    for simulation in ["protan", "deutan", "tritan"] {
        assert!(colour::simulated_separation("#222222", "#232323", simulation) < floor);
    }
}
