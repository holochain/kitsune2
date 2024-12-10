use super::*;
use kitsune2_api::id::Id;

#[inline(always)]
fn create() -> Inner {
    Inner::new(MemPeerStoreConfig::default(), std::time::Instant::now())
}

const AGENT_1: AgentId = AgentId(Id(bytes::Bytes::from_static(b"agent1")));
const AGENT_2: AgentId = AgentId(Id(bytes::Bytes::from_static(b"agent2")));
const SPACE_1: SpaceId = SpaceId(Id(bytes::Bytes::from_static(b"space1")));

/// Sneak some test-data into the url field (as the peer id)
/// this will let us validate store actions when we extract
/// it again later via [unsneak_url].
fn sneak_url(s: &str) -> Url {
    Url::from_str(format!("ws://a.b:80/{s}")).unwrap()
}

/// Extract some test-data from the url field (from the peer id)
/// that was put in via the [sneak_url] function.
fn unsneak_url(u: &Url) -> String {
    u.peer_id().unwrap().into()
}

#[derive(Debug, Default)]
struct AgentBuild {
    pub agent: Option<AgentId>,
    pub space: Option<SpaceId>,
    pub created_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub is_tombstone: Option<bool>,
    pub url: Option<Option<Url>>,
    pub storage_arc: Option<DhtArc>,
}

impl AgentBuild {
    pub fn build(self) -> Arc<AgentInfoSigned> {
        static NEXT_AGENT: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let agent = self.agent.unwrap_or_else(|| {
            let a =
                NEXT_AGENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let a = a.to_le_bytes();
            AgentId(Id(bytes::Bytes::copy_from_slice(&a)))
        });
        let space = self.space.unwrap_or_else(|| SPACE_1.clone());
        let created_at = self.created_at.unwrap_or_else(Timestamp::now);
        let expires_at = self.expires_at.unwrap_or_else(|| {
            created_at + std::time::Duration::from_secs(60 * 20)
        });
        let is_tombstone = self.is_tombstone.unwrap_or(false);
        let url = self.url.unwrap_or(None);
        let storage_arc = self.storage_arc.unwrap_or(DhtArc::FULL);
        let agent_info = serde_json::to_string(&AgentInfo {
            agent,
            space,
            created_at,
            expires_at,
            is_tombstone,
            url,
            storage_arc,
        })
        .unwrap();
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Enc {
            agent_info: String,
            signature: String,
        }
        let encoded = serde_json::to_string(&Enc {
            agent_info,
            signature: "".into(),
        })
        .unwrap();
        #[derive(Debug)]
        struct V;
        impl Verifier for V {
            fn verify(&self, _i: &AgentInfo, _m: &[u8], _s: &[u8]) -> bool {
                true
            }
        }
        // going through this trouble to use decode because it's sync
        AgentInfoSigned::decode(&V, encoded.as_bytes()).unwrap()
    }
}

#[test]
fn empty_store() {
    let mut s = create();

    assert_eq!(0, s.get_all().len());
}

#[test]
fn prune_prunes_only_expired_agents() {
    let mut s = create();

    s.insert(vec![
        AgentBuild {
            expires_at: Some(Timestamp::now()),
            ..Default::default()
        }
        .build(),
        AgentBuild {
            expires_at: Some(
                Timestamp::now() + std::time::Duration::from_secs(10),
            ),
            ..Default::default()
        }
        .build(),
    ]);

    s.do_prune(
        std::time::Instant::now(),
        Timestamp::now() + std::time::Duration::from_secs(5),
    );

    assert_eq!(1, s.get_all().len());
}

#[test]
fn happy_get() {
    let mut s = create();

    s.insert(vec![AgentBuild {
        agent: Some(AGENT_1),
        ..Default::default()
    }
    .build()]);

    let a = s.get(AGENT_1).unwrap();
    assert_eq!(a.agent, AGENT_1);
}

#[test]
fn happy_get_all() {
    let mut s = create();

    s.insert(vec![
        AgentBuild {
            agent: Some(AGENT_1),
            ..Default::default()
        }
        .build(),
        AgentBuild {
            agent: Some(AGENT_2),
            ..Default::default()
        }
        .build(),
    ]);

    let mut a = s
        .get_all()
        .into_iter()
        .map(|a| a.agent.clone())
        .collect::<Vec<_>>();
    a.sort();
    assert_eq!(&[AGENT_1, AGENT_2], a.as_slice());
}

#[test]
fn fixture_get_by_overlapping_storage_arc() {
    const fn u32f(f: f64) -> u32 {
        (u32::MAX as f64 * f) as u32
    }

    #[allow(clippy::type_complexity)]
    const F: &[(&[&str], DhtArc, &[(&str, DhtArc)])] = &[
        (
            &["a", "b"],
            DhtArc::FULL,
            &[("a", DhtArc::FULL), ("b", DhtArc::FULL)],
        ),
        (
            &[],
            DhtArc::FULL,
            &[("a", DhtArc::Empty), ("b", DhtArc::Empty)],
        ),
        (
            &[],
            DhtArc::Empty,
            &[("a", DhtArc::FULL), ("b", DhtArc::FULL)],
        ),
        (
            &["a"],
            DhtArc::Arc(0, u32::MAX / 2),
            &[
                ("a", DhtArc::Arc(400, u32::MAX / 2 - 400)),
                ("b", DhtArc::Arc(u32f(0.8), u32f(0.9))),
            ],
        ),
    ];

    for (exp, q, arc_list) in F.iter() {
        let mut s = create();

        for (arc_name, arc) in arc_list.iter() {
            s.insert(vec![AgentBuild {
                storage_arc: Some(arc.clone()),
                url: Some(Some(sneak_url(arc_name))),
                ..Default::default()
            }
            .build()]);
        }

        let mut got = s
            .get_by_overlapping_storage_arc(q.clone())
            .into_iter()
            .map(|info| unsneak_url(info.url.as_ref().unwrap()))
            .collect::<Vec<_>>();

        got.sort();

        assert_eq!(exp, &got.as_slice());
    }
}

#[test]
fn fixture_get_near_location() {
    let mut s = create();

    for idx in 0..8 {
        let loc = (u32::MAX / 8) * idx;
        s.insert(vec![AgentBuild {
            // for simplicity have agents claim arcs of len 1
            storage_arc: Some(DhtArc::Arc(loc, loc + 1)),
            // set the url to the idx for matching
            url: Some(Some(sneak_url(&idx.to_string()))),
            ..Default::default()
        }
        .build()]);
    }

    // these should not be returned because they are invalid.
    s.insert(vec![
        AgentBuild {
            storage_arc: Some(DhtArc::Empty),
            url: Some(Some(sneak_url("zero-arc"))),
            ..Default::default()
        }
        .build(),
        AgentBuild {
            is_tombstone: Some(true),
            url: Some(Some(sneak_url("tombstone"))),
            ..Default::default()
        }
        .build(),
        AgentBuild {
            expires_at: Some(Timestamp::from_micros(
                Timestamp::now().as_micros()
                    - std::time::Duration::from_secs(10).as_micros() as i64,
            )),
            url: Some(Some(sneak_url("expired"))),
            ..Default::default()
        }
        .build(),
    ]);

    const F: &[(&[&str], u32)] = &[
        (&["0", "1", "7", "2", "6", "3", "5", "4"], 0),
        (&["0", "1", "7", "2", "6", "3", "5", "4"], u32::MAX),
        (&["4", "5", "3", "6", "2", "7", "1", "0"], u32::MAX / 2),
    ];

    for (exp, loc) in F {
        let got = s
            .get_near_location(*loc, 42)
            .into_iter()
            .map(|info| unsneak_url(info.url.as_ref().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(exp, &got.as_slice());
    }
}
