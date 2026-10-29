//! Transponder codes. A code is its four octal digits read as a decimal number
//! (`4201` is "four two zero one"), as the pilot hears and dials it.

use std::ops::RangeInclusive;

/// Codes never handed out: 0000, VFR (7000, 2000 and 1200 in other regions) and the
/// emergency codes (unlawful interference, radio failure, emergency).
const RESERVED: [u16; 7] = [0, 1200, 2000, 7000, 7500, 7600, 7700];

/// Default pool of discrete codes the engine assigns from.
pub const DEFAULT_POOL: RangeInclusive<u16> = 4201..=4277;

/// A code that may be assigned: four octal digits, not reserved.
pub fn is_discrete(code: u16) -> bool {
    code <= 7777 && !format!("{code}").chars().any(|c| c > '7') && !RESERVED.contains(&code)
}

/// The assignable codes of `pool`, in order (`4206..=4211` -> 4206, 4207, 4210, 4211).
pub fn discrete_codes(pool: &RangeInclusive<u16>) -> impl Iterator<Item = u16> {
    pool.clone().filter(|&c| is_discrete(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_octal_unreserved_codes() {
        assert!(is_discrete(4201));
        assert!(is_discrete(421));
        assert!(!is_discrete(4208));
        assert!(!is_discrete(4290));
        assert!(!is_discrete(8000));
        assert!(!is_discrete(17777));
        for c in RESERVED {
            assert!(!is_discrete(c), "{c}");
        }
        assert_eq!(
            discrete_codes(&(4206..=4211)).collect::<Vec<_>>(),
            vec![4206, 4207, 4210, 4211]
        );
        assert_eq!(
            discrete_codes(&(7476..=7502)).collect::<Vec<_>>(),
            vec![7476, 7477, 7501, 7502]
        );
        assert_eq!(discrete_codes(&DEFAULT_POOL).count(), 63);
    }
}
