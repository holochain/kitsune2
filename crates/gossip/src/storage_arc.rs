use kitsune2_api::{DhtArc, K2Result};
use kitsune2_api::{DynLocalAgent, LocalAgent};
use kitsune2_dht::ArcSet;
use kitsune2_dht::DhtSnapshot;
use std::collections::HashSet;

/// Update the storage arcs of local agents based on a DhtSnapshot.
///
/// The snapshot may either be a [DhtSnapshot::RingSectorDetails] or a [DhtSnapshot::Minimal].
/// A minimal snapshot may only be provided when the snapshot has already been checked to be
/// identical to our own minimal snapshot. In that case we know that the entire common arc set
/// was in sync.
///
/// Note that the DHT model prioritises syncing the disc. So if we've reached a ring diff then we
/// can assume that the disc was synced at the point this diff was produced. This function requires
/// the input [DhtSnapshot] to be a [DhtSnapshot::RingSectorDetails] variant.
///
/// The function looks for sectors within the common arc set that did not show up as a mismatch
/// in the snapshot. Such sectors are considered synced. If any synced sectors can be added to the
/// start or end of the current storage arc for a local agent, it will do so. Any synced sectors
/// that cannot be added to the storage arc to produce a larger continuous arc are ignored.
pub(crate) fn update_storage_arcs(
    snapshot: &DhtSnapshot,
    local_agents: Vec<DynLocalAgent>,
    common_arc_set: ArcSet,
) -> K2Result<()> {
    let mismatched_sectors = match snapshot {
        DhtSnapshot::Minimal { .. } => HashSet::with_capacity(0),
        DhtSnapshot::RingSectorDetails {
            ring_sector_hashes, ..
        } => {
            // These sectors didn't match, so we can't include them in our storage arc.
            ring_sector_hashes
                .values()
                .flat_map(|v| v.keys())
                .copied()
                .collect::<HashSet<_>>()
        }
        _ => {
            tracing::info!("Unable to update storage arc with a non-ring sector details snapshot");
            return Ok(());
        }
    };

    // The difference between the common arc set used to construct the DhtSnapshot and the
    // mismatched sectors is the set of sectors that we can use to update our storage arc.
    let synced_arcs = common_arc_set
        .without_sector_indices(mismatched_sectors)
        .as_arcs();

    for local_agent in local_agents {
        let target_storage_arc = local_agent.get_tgt_storage_arc();
        let current_storage_arc = local_agent.get_cur_storage_arc();

        if target_storage_arc == current_storage_arc {
            // No adjustment needed, already at the target.
            continue;
        }

        let target_set = ArcSet::new(vec![target_storage_arc])?;

        // Add our current storage arc to the set of sectors covered by synced arcs.
        let mut synced_arc_set = synced_arcs.clone();
        synced_arc_set.push(current_storage_arc);

        // Then intersect the result with our target arc. This prevents the storage arc from
        // growing outside the target arc. That might happen when we have multiple local agents
        // with different target arcs.
        let new_arcs = ArcSet::new(synced_arc_set)?
            .intersection(&target_set)
            .as_arcs();

        #[cfg(feature = "sharding")]
        if current_storage_arc == DhtArc::Empty {
            // When our current storage arc is empty, we're free to pick a new one that is
            // contained within the target arc. Pick the largest one.
            // TODO we might want to revisit this logic so that we pick an arc that starts from the
            //      agent's location.
            if let Some(new_arc) =
                new_arcs.into_iter().max_by_key(|arc| arc.len())
            {
                local_agent.set_cur_storage_arc(new_arc);
            }
        } else {
            // At this point, we have added any synced sectors to our current storage arc and
            // ignored sectors that aren't in our target arc. We can find the largest overlapping
            // arc and that will contain our old storage arc with any new sectors added.
            if let Some(new_arc) = new_arcs.into_iter().find(|arc| {
                arc.len() > current_storage_arc.len()
                    && arc.overlaps(&current_storage_arc)
            }) {
                local_agent.set_cur_storage_arc(new_arc);
            }
        }
        #[cfg(not(feature = "sharding"))]
        if new_arcs.into_iter().any(|arc| arc == DhtArc::FULL) {
            local_agent.set_cur_storage_arc(DhtArc::FULL);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kitsune2_api::{DhtArc, UNIX_TIMESTAMP};
    use kitsune2_core::Ed25519LocalAgent;
    use kitsune2_dht::SECTOR_SIZE;
    use std::sync::Arc;

    fn test_snapshot_with_mismatched_sectors(
        mismatched: &[u32],
    ) -> DhtSnapshot {
        DhtSnapshot::RingSectorDetails {
            ring_sector_hashes: [(
                0,
                mismatched
                    .iter()
                    .map(|s| (*s, bytes::Bytes::new()))
                    .collect(),
            )]
            .into_iter()
            .collect(),
            disc_boundary: UNIX_TIMESTAMP,
        }
    }

    #[test]
    fn storage_arc_at_target() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());
        let arc = DhtArc::Arc(0, 5 * SECTOR_SIZE - 1);
        local_agent.set_cur_storage_arc(arc);
        local_agent.set_tgt_storage_arc_hint(arc);

        update_storage_arcs(
            &test_snapshot_with_mismatched_sectors(&[0, 1, 2]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc doesn't change
        assert_eq!(arc, local_agent.get_cur_storage_arc());
    }

    #[cfg(feature = "sharding")]
    #[test]
    fn no_mismatched_sectors_from_empty() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());

        // Start with an empty storage arc
        local_agent.set_cur_storage_arc(DhtArc::Empty);

        // and a target arc that is larger than the current storage arc
        let arc = DhtArc::Arc(0, 5 * SECTOR_SIZE - 1);
        local_agent.set_tgt_storage_arc_hint(arc);

        update_storage_arcs(
            // No mismatched sectors
            &test_snapshot_with_mismatched_sectors(&[]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc is updated to the target
        assert_eq!(arc, local_agent.get_cur_storage_arc());
        assert_eq!(
            local_agent.get_tgt_storage_arc(),
            local_agent.get_cur_storage_arc()
        );
    }

    #[cfg(feature = "sharding")]
    #[test]
    fn some_mismatched_sectors_from_empty() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());

        // Start with an empty storage arc
        local_agent.set_cur_storage_arc(DhtArc::Empty);

        // and a target arc that is larger than the current storage arc
        local_agent
            .set_tgt_storage_arc_hint(DhtArc::Arc(0, 5 * SECTOR_SIZE - 1));

        update_storage_arcs(
            // one mismatched sector, in the middle of the target arc
            &test_snapshot_with_mismatched_sectors(&[3]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc is updated to the largest of the possible arcs around the mismatched sector
        assert_eq!(
            DhtArc::Arc(0, 3 * SECTOR_SIZE - 1),
            local_agent.get_cur_storage_arc()
        );
    }

    #[test]
    fn discover_sectors_that_do_not_expand_arc() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());

        // Start with an arc that is smaller than the target arc
        let arc = DhtArc::Arc(0, 2 * SECTOR_SIZE - 1);
        local_agent.set_cur_storage_arc(arc);

        // and some larger target arc
        local_agent
            .set_tgt_storage_arc_hint(DhtArc::Arc(0, 5 * SECTOR_SIZE - 1));

        update_storage_arcs(
            // mismatch at the end of our arc, but that means there are two sectors that have
            // matched. We can't expand our arc until `2` matches.
            &test_snapshot_with_mismatched_sectors(&[2]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc cannot be updated
        assert_eq!(arc, local_agent.get_cur_storage_arc());
    }

    #[cfg(feature = "sharding")]
    #[test]
    fn expand_towards_target_arc() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());

        // Start with an arc that is smaller than the target arc
        let arc = DhtArc::Arc(2 * SECTOR_SIZE, 5 * SECTOR_SIZE - 1);
        local_agent.set_cur_storage_arc(arc);

        // and some larger target arc
        local_agent
            .set_tgt_storage_arc_hint(DhtArc::Arc(0, 10 * SECTOR_SIZE - 1));

        update_storage_arcs(
            // mismatch on either side of our storage arc, but with room to grow
            &test_snapshot_with_mismatched_sectors(&[0, 8]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc grows as much as it can
        assert_eq!(
            DhtArc::Arc(SECTOR_SIZE, 8 * SECTOR_SIZE - 1),
            local_agent.get_cur_storage_arc()
        );
    }

    #[cfg(feature = "sharding")]
    #[test]
    fn update_multiple_agents() {
        let local_agent_1 = Arc::new(Ed25519LocalAgent::default());
        let local_agent_2 = Arc::new(Ed25519LocalAgent::default());
        let local_agent_3 = Arc::new(Ed25519LocalAgent::default());

        // Set up an arc outside the common arc set, should not change
        local_agent_1.set_tgt_storage_arc_hint(DhtArc::FULL);
        let arc_1 = DhtArc::Arc(10 * SECTOR_SIZE, 15 * SECTOR_SIZE - 1);
        local_agent_1.set_cur_storage_arc(arc_1);

        // Set up an arc that can grow
        local_agent_2.set_tgt_storage_arc_hint(DhtArc::FULL);
        let arc_2 = DhtArc::Arc(0, 5 * SECTOR_SIZE - 1);
        local_agent_2.set_cur_storage_arc(arc_2);

        // Set up an arc that can grow but won't be able to because of mismatched sectors
        local_agent_3.set_tgt_storage_arc_hint(DhtArc::FULL);
        let arc_3 = DhtArc::Arc(4 * SECTOR_SIZE, 9 * SECTOR_SIZE - 1);
        local_agent_3.set_cur_storage_arc(arc_3);

        update_storage_arcs(
            // Some sector mismatches
            &test_snapshot_with_mismatched_sectors(&[3, 4]),
            vec![
                local_agent_1.clone(),
                local_agent_2.clone(),
                local_agent_3.clone(),
            ],
            ArcSet::new(vec![DhtArc::Arc(0, 7 * SECTOR_SIZE - 1)]).unwrap(),
        )
        .unwrap();

        // Storage arc 1 restricted by common arc set
        assert_eq!(arc_1, local_agent_1.get_cur_storage_arc());

        // Storage arc 2 grows within the common arc set
        assert_eq!(
            DhtArc::Arc(0, 7 * SECTOR_SIZE - 1),
            local_agent_2.get_cur_storage_arc()
        );

        // Storage arc 3 restricted by mismatched sectors
        assert_eq!(arc_3, local_agent_3.get_cur_storage_arc());
    }

    #[cfg(not(feature = "sharding"))]
    #[test]
    fn expand_to_full_from_empty() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());
        local_agent.set_cur_storage_arc(DhtArc::Empty);
        local_agent.set_tgt_storage_arc_hint(DhtArc::FULL);

        update_storage_arcs(
            &test_snapshot_with_mismatched_sectors(&[]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc expands to full
        assert_eq!(DhtArc::FULL, local_agent.get_cur_storage_arc());
    }

    #[cfg(not(feature = "sharding"))]
    #[test]
    fn no_expand_when_any_sector_mismatches() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());
        local_agent.set_cur_storage_arc(DhtArc::Empty);
        local_agent.set_tgt_storage_arc_hint(DhtArc::FULL);

        update_storage_arcs(
            // Single mismatch
            &test_snapshot_with_mismatched_sectors(&[40]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Storage arc expands to full
        assert_eq!(DhtArc::Empty, local_agent.get_cur_storage_arc());
    }

    /// Note that this would be a mistake by the host implementation or some sharding logic.
    /// If the target arc is being reduced then the storage arc should be reduced too. It is
    /// not the job of this function to correct such mistakes. This test just checks that we
    /// don't do anything unexpected when the arcs are mixed up.
    #[test]
    fn storage_arc_larger_than_target() {
        let local_agent = Arc::new(Ed25519LocalAgent::default());

        // Start with some storage arc
        let arc = DhtArc::Arc(0, 50 * SECTOR_SIZE - 1);
        local_agent.set_cur_storage_arc(arc);

        // but a target arc that is smaller than the storage arc
        local_agent
            .set_tgt_storage_arc_hint(DhtArc::Arc(0, 10 * SECTOR_SIZE - 1));

        update_storage_arcs(
            // one mismatched sector, in the middle of the target arc
            &test_snapshot_with_mismatched_sectors(&[3]),
            vec![local_agent.clone()],
            ArcSet::new(vec![DhtArc::FULL]).unwrap(),
        )
        .unwrap();

        // Then the storage arc is not updated
        assert_eq!(arc, local_agent.get_cur_storage_arc());
    }
}
