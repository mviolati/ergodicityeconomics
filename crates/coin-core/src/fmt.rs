//! Italian number formatting (thousands ".", decimals ",").
//!
//! Values are TRUNCATED, never rounded up. A shown value is therefore never above the true
//! value, and a player below 1 billion EUR is never shown as "1,00 mld €".

const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// Integer with thousands separators: 1234567 -> "1.234.567".
pub fn int(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Exponent in Unicode superscript: -21 -> "⁻²¹".
pub fn superscript(e: i32) -> String {
    let mut s = String::new();
    if e < 0 {
        s.push('⁻');
    }
    for c in e.unsigned_abs().to_string().chars() {
        s.push(SUP[c.to_digit(10).expect("digit") as usize]);
    }
    s
}

/// Share `part / whole` in percent, truncated to 3 decimals: "0,027%", "< 0,001%", "100%".
pub fn pct(part: u64, whole: u64) -> String {
    assert!(whole > 0 && part <= whole);
    if part == whole {
        return "100%".into();
    }
    if part == 0 {
        return "0%".into();
    }
    // thousandths of a percent, exact integer arithmetic
    let milli = u128::from(part) * 100_000 / u128::from(whole);
    if milli == 0 {
        return "< 0,001%".into();
    }
    let (whole_pct, frac) = (milli / 1000, milli % 1000);
    let mut s = int(whole_pct as u64);
    if frac > 0 {
        let f = format!("{frac:03}");
        s.push(',');
        s.push_str(f.trim_end_matches('0'));
    }
    s.push('%');
    s
}

/// `d` significant digits of 10^l, truncated: (digits, exponent of the first digit).
fn sig(l: f64, d: u32) -> (u64, i32) {
    let e = l.floor();
    let m = 10f64.powf(l - e); // in [1, 10)
    let scale = 10f64.powi(d as i32 - 1);
    // 1e-12 relative slack absorbs log/pow round-trip error (1.2299999... for 1.23). It cannot
    // lift a lattice value across a threshold: no lattice point is within 2e-9 of one.
    let mut digits = (m * scale * (1.0 + 1e-12)).floor() as u64;
    // powf can return a hair under 1.0 for l an exact integer; never report 0.99 for 1.
    if digits < scale as u64 {
        digits = scale as u64;
    }
    (digits.min(10u64.pow(d) - 1), e as i32)
}

/// 3 significant digits for 10^l with l in [-2, 3): "0,0123", "0,999", "1,00", "12,3", "999".
fn three(l: f64) -> String {
    let (digits, e) = sig(l, 3);
    let s = format!("{digits:03}");
    match e {
        2 => s,
        1 => format!("{},{}", &s[..2], &s[2..]),
        0 => format!("{},{}", &s[..1], &s[1..]),
        -1 => format!("0,{s}"),
        -2 => format!("0,0{s}"),
        _ => unreachable!("three() takes l in [-2, 3)"),
    }
}

/// Wealth in EUR from its log10, truncated:
/// "3,1 × 10⁻¹³ €", "0,0123 €", "12,3 €", "6.216 €", "134 mln €", "7,72 mld €", "8,4 × 10¹⁶ €".
pub fn eur(l: f64) -> String {
    assert!(l.is_finite(), "wealth must be finite");
    if !(-2.0..12.0).contains(&l) {
        let (digits, e) = sig(l, 2);
        return format!("{},{} × 10{} €", digits / 10, digits % 10, superscript(e));
    }
    if l >= 9.0 {
        return format!("{} mld €", three(l - 9.0));
    }
    if l >= 6.0 {
        return format!("{} mln €", three(l - 6.0));
    }
    if l >= 3.0 {
        // Same slack as in `sig`: 5.000 € must not print as 4.999 €.
        let v = (10f64.powf(l) * (1.0 + 1e-12)).floor() as u64;
        return format!("{} €", int(v.max(1000)));
    }
    format!("{} €", three(l))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_and_percentages() {
        assert_eq!(int(0), "0");
        assert_eq!(int(999), "999");
        assert_eq!(int(1000), "1.000");
        assert_eq!(int(1_234_567), "1.234.567");
        assert_eq!(pct(277, 1_000_000), "0,027%");
        assert_eq!(pct(1, 1_000_000), "< 0,001%");
        assert_eq!(pct(839, 1000), "83,9%");
        assert_eq!(pct(999_999, 1_000_000), "99,999%");
        assert_eq!(pct(1000, 1000), "100%");
        assert_eq!(pct(0, 1000), "0%");
        assert_eq!(pct(1, 3), "33,333%");
    }

    #[test]
    fn euros() {
        assert_eq!(eur(2.0), "100 €");
        assert_eq!(eur(0.0), "1,00 €");
        assert_eq!(eur(3.0f64.log10()), "3,00 €");
        assert_eq!(eur(12.345f64.log10()), "12,3 €");
        assert_eq!(eur(0.0123f64.log10()), "0,0123 €");
        assert_eq!(eur(6216.9f64.log10()), "6.216 €");
        assert_eq!(eur(5000f64.log10()), "5.000 €");
        assert_eq!(eur(1000f64.log10()), "1.000 €");
        assert_eq!(eur(1.34e8f64.log10()), "134 mln €");
        assert_eq!(eur(7.729e9f64.log10()), "7,72 mld €");
        assert_eq!(eur(16.9255), "8,4 × 10¹⁶ €");
        assert_eq!(eur(-12.52), "3,0 × 10⁻¹³ €");
        assert_eq!(eur(12.0), "1,0 × 10¹² €");
    }

    #[test]
    fn truncation_never_crosses_a_threshold() {
        // Just below 1 billion and just below 1 EUR stay below.
        assert_eq!(eur(9.0 - 1e-9), "999 mln €");
        assert_eq!(eur(-1e-9), "0,999 €");
        assert_eq!(eur(9.0 + 1e-9), "1,00 mld €");
        assert_eq!(superscript(-21), "⁻²¹");
    }
}
