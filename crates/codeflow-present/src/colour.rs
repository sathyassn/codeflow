//! Shared colour measurements for imported palettes and the kit contract.
//! CIEDE2000 follows Sharma, Wu and Dalal; simulation uses the published
//! Machado, Oliveira and Fernandes severity-1 matrices in the token source.

const MATRICES: &str = include_str!(
    "../../../assets/base/agents/skills/cf-present/resources/design-system/source/matrices.json"
);
const FLOORS: &str = include_str!(
    "../../../assets/base/agents/skills/cf-present/resources/design-system/source/floors.json"
);

pub(crate) fn floor(name: &str) -> f64 {
    let floors: serde_json::Value =
        serde_json::from_str(FLOORS).expect("embedded floors are valid");
    floors[name].as_f64().expect("embedded floor is numeric")
}

pub(crate) fn simulated_separation(first: &str, second: &str, simulation: &str) -> f64 {
    let matrices: std::collections::BTreeMap<String, [[f64; 3]; 3]> =
        serde_json::from_str(MATRICES).expect("embedded matrices are valid");
    let matrix = matrices.get(simulation).expect("known simulation");
    let simulate = |hex: &str| {
        let channels = rgb(hex).map(linear);
        lab_linear(matrix.map(|row| {
            row.into_iter()
                .zip(channels)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                .clamp(0.0, 1.0)
        }))
    };
    delta_e_lab(simulate(first), simulate(second))
}

pub(crate) fn rgb(hex: &str) -> [f64; 3] {
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

pub(crate) fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub(crate) fn lab(hex: &str) -> [f64; 3] {
    lab_linear(rgb(hex).map(linear))
}

fn lab_linear([red, green, blue]: [f64; 3]) -> [f64; 3] {
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
pub(crate) fn delta_e_2000(first: &str, second: &str) -> f64 {
    delta_e_lab(lab(first), lab(second))
}

pub(crate) fn delta_e_lab(first: [f64; 3], second: [f64; 3]) -> f64 {
    let [l1, a1, b1] = first;
    let [l2, a2, b2] = second;
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
