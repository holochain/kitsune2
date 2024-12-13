use kitsune2_api::{DhtArc, K2Error, K2Result};
use std::collections::HashSet;

#[derive(Debug)]
pub struct ArcSet {
    inner: HashSet<u32>,
}

impl ArcSet {
    pub fn new(size: u32, arcs: Vec<DhtArc>) -> K2Result<Self> {
        let factor = u32::MAX / size + 1;

        // The original factor should have been a power of 2
        if factor == 0 || factor & (factor - 1) != 0 {
            return Err(K2Error::other("Invalid size"));
        }

        let mut inner = HashSet::new();
        for arc in arcs {
            // If we have reached full arc then there's no need to keep going
            if inner.len() == factor as usize {
                break;
            }

            match arc {
                DhtArc::Empty => {
                    continue;
                }
                DhtArc::Arc(start, end) => {
                    let num_sectors_covered = if start > end {
                        let length = u32::MAX - start + end + 1;
                        length / size + 1
                    } else {
                        (end - start) / size + 1
                    };

                    let mut start = start;
                    for _ in 0..num_sectors_covered {
                        inner.insert(start / size);
                        start = start.overflowing_add(size).0;
                    }

                    if start != end.overflowing_add(1).0
                        && !(end == u32::MAX && start == 0)
                    {
                        return Err(K2Error::other(format!(
                            "Invalid arc, expected end at {} but arc specifies {}",
                            start, end
                        )));
                    }
                }
            }
        }

        Ok(ArcSet { inner })
    }

    pub(crate) fn includes_sector_id(&self, value: u32) -> bool {
        self.inner.contains(&value)
    }

    pub(crate) fn covered_sector_count(&self) -> usize {
        self.inner.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SECTOR_SIZE: u32 = 1u32 << 23;

    #[test]
    fn new_with_no_arcs() {
        let set = ArcSet::new(SECTOR_SIZE, vec![]).unwrap();

        assert!(set.inner.is_empty());
    }

    #[test]
    fn new_with_full_arc() {
        let set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::FULL]).unwrap();

        // Sufficient to check that all the right values are included
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn new_with_two_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(0, 255 * SECTOR_SIZE - 1),
                DhtArc::Arc(255 * SECTOR_SIZE, u32::MAX),
            ],
        )
        .unwrap();

        // Should become a full arc
        assert_eq!(512, set.inner.len());
        assert_eq!(511, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn overlapping_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
                DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
            ],
        )
        .unwrap();

        assert_eq!(4, set.inner.len());
        assert_eq!(3, *set.inner.iter().max().unwrap());
    }

    #[test]
    fn wrapping_arc() {
        let set =
            ArcSet::new(SECTOR_SIZE, vec![DhtArc::Arc(510 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1)])
                .unwrap();

        assert_eq!(5, set.inner.len(), "Set is {:?}", set.inner);
        assert_eq!(
            set.inner.len(),
            set.inner
                .intersection(&vec![510, 511, 0, 1, 2].into_iter().collect())
                .count()
        );
    }

    #[test]
    fn overlapping_wrapping_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![
                DhtArc::Arc(510 * SECTOR_SIZE, 3 * SECTOR_SIZE - 1),
                DhtArc::Arc(2 * SECTOR_SIZE, 4 * SECTOR_SIZE - 1),
            ],
        )
        .unwrap();

        assert_eq!(6, set.inner.len(), "Set is {:?}", set.inner);
        assert_eq!(
            set.inner.len(),
            set.inner
                .intersection(&vec![510, 511, 0, 1, 2, 3].into_iter().collect())
                .count()
        );
    }

    #[test]
    fn arc_not_on_boundaries() {
        let set = ArcSet::new(SECTOR_SIZE, vec![DhtArc::Arc(0, 50)]);

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388608 but arc specifies 50 (src: None)",
            set.unwrap_err().to_string()
        );
    }

    #[test]
    fn valid_and_invalid_arcs() {
        let set = ArcSet::new(
            SECTOR_SIZE,
            vec![DhtArc::Arc(0, SECTOR_SIZE - 1), DhtArc::Arc(u32::MAX, u32::MAX)],
        );

        assert!(set.is_err());
        assert_eq!(
            "Invalid arc, expected end at 8388607 but arc specifies 4294967295 (src: None)",
            set.unwrap_err().to_string()
        );
    }
}
