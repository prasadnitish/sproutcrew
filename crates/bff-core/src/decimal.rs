//! Credit balances arrive as decimal strings. Compare them exactly, never as floats.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    /// Digits with no leading or trailing zeros. Empty means zero.
    int: String,
    frac: String,
}

impl Decimal {
    pub fn parse(s: &str) -> Option<Decimal> {
        let (negative, rest) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
        let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
        if (int.is_empty() && frac.is_empty()) || !digits(int) || !digits(frac) {
            return None;
        }
        let int = int.trim_start_matches('0').to_string();
        let frac = frac.trim_end_matches('0').to_string();
        let is_zero = int.is_empty() && frac.is_empty();
        Some(Decimal {
            negative: negative && !is_zero,
            int,
            frac,
        })
    }
}

impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.negative { "-" } else { "" };
        let int = if self.int.is_empty() { "0" } else { &self.int };
        if self.frac.is_empty() {
            write!(f, "{sign}{int}")
        } else {
            write!(f, "{sign}{int}.{}", self.frac)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_values_with_different_spelling_compare_equal() {
        assert_eq!(Decimal::parse("12.50"), Decimal::parse("12.5"));
        assert_eq!(Decimal::parse("012.5"), Decimal::parse("12.5"));
        assert_eq!(Decimal::parse("12"), Decimal::parse("12.000"));
        assert_eq!(Decimal::parse("-0"), Decimal::parse("0"));
    }

    #[test]
    fn different_values_compare_unequal() {
        assert_ne!(Decimal::parse("12.50"), Decimal::parse("12.51"));
        assert_ne!(Decimal::parse("-1"), Decimal::parse("1"));
    }

    #[test]
    fn negative_balance_parses() {
        assert_eq!(Decimal::parse("-3.25").unwrap().to_string(), "-3.25");
    }

    #[test]
    fn garbage_is_rejected() {
        for s in ["", "abc", "1.2.3", "1e5", " 1", "1,5", "--1", "."] {
            assert!(Decimal::parse(s).is_none(), "{s:?}");
        }
    }

    #[test]
    fn display_is_canonical() {
        assert_eq!(Decimal::parse("007.1000").unwrap().to_string(), "7.1");
        assert_eq!(Decimal::parse("0").unwrap().to_string(), "0");
    }
}
