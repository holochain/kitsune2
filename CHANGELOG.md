# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## \[[0.3.1](https://github.com/holochain/kitsune2/compare/v0.3.0...v0.3.1)\] - 2025-11-10

### Bug Fixes

- Reduce lock hold on `space_map`

## \[[0.3.0](https://github.com/holochain/kitsune2/compare/v0.2.11...v0.3.0)\] - 2025-11-04

### Features

- Add CORS support to the bootstrap server by @ThetaSinner in [#355](https://github.com/holochain/kitsune2/pull/355)
- Bare-bones Docusaurus site with kitsune2 styling (#336) by @pdaoust in [#336](https://github.com/holochain/kitsune2/pull/336)
- Notify drained queue listeners when sending fetch request fails by @jost-s in [#338](https://github.com/holochain/kitsune2/pull/338)
- Check if agents are blocked when receiving a message (#320) by @matthme in [#320](https://github.com/holochain/kitsune2/pull/320)
- Add counter for blocked messages to the Transport trait (#335) by @matthme in [#335](https://github.com/holochain/kitsune2/pull/335)
- \[**BREAKING**\] Add report module with fetched op api and no-op core report module by @neonphog
- Upgrade tx5 to 0.8 by @ThetaSinner in [#322](https://github.com/holochain/kitsune2/pull/322)
- Support the go-pion backend for tx5 as an optional configuration by @ThetaSinner in [#301](https://github.com/holochain/kitsune2/pull/301)
- Make MemPeerStore inner insert method async by @cdunster
- Use async Mutex for MemPeerStore inner by @cdunster
- Add blocks module to the Space module by @cdunster
- Add method to PeerStore to get agents by peer URL by @cdunster
- Add common function to remove and block an agent by ID by @cdunster
- Check if agents are blocked before inserting into peer_store by @cdunster
- PeerStoreFactory::create now takes a DynBlocks by @cdunster
- Add in-memory blocks implementation as default Blocks by @cdunster
- Add mem_blocks module to implement Blocks and BlocksFactory by @cdunster
  - This is an in-memory implementation to be used for testing only.
- Add blocks factory to blocks module in API crate by @cdunster
- Add blocks module to API crate by @cdunster
- Add method to remove peer from peer_store based on AgentId by @cdunster

### Bug Fixes

- *(kitsune2_core)* Fix an issue in the `MemTransport` that allowed notify and module messages to be sent before the preflight exchange was complete by @ThetaSinner
- Remove requests from state if they could not be sent to queue by @jost-s
- \[**BREAKING**\] Remove as_any from report trait by @neonphog in [#331](https://github.com/holochain/kitsune2/pull/331)
- When an empty target list was passed to MemBlocks::are_all_blocked() it incorrectly returned true (#321) by @matthme in [#321](https://github.com/holochain/kitsune2/pull/321)
- Use default-features = false for transport dependencies by @ThetaSinner in [#314](https://github.com/holochain/kitsune2/pull/314)
- Increase default re-insert delay for fetch requests by @ThetaSinner in [#317](https://github.com/holochain/kitsune2/pull/317)
- Ensure the gossip initiate loop will always progress even if the fetch queue does not notify it by @ThetaSinner in [#304](https://github.com/holochain/kitsune2/pull/304)
- Fetch should not error when too many messages have been received by @ThetaSinner in [#305](https://github.com/holochain/kitsune2/pull/305)
- Close connections when marking a peer unresponsive by @ThetaSinner in [#303](https://github.com/holochain/kitsune2/pull/303)
- Out of bounds panic in the DHT time module caused by a wrong bounds check by @ThetaSinner in [#285](https://github.com/holochain/kitsune2/pull/285)
- Revert renaming of space field in AgentInfo (#278) by @matthme in [#278](https://github.com/holochain/kitsune2/pull/278)

### Miscellaneous Tasks

- *(kitsune2_core)* Add more tracing for prune and insert operations by @ThetaSinner
- *(kitsune2_gossip)* Downgrade some common tracing logs from info to debug level to reduce noise in logs by @ThetaSinner in [#307](https://github.com/holochain/kitsune2/pull/307)
- Remove MacOS specific overrides, not required at nixos-25.05 by @ThetaSinner in [#341](https://github.com/holochain/kitsune2/pull/341)
- Upgrade to Rust 1.88 by @ThetaSinner
- Upgrade tx5 to 0.7.1 by @ThetaSinner in [#306](https://github.com/holochain/kitsune2/pull/306)
- Update to the latest tx5 release which includes a bugfix for handling p2p connection setup failures by @ThetaSinner in [#296](https://github.com/holochain/kitsune2/pull/296)
- Allow a dedicated override for `OpId` by @ThetaSinner in [#279](https://github.com/holochain/kitsune2/pull/279)
- Downgrade "no peers" gossip tracing messages from info to debug because they're too noisy when no other peers are available on the network by @ThetaSinner in [#276](https://github.com/holochain/kitsune2/pull/276)
- Expose new module configuration for the tx5 transport `danger_force_signal_relay` by @ThetaSinner in [#270](https://github.com/holochain/kitsune2/pull/270)
- Update tx5 to 0.5.0 by @ThetaSinner

### Build System

- Speed up tests by running them once by @ThetaSinner

### CI

- Use `info` log level while running tests on CI to reduce the noise of the default `debug` level by @ThetaSinner in [#356](https://github.com/holochain/kitsune2/pull/356)

### Testing

- *(kitsune2_core)* Add a new test that preflight messages are send and received before other messages by @ThetaSinner in [#330](https://github.com/holochain/kitsune2/pull/330)
- Fix flaky `preflight_send_recv` test by @ThetaSinner in [#358](https://github.com/holochain/kitsune2/pull/358)
- Improve flaky `two_node_gossip` test by @ThetaSinner in [#357](https://github.com/holochain/kitsune2/pull/357)
- Purge state when request_ops fails by @jost-s
- Add test for when a tie-breaker check results in a tie by @cdunster in [#292](https://github.com/holochain/kitsune2/pull/292)
- Give test `two_node_gossip` more time to sync on CI by @ThetaSinner in [#308](https://github.com/holochain/kitsune2/pull/308)
- Attempt to make `two_new_agents_sync` less flaky on macos on CI by @ThetaSinner

### Refactor

- Remove tx5 test-utils dependency from kitsune crate (#349) by @matthme in [#349](https://github.com/holochain/kitsune2/pull/349)
- Remove re-insertion-logic from outgoing request in fetch module by @jost-s
- Make preflight handlers async (#326) by @matthme in [#326](https://github.com/holochain/kitsune2/pull/326)
- Clearer namings and more docs for the MemTransport (#323) by @matthme in [#323](https://github.com/holochain/kitsune2/pull/323)
- Extract block targets in tests into constants by @cdunster in [#290](https://github.com/holochain/kitsune2/pull/290)
- \[**BREAKING**\] Rename space to space_id for clarity where applicable (#273) by @matthme in [#273](https://github.com/holochain/kitsune2/pull/273)

### Styling

- Clean up imports in test module by @cdunster

### Documentation

- Remove mention of re-insertion from docs by @jost-s
- Update PeerStore::insert doc-comment to mention blocks by @cdunster

### Automated Changes

- *(deps)* Bump holochain/actions from 1.1.0 to 1.3.0 by @dependabot[bot] in [#337](https://github.com/holochain/kitsune2/pull/337)
- *(deps)* Bump actions/setup-go from 5 to 6 by @dependabot[bot] in [#332](https://github.com/holochain/kitsune2/pull/332)
- *(deps)* Bump actions/checkout from 4 to 5 by @dependabot[bot] in [#299](https://github.com/holochain/kitsune2/pull/299)

### First-time Contributors

- @pdaoust made their first contribution in [#336](https://github.com/holochain/kitsune2/pull/336)


## \[[0.3.0-dev.4](https://github.com/holochain/kitsune2/compare/v0.3.0-dev.3...v0.3.0-dev.4)\] - 2025-10-09

### Features

- Bare-bones Docusaurus site with kitsune2 styling (#336) by @pdaoust in [#336](https://github.com/holochain/kitsune2/pull/336)
- Notify drained queue listeners when sending fetch request fails by @jost-s in [#338](https://github.com/holochain/kitsune2/pull/338)
- Check if agents are blocked when receiving a message (#320) by @matthme in [#320](https://github.com/holochain/kitsune2/pull/320)
- Add counter for blocked messages to the Transport trait (#335) by @matthme in [#335](https://github.com/holochain/kitsune2/pull/335)

### Bug Fixes

- Remove requests from state if they could not be sent to queue by @jost-s

### Miscellaneous Tasks

- Remove MacOS specific overrides, not required at nixos-25.05 by @ThetaSinner in [#341](https://github.com/holochain/kitsune2/pull/341)
- Upgrade to Rust 1.88 by @ThetaSinner

### Testing

- Purge state when request_ops fails by @jost-s

### Refactor

- Remove re-insertion-logic from outgoing request in fetch module by @jost-s

### Documentation

- Remove mention of re-insertion from docs by @jost-s

### Automated Changes

- *(deps)* Bump holochain/actions from 1.1.0 to 1.3.0 by @dependabot[bot] in [#337](https://github.com/holochain/kitsune2/pull/337)
  - Bumps [holochain/actions](https://github.com/holochain/actions) from 1.1.0 to 1.3.0. - [Release notes](https://github.com/holochain/actions/releases) - [Commits](https://github.com/holochain/actions/compare/v1.1.0...v1.3.0)
  - Updated-dependencies: - dependency-name: holochain/actions   dependency-version: 1.3.0   dependency-type: direct:production   update-type: version-update:semver-minor ...
- *(deps)* Bump actions/setup-go from 5 to 6 by @dependabot[bot] in [#332](https://github.com/holochain/kitsune2/pull/332)
  - Bumps [actions/setup-go](https://github.com/actions/setup-go) from 5 to 6. - [Release notes](https://github.com/actions/setup-go/releases) - [Commits](https://github.com/actions/setup-go/compare/v5...v6)
  - Updated-dependencies: - dependency-name: actions/setup-go   dependency-version: '6'   dependency-type: direct:production   update-type: version-update:semver-major ...

### First-time Contributors

- @pdaoust made their first contribution in [#336](https://github.com/holochain/kitsune2/pull/336)

## \[[0.3.0-dev.3](https://github.com/holochain/kitsune2/compare/v0.3.0-dev.2...v0.3.0-dev.3)\] - 2025-09-11

### Features

- \[**BREAKING**\] Add report module with fetched op api and no-op core report module by @neonphog

### Bug Fixes

- *(kitsune2_core)* Fix an issue in the `MemTransport` that allowed notify and module messages to be sent before the preflight exchange was complete by @ThetaSinner
- \[**BREAKING**\] Remove as_any from report trait by @neonphog in [#331](https://github.com/holochain/kitsune2/pull/331)

### Testing

- *(kitsune2_core)* Add a new test that preflight messages are send and received before other messages by @ThetaSinner in [#330](https://github.com/holochain/kitsune2/pull/330)

### Doc

- Update crates/api/src/report.rs by @neonphog

## \[[0.3.0-dev.2](https://github.com/holochain/kitsune2/compare/v0.3.0-dev.1...v0.3.0-dev.2)\] - 2025-09-03

### Features

- Upgrade tx5 to 0.8 by @ThetaSinner in [#322](https://github.com/holochain/kitsune2/pull/322)
- Support the go-pion backend for tx5 as an optional configuration by @ThetaSinner in [#301](https://github.com/holochain/kitsune2/pull/301)
- Make MemPeerStore inner insert method async by @cdunster
- Use async Mutex for MemPeerStore inner by @cdunster
- Add blocks module to the Space module by @cdunster
- Add method to PeerStore to get agents by peer URL by @cdunster
- Add common function to remove and block an agent by ID by @cdunster
- Check if agents are blocked before inserting into peer_store by @cdunster
- PeerStoreFactory::create now takes a DynBlocks by @cdunster
- Add in-memory blocks implementation as default Blocks by @cdunster
- Add mem_blocks module to implement Blocks and BlocksFactory by @cdunster
  - This is an in-memory implementation to be used for testing only.
- Add blocks factory to blocks module in API crate by @cdunster
- Add blocks module to API crate by @cdunster
- Add method to remove peer from peer_store based on AgentId by @cdunster

### Bug Fixes

- When an empty target list was passed to MemBlocks::are_all_blocked() it incorrectly returned true (#321) by @matthme in [#321](https://github.com/holochain/kitsune2/pull/321)
- Use default-features = false for transport dependencies by @ThetaSinner in [#314](https://github.com/holochain/kitsune2/pull/314)
- Increase default re-insert delay for fetch requests by @ThetaSinner in [#317](https://github.com/holochain/kitsune2/pull/317)
  - This should improve performance because the fast fetch retry could spam the target before they get a chance to respond
- Ensure the gossip initiate loop will always progress even if the fetch queue does not notify it by @ThetaSinner in [#304](https://github.com/holochain/kitsune2/pull/304)
- Fetch should not error when too many messages have been received by @ThetaSinner in [#305](https://github.com/holochain/kitsune2/pull/305)
  - Instead, fetch should just handle as many messages as it is willing to and drop messages that it can't respond to. The remote peers whose requests fail will just have to try again.
- Close connections when marking a peer unresponsive by @ThetaSinner in [#303](https://github.com/holochain/kitsune2/pull/303)
- Out of bounds panic in the DHT time module caused by a wrong bounds check by @ThetaSinner in [#285](https://github.com/holochain/kitsune2/pull/285)
- Revert renaming of space field in AgentInfo (#278) by @matthme in [#278](https://github.com/holochain/kitsune2/pull/278)

### Miscellaneous Tasks

- *(kitsune2_core)* Add more tracing for prune and insert operations by @ThetaSinner
- *(kitsune2_gossip)* Downgrade some common tracing logs from info to debug level to reduce noise in logs by @ThetaSinner in [#307](https://github.com/holochain/kitsune2/pull/307)
- Upgrade tx5 to 0.7.1 by @ThetaSinner in [#306](https://github.com/holochain/kitsune2/pull/306)
- Update to the latest tx5 release which includes a bugfix for handling p2p connection setup failures by @ThetaSinner in [#296](https://github.com/holochain/kitsune2/pull/296)
- Allow a dedicated override for `OpId` by @ThetaSinner in [#279](https://github.com/holochain/kitsune2/pull/279)
- Downgrade "no peers" gossip tracing messages from info to debug because they're too noisy when no other peers are available on the network by @ThetaSinner in [#276](https://github.com/holochain/kitsune2/pull/276)

### Testing

- Add test for when a tie-breaker check results in a tie by @cdunster in [#292](https://github.com/holochain/kitsune2/pull/292)
- Give test `two_node_gossip` more time to sync on CI by @ThetaSinner in [#308](https://github.com/holochain/kitsune2/pull/308)
- Attempt to make `two_new_agents_sync` less flaky on macos on CI by @ThetaSinner

### Refactor

- Make preflight handlers async (#326) by @matthme in [#326](https://github.com/holochain/kitsune2/pull/326)
- Clearer namings and more docs for the MemTransport (#323) by @matthme in [#323](https://github.com/holochain/kitsune2/pull/323)
- Extract block targets in tests into constants by @cdunster in [#290](https://github.com/holochain/kitsune2/pull/290)
- \[**BREAKING**\] Rename space to space_id for clarity where applicable (#273) by @matthme in [#273](https://github.com/holochain/kitsune2/pull/273)

### Styling

- Clean up imports in test module by @cdunster

### Documentation

- Update PeerStore::insert doc-comment to mention blocks by @cdunster

### Automated Changes

- *(deps)* Bump actions/checkout from 4 to 5 by @dependabot[bot] in [#299](https://github.com/holochain/kitsune2/pull/299)
  - Bumps [actions/checkout](https://github.com/actions/checkout) from 4 to 5. - [Release notes](https://github.com/actions/checkout/releases) - [Changelog](https://github.com/actions/checkout/blob/main/CHANGELOG.md) - [Commits](https://github.com/actions/checkout/compare/v4...v5)
  - Updated-dependencies: - dependency-name: actions/checkout   dependency-version: '5'   dependency-type: direct:production   update-type: version-update:semver-major ...

## [0.3.0-dev.1] - 2025-07-24

### Changed

- Expose new module configuration for the tx5 transport `danger_force_signal_relay` by @ThetaSinner in [#270](https://github.com/holochain/kitsune2/pull/270)
- Update tx5 to 0.5.0 by @ThetaSinner

## [0.2.11] - 2025-07-02

### Changed

- Fix new flags for SBD config on the bootstrap server to be flags rather than arguments by @ThetaSinner in [#259](https://github.com/holochain/kitsune2/pull/259)

## [0.2.10] - 2025-07-01

### Added

- Add job to generate changelog preview as a comment on PRs by @cdunster in [#254](https://github.com/holochain/kitsune2/pull/254)

### Changed

- Add rate limiting flags to the bootstrap server for configuring SBD by @ThetaSinner in [#257](https://github.com/holochain/kitsune2/pull/257)
- Reduce the wait time when no local agents are available because it should be a temporary state by @ThetaSinner in [#253](https://github.com/holochain/kitsune2/pull/253)

### Fixed

- Replace flaky functional tests for gossip data limits with more reliable unit tests by @ThetaSinner in [#255](https://github.com/holochain/kitsune2/pull/255)
- Tests for agents message response handler by @ThetaSinner
- Tests for handling peer busy messages by @ThetaSinner
- Clear a warning about unused configuration in gossip functional tests by @ThetaSinner

## [0.2.9] - 2025-06-20

### Added

- Add a changelog by @cdunster in [#248](https://github.com/holochain/kitsune2/pull/248)
- Add workflow that tags and publishes a release upon merge to main by @cdunster in [#245](https://github.com/holochain/kitsune2/pull/245)
- Add a manually-triggered workflow to prepare a release by @cdunster

### Changed

- Bump dependencies by @ThetaSinner in [#251](https://github.com/holochain/kitsune2/pull/251)
- Update Cargo.toml use repository instead of homepage by @szabgab in [#244](https://github.com/holochain/kitsune2/pull/244)

## First-time Contributors

* @szabgab made their first contribution in [#244](https://github.com/holochain/kitsune2/pull/244)

## [0.2.8] - 2025-06-16

### Added

- Add common functions to filter out unresponsive agents by @jost-s
- Add method PeerMetaStore::get_all_by_key by @jost-s
- Add logic to filter unresponsive peers in gossip (#235) by @matthme in [#235](https://github.com/holochain/kitsune2/pull/235)
- Add get/set to mark peer unresponsive in peer meta store by @jost-s in [#229](https://github.com/holochain/kitsune2/pull/229)
- Add peer url to error message in tx5 send (#228) by @matthme in [#228](https://github.com/holochain/kitsune2/pull/228)

### Changed

- Check if queue is drained when re-inserting request failed by @jost-s in [#237](https://github.com/holochain/kitsune2/pull/237)
- Only re-insert requests into queue if their ops are still needed by @jost-s
- Change mutex unwraps to expects by @jost-s
- Rectify what get_all_by_key returns by @jost-s
- Log errors when querying peer meta store for unresponsive urls by @jost-s
- Simplify flow of outgoing request task by @jost-s
- Log errors when querying peer meta store for unresponsive urls by @jost-s
- Check if transport is dropped before checking if url is unresponsive by @jost-s
- Check that only an unresponsive url's requests are dropped in test by @jost-s
- Filter peers without url out in filter_unresponsive_agents by @jost-s
- Rewrite "if x/if not x" to if/else by @jost-s
- Update comment on unresponsive url by @jost-s
- Filter unresponsive urls when requesting ops by @jost-s
- Filter unresponsive urls when publishing by @jost-s
- Implement PeerMetaStore::get_all_by_key for test store by @jost-s
- Move long tx5 transport test to integration tests (#236) by @matthme in [#236](https://github.com/holochain/kitsune2/pull/236)
- Record peer connection failures in the peer meta store (#233) by @matthme in [#233](https://github.com/holochain/kitsune2/pull/233)
- Modify get/set to mark peer unresponsive in peer meta store by @jost-s in [#234](https://github.com/holochain/kitsune2/pull/234)
- Bump johnwason/vcpkg-action from 6 to 7 (#232) by @dependabot[bot] in [#232](https://github.com/holochain/kitsune2/pull/232)
- Faster initiate (#230) by @ThetaSinner in [#230](https://github.com/holochain/kitsune2/pull/230)
- Notify on fetch queue drained (#227) by @ThetaSinner in [#227](https://github.com/holochain/kitsune2/pull/227)
- Implement the showcase demo app (#219) by @cdunster in [#219](https://github.com/holochain/kitsune2/pull/219)
- Permit receiving a burst of gossip initiations (#226) by @ThetaSinner in [#226](https://github.com/holochain/kitsune2/pull/226)

### Fixed

- Send queue drained notification when no requests to be sent is empty by @jost-s
- Change timeout to 100 ms in unresponsive_urls_are_filtered by @jost-s

### Removed

- Remove only current request from state when url unresponsive by @jost-s
- Remove backoff and replace with unresponsive function in peer meta store by @jost-s

## [0.2.7] - 2025-05-23

### Changed

- Functioning net stats from mem transport (#225) by @neonphog in [#225](https://github.com/holochain/kitsune2/pull/225)
- Fix MemBootstrap Space Crossover (#224) by @neonphog in [#224](https://github.com/holochain/kitsune2/pull/224)

## [0.2.6] - 2025-05-15

### Fixed

- Fix authorization (#218) by @neonphog in [#218](https://github.com/holochain/kitsune2/pull/218)
- Fix test-auth-hook-server docker default cmd (#217) by @neonphog in [#217](https://github.com/holochain/kitsune2/pull/217)

## [0.2.5] - 2025-05-14

### Changed

- Bug fix and auth test binding change (#216) by @neonphog in [#216](https://github.com/holochain/kitsune2/pull/216)

## [0.2.4] - 2025-05-12

### Added

- Add rust-analyzer as a toolchain component (#214) by @cdunster in [#214](https://github.com/holochain/kitsune2/pull/214)
- Support cargo-make in Nix devShell (#213) by @cdunster in [#213](https://github.com/holochain/kitsune2/pull/213)

### Changed

- Simpler test target (#211) by @ThetaSinner in [#211](https://github.com/holochain/kitsune2/pull/211)
- Bootstrap-auth-test docker build (#212) by @neonphog in [#212](https://github.com/holochain/kitsune2/pull/212)
- Close spaces (#208) by @ThetaSinner in [#208](https://github.com/holochain/kitsune2/pull/208)

## [0.2.3] - 2025-05-08

### Added

- Add Nix devShell and direnv support (#207) by @cdunster in [#207](https://github.com/holochain/kitsune2/pull/207)

### Changed

- Authenticate (#198) by @neonphog in [#198](https://github.com/holochain/kitsune2/pull/198)
- Add cargo make (#193) by @ThetaSinner in [#193](https://github.com/holochain/kitsune2/pull/193)
- Use Google STUN (#210) by @ThetaSinner in [#210](https://github.com/holochain/kitsune2/pull/210)
- Fix feature logic for automock (#206) by @ThetaSinner in [#206](https://github.com/holochain/kitsune2/pull/206)

## First-time Contributors

* @cdunster made their first contribution in [#207](https://github.com/holochain/kitsune2/pull/207)

## [0.2.2] - 2025-04-29

### Changed

- Moar tx5 config (#202) by @neonphog in [#202](https://github.com/holochain/kitsune2/pull/202)

## [0.2.1] - 2025-04-29

### Changed

- Tidy dependencies (#201) by @ThetaSinner in [#201](https://github.com/holochain/kitsune2/pull/201)
- Move server address logging to binary (#199) by @anchalshivank in [#199](https://github.com/holochain/kitsune2/pull/199)

## First-time Contributors

* @anchalshivank made their first contribution in [#199](https://github.com/holochain/kitsune2/pull/199)

## [0.2.0] - 2025-04-28

### Changed

- K2 consistency fixes (#195) by @ThetaSinner in [#195](https://github.com/holochain/kitsune2/pull/195)
- Add schema support for configuration (#194) by @ThetaSinner in [#194](https://github.com/holochain/kitsune2/pull/194)

## [0.1.5] - 2025-04-17

### Changed

- Bump tx5 and k2 version (#191) by @neonphog in [#191](https://github.com/holochain/kitsune2/pull/191)

## [0.1.4] - 2025-04-17

### Changed

- Expose current URL instead of is_local_agent (#189) by @ThetaSinner in [#189](https://github.com/holochain/kitsune2/pull/189)

## [0.1.3] - 2025-04-16

### Changed

- Tools for filtering local agents (#187) by @ThetaSinner in [#187](https://github.com/holochain/kitsune2/pull/187)

### Fixed

- Fix double sleep (#186) by @ThetaSinner in [#186](https://github.com/holochain/kitsune2/pull/186)

## [0.1.2] - 2025-04-14

### Changed

- Initiate faster after failure (#180) by @ThetaSinner in [#180](https://github.com/holochain/kitsune2/pull/180)
- Showcase Fixes (#179) by @neonphog in [#179](https://github.com/holochain/kitsune2/pull/179)
- Resolve conflicting initiates (#182) by @ThetaSinner in [#182](https://github.com/holochain/kitsune2/pull/182)
- Filter out requests for ops that are already in the store by @jost-s in [#178](https://github.com/holochain/kitsune2/pull/178)
- Broadcast agent info when declaring a new storage arc (#176) by @ThetaSinner in [#176](https://github.com/holochain/kitsune2/pull/176)
- Initiate first gossip round faster (#171) by @ThetaSinner in [#171](https://github.com/holochain/kitsune2/pull/171)
- Validation error if config disallows plain text, but plain text sbd used (#169) by @neonphog in [#169](https://github.com/holochain/kitsune2/pull/169)

### Fixed

- Fix initiate_interval_ms description by @jost-s in [#170](https://github.com/holochain/kitsune2/pull/170)
- Add historical sync test for gossip (#168) by @ThetaSinner in [#168](https://github.com/holochain/kitsune2/pull/168)

## [0.1.1] - 2025-04-03

### Changed

- Process_incoming_ops in MemOpStore returns all op ids by @jost-s in [#167](https://github.com/holochain/kitsune2/pull/167)
- Unit tests for terminate (#165) by @ThetaSinner in [#165](https://github.com/holochain/kitsune2/pull/165)
- Update SBD and tx5 (#166) by @ThetaSinner in [#166](https://github.com/holochain/kitsune2/pull/166)

## [0.1.0] - 2025-04-02

### Added

- Add docker container (#134) by @ThetaSinner in [#134](https://github.com/holochain/kitsune2/pull/134)
- Add systems x86_64-darwin and aarch64-linux by @jost-s in [#137](https://github.com/holochain/kitsune2/pull/137)
- Add bootstrap client (#129) by @ThetaSinner in [#129](https://github.com/holochain/kitsune2/pull/129)
- Add agent publish (#124) by @matthme in [#124](https://github.com/holochain/kitsune2/pull/124)
- Add integration test by @jost-s in [#100](https://github.com/holochain/kitsune2/pull/100)
- Add publish module (#119) by @matthme in [#119](https://github.com/holochain/kitsune2/pull/119)
- Add more peer meta for gossip (#114) by @ThetaSinner in [#114](https://github.com/holochain/kitsune2/pull/114)
- Add links to module crates by @jost-s in [#115](https://github.com/holochain/kitsune2/pull/115)
- Add job to nix build bootstrap-srv by @jost-s in [#116](https://github.com/holochain/kitsune2/pull/116)
- Add flake with bootstrap-srv package by @jost-s
- Add top-level crate with prod builder by @jost-s in [#85](https://github.com/holochain/kitsune2/pull/85)
- Add test bootstrap server by @jost-s
- Add gossip, fetch, op store and peer meta store to space by @jost-s
- Add dht update task (#90) by @ThetaSinner in [#90](https://github.com/holochain/kitsune2/pull/90)
- Add gossip tasks (#80) by @ThetaSinner in [#80](https://github.com/holochain/kitsune2/pull/80)
- Add macro for repeated checks until timeout by @jost-s
- Add fetch integration tests by @jost-s in [#78](https://github.com/holochain/kitsune2/pull/78)
- Add id utilities by @jost-s
- Add test space id by @jost-s in [#64](https://github.com/holochain/kitsune2/pull/64)
- Add module handler for incoming requests/responses (#70) by @jost-s in [#70](https://github.com/holochain/kitsune2/pull/70)
- Add peer meta store (#69) by @ThetaSinner in [#69](https://github.com/holochain/kitsune2/pull/69)
- Add automocks to transport for testing by @jost-s
- Add protobuf definition for ops by @jost-s
- Add fetch module (#35) by @jost-s in [#35](https://github.com/holochain/kitsune2/pull/35)
- Add Taplo (#49) by @ThetaSinner in [#49](https://github.com/holochain/kitsune2/pull/49)
- Add test utils with tracing helper (#46) by @ThetaSinner in [#46](https://github.com/holochain/kitsune2/pull/46)
- Add basic pr checks (#2) by @ThetaSinner in [#2](https://github.com/holochain/kitsune2/pull/2)
- Add skeleton api crate (#1) by @neonphog in [#1](https://github.com/holochain/kitsune2/pull/1)

### Changed

- Remove `get_local_agents` from the space API (#162) by @ThetaSinner in [#162](https://github.com/holochain/kitsune2/pull/162)
- Add jitter to gossip initiation (#161) by @ThetaSinner in [#161](https://github.com/holochain/kitsune2/pull/161)
- Make network stats typed (#154) by @ThetaSinner in [#154](https://github.com/holochain/kitsune2/pull/154)
- Expose the transport from the `Kitsune` trait (#152) by @ThetaSinner in [#152](https://github.com/holochain/kitsune2/pull/152)
- Add state dumps for fetch and gossip modules (#150) by @ThetaSinner in [#150](https://github.com/holochain/kitsune2/pull/150)
- Expose webrtc config (#151) by @ThetaSinner in [#151](https://github.com/holochain/kitsune2/pull/151)
- Add `dump_network_stats` to the transport (#149) by @ThetaSinner in [#149](https://github.com/holochain/kitsune2/pull/149)
- Expose DHT docs that get made private (#148) by @ThetaSinner in [#148](https://github.com/holochain/kitsune2/pull/148)
- Gossip improvements (#147) by @ThetaSinner in [#147](https://github.com/holochain/kitsune2/pull/147)
- DhtArc::len -> DhtArc::arc_span (#146) by @neonphog in [#146](https://github.com/holochain/kitsune2/pull/146)
- Bump cachix/cachix-action from 15 to 16 (#145) by @dependabot[bot] in [#145](https://github.com/holochain/kitsune2/pull/145)
- Avoid computing hashes for empty slices (#144) by @ThetaSinner in [#144](https://github.com/holochain/kitsune2/pull/144)
- Expose publish module (#143) by @neonphog in [#143](https://github.com/holochain/kitsune2/pull/143)
- Bump cachix/install-nix-action from 30 to 31 (#140) by @dependabot[bot] in [#140](https://github.com/holochain/kitsune2/pull/140)
- Space accessors (#142) by @neonphog in [#142](https://github.com/holochain/kitsune2/pull/142)
- If peer-meta is per space, pass space in factory (#141) by @neonphog in [#141](https://github.com/holochain/kitsune2/pull/141)
- Fix docs links (#138) by @ThetaSinner in [#138](https://github.com/holochain/kitsune2/pull/138)
- Reload TLS certificates (#139) by @ThetaSinner in [#139](https://github.com/holochain/kitsune2/pull/139)
- Cargo.lock by @neonphog
- Separate call to register kitsune handler (#135) by @neonphog in [#135](https://github.com/holochain/kitsune2/pull/135)
- Support SBD in bootstrap (#132) by @ThetaSinner in [#132](https://github.com/holochain/kitsune2/pull/132)
- Kitsune2_core docs point to the wrong crate (#133) by @neonphog in [#133](https://github.com/holochain/kitsune2/pull/133)
- Fix versioned crate reference and bump by @ThetaSinner
- Add bootstrap server missing CLI args (#128) by @ThetaSinner in [#128](https://github.com/holochain/kitsune2/pull/128)
- Downgrade to Rust edition 2021 (#127) by @ThetaSinner in [#127](https://github.com/holochain/kitsune2/pull/127)
- Bump Rust (#126) by @ThetaSinner in [#126](https://github.com/holochain/kitsune2/pull/126)
- Simplify space notify (#125) by @neonphog in [#125](https://github.com/holochain/kitsune2/pull/125)
- Add TLS support (#123) by @ThetaSinner in [#123](https://github.com/holochain/kitsune2/pull/123)
- Make publish-all by @neonphog
- Add READMEs (#122) by @ThetaSinner in [#122](https://github.com/holochain/kitsune2/pull/122)
- Showcase Chat (#120) by @neonphog in [#120](https://github.com/holochain/kitsune2/pull/120)
- Bump to tx5 v0.3.0-beta to fix buffer overrun (#121) by @neonphog in [#121](https://github.com/holochain/kitsune2/pull/121)
- More initiate tests (#117) by @ThetaSinner in [#117](https://github.com/holochain/kitsune2/pull/117)
- Use vcpkg on windows for libsodium dependency (#118) by @neonphog in [#118](https://github.com/holochain/kitsune2/pull/118)
- Publish crates (#109) by @ThetaSinner in [#109](https://github.com/holochain/kitsune2/pull/109)
- Clean up bookmark handling (#108) by @ThetaSinner in [#108](https://github.com/holochain/kitsune2/pull/108)
- Peer meta store no space (#112) by @ThetaSinner in [#112](https://github.com/holochain/kitsune2/pull/112)
- Pick serde version that is more widely compatible (#111) by @ThetaSinner in [#111](https://github.com/holochain/kitsune2/pull/111)
- Update storage arcs (#99) by @ThetaSinner in [#99](https://github.com/holochain/kitsune2/pull/99)
- Bump tx5 to fix broken pipe bug (#105) by @neonphog in [#105](https://github.com/holochain/kitsune2/pull/105)
- Prepare manifests for publishing (#106) by @ThetaSinner in [#106](https://github.com/holochain/kitsune2/pull/106)
- Tidy imports (#104) by @ThetaSinner in [#104](https://github.com/holochain/kitsune2/pull/104)
- Remove complete or no longer relevant TODOs (#103) by @ThetaSinner in [#103](https://github.com/holochain/kitsune2/pull/103)
- Limit op data at sector granularity (#101) by @ThetaSinner in [#101](https://github.com/holochain/kitsune2/pull/101)
- Skeleton crate for kitsune2 showcase app (#98) by @neonphog in [#98](https://github.com/holochain/kitsune2/pull/98)
- Op data limit tests (#96) by @ThetaSinner in [#96](https://github.com/holochain/kitsune2/pull/96)
- Extend space by gossip, fetch and op store by @jost-s
- Use tx5 dep from workspace by @jost-s
- Run test workflow on all branches by @jost-s
- Limit data sent after dht differences (#94) by @ThetaSinner in [#94](https://github.com/holochain/kitsune2/pull/94)
- New ops respect common arc set (#92) by @ThetaSinner in [#92](https://github.com/holochain/kitsune2/pull/92)
- Limit the number of accepted rounds in progress concurrently (#91) by @ThetaSinner in [#91](https://github.com/holochain/kitsune2/pull/91)
- Split up gossip file (#89) by @ThetaSinner in [#89](https://github.com/holochain/kitsune2/pull/89)
- Integrate dht model (#82) by @ThetaSinner in [#82](https://github.com/holochain/kitsune2/pull/82)
- Remove `ci_pass` job name (#87) by @ThetaSinner in [#87](https://github.com/holochain/kitsune2/pull/87)
- Config Validation (#84) by @neonphog in [#84](https://github.com/holochain/kitsune2/pull/84)
- Tx5Transport Module (#75) by @neonphog in [#75](https://github.com/holochain/kitsune2/pull/75)
- Split local agent store out of the space (#77) by @ThetaSinner in [#77](https://github.com/holochain/kitsune2/pull/77)
- Integrate fetch with gossip (#76) by @ThetaSinner in [#76](https://github.com/holochain/kitsune2/pull/76)
- Go go gossip (#72) by @ThetaSinner in [#72](https://github.com/holochain/kitsune2/pull/72)
- Process incoming responses to fetch requests by @jost-s
- Rename Request -> FetchRequest & Response -> FetchResponse by @jost-s
- Delay fetch request retries by @jost-s in [#73](https://github.com/holochain/kitsune2/pull/73)
- Op store split metadata (#74) by @ThetaSinner in [#74](https://github.com/holochain/kitsune2/pull/74)
- Gossip init (#68) by @ThetaSinner in [#68](https://github.com/holochain/kitsune2/pull/68)
- Pass ops as bytes to op store by @jost-s in [#71](https://github.com/holochain/kitsune2/pull/71)
- Build op store definition by @jost-s in [#63](https://github.com/holochain/kitsune2/pull/63)
- Implement fetch response queue by @jost-s
- Move the in-memory op store to the core crate (#67) by @ThetaSinner in [#67](https://github.com/holochain/kitsune2/pull/67)
- DHT module docs (#51) by @ThetaSinner in [#51](https://github.com/holochain/kitsune2/pull/51)
- Bootstrap and Re-Sign Agent Infos Before they Expire (#61) by @neonphog in [#61](https://github.com/holochain/kitsune2/pull/61)
- Signing Agent Infos (#55) by @neonphog in [#55](https://github.com/holochain/kitsune2/pull/55)
- Dht diff (#59) by @ThetaSinner in [#59](https://github.com/holochain/kitsune2/pull/59)
- Improve back off feature for unresponsive agents (#60) by @jost-s in [#60](https://github.com/holochain/kitsune2/pull/60)
- Transport Integration - Space Notify Send & Receive (#54) by @neonphog in [#54](https://github.com/holochain/kitsune2/pull/54)
- Bootstrap Client Module (#50) by @neonphog in [#50](https://github.com/holochain/kitsune2/pull/50)
- Config V2 (#58) by @neonphog in [#58](https://github.com/holochain/kitsune2/pull/58)
- Consider persisting tombstones (#57) by @maackle in [#57](https://github.com/holochain/kitsune2/pull/57)
- Tiny_http -> axum (#52) by @neonphog in [#52](https://github.com/holochain/kitsune2/pull/52)
- Transport trait (#53) by @neonphog in [#53](https://github.com/holochain/kitsune2/pull/53)
- Dht hash partitioning (#47) by @ThetaSinner in [#47](https://github.com/holochain/kitsune2/pull/47)
- Transport API and StubTransport Test Implementation (#45) by @neonphog in [#45](https://github.com/holochain/kitsune2/pull/45)
- BasicArc convert to type (#48) by @ThetaSinner in [#48](https://github.com/holochain/kitsune2/pull/48)
- Need a Real Url Type (#44) by @neonphog in [#44](https://github.com/holochain/kitsune2/pull/44)
- First-iteration top-level kitsune module (#40) by @neonphog in [#40](https://github.com/holochain/kitsune2/pull/40)
- Kitsune2 Top-Level P2P Protocol (#38) by @neonphog in [#38](https://github.com/holochain/kitsune2/pull/38)
- Two way op flow (#41) by @ThetaSinner in [#41](https://github.com/holochain/kitsune2/pull/41)
- Eliminate the origin time (#37) by @ThetaSinner in [#37](https://github.com/holochain/kitsune2/pull/37)
- Switch to core crate (#39) by @neonphog in [#39](https://github.com/holochain/kitsune2/pull/39)
- Space Module Stubs (#33) by @neonphog in [#33](https://github.com/holochain/kitsune2/pull/33)
- PeerStore API and MemPeerStore Implementation (#19) by @neonphog in [#19](https://github.com/holochain/kitsune2/pull/19)
- Time slice (#20) by @ThetaSinner in [#20](https://github.com/holochain/kitsune2/pull/20)
- Kitsune2 Bootstrap Server--Testing (#11) by @neonphog in [#11](https://github.com/holochain/kitsune2/pull/11)
- Simplified mvp module factory config pattern (#12) by @neonphog in [#12](https://github.com/holochain/kitsune2/pull/12)
- Kitsune2 Bootstrap Server--Core (#10) by @neonphog in [#10](https://github.com/holochain/kitsune2/pull/10)
- Use K2Error in existing k2 code (#18) by @neonphog in [#18](https://github.com/holochain/kitsune2/pull/18)
- K2Error Type (#8) by @neonphog in [#8](https://github.com/holochain/kitsune2/pull/8)
- Kitsune2 Bootstrap Server--Preparation (#9) by @neonphog in [#9](https://github.com/holochain/kitsune2/pull/9)
- Create the bootstrap spec (#6) by @neonphog in [#6](https://github.com/holochain/kitsune2/pull/6)
- Agent types (#5) by @neonphog in [#5](https://github.com/holochain/kitsune2/pull/5)
- Serde + timestamp type (#4) by @neonphog in [#4](https://github.com/holochain/kitsune2/pull/4)
- Simplified hash types for kitsune2 (#3) by @neonphog in [#3](https://github.com/holochain/kitsune2/pull/3)
- Kitsune2 by @neonphog

### Fixed

- Bootstrap bad URLs (#158) by @ThetaSinner in [#158](https://github.com/holochain/kitsune2/pull/158)
- Do less work when the byte limit is exhausted after finding a DHT diff (#102) by @ThetaSinner in [#102](https://github.com/holochain/kitsune2/pull/102)
- Limit the value we'll accept as a max op bytes… (#97) by @ThetaSinner in [#97](https://github.com/holochain/kitsune2/pull/97)
- Use space's fetch and op store in tests by @jost-s
- Fix test requests_are_dropped_when_max_back_off_expired in core_fetch by @jost-s in [#88](https://github.com/holochain/kitsune2/pull/88)
- Increase timeouts in fetch tests to prevent flakiness by @jost-s in [#81](https://github.com/holochain/kitsune2/pull/81)

## First-time Contributors

* @ThetaSinner made their first contribution in [#164](https://github.com/holochain/kitsune2/pull/164)

* @neonphog made their first contribution

* @dependabot[bot] made their first contribution in [#145](https://github.com/holochain/kitsune2/pull/145)

* @jost-s made their first contribution in [#137](https://github.com/holochain/kitsune2/pull/137)

* @matthme made their first contribution in [#124](https://github.com/holochain/kitsune2/pull/124)

* @maackle made their first contribution in [#57](https://github.com/holochain/kitsune2/pull/57)

[0.2.8]: https://github.com/holochain/kitsune2/compare/v0.2.7..v0.2.8
[0.2.7]: https://github.com/holochain/kitsune2/compare/v0.2.6..v0.2.7
[0.2.6]: https://github.com/holochain/kitsune2/compare/v0.2.5..v0.2.6
[0.2.5]: https://github.com/holochain/kitsune2/compare/v0.2.4..v0.2.5
[0.2.4]: https://github.com/holochain/kitsune2/compare/v0.2.3..v0.2.4
[0.2.3]: https://github.com/holochain/kitsune2/compare/v0.2.2..v0.2.3
[0.2.2]: https://github.com/holochain/kitsune2/compare/v0.2.1..v0.2.2
[0.2.1]: https://github.com/holochain/kitsune2/compare/v0.2.0..v0.2.1
[0.2.0]: https://github.com/holochain/kitsune2/compare/v0.1.5..v0.2.0
[0.1.5]: https://github.com/holochain/kitsune2/compare/v0.1.4..v0.1.5
[0.1.4]: https://github.com/holochain/kitsune2/compare/v0.1.3..v0.1.4
[0.1.3]: https://github.com/holochain/kitsune2/compare/v0.1.2..v0.1.3
[0.1.2]: https://github.com/holochain/kitsune2/compare/v0.1.1..v0.1.2
[0.1.1]: https://github.com/holochain/kitsune2/compare/v0.1.0..v0.1.1

<!-- generated by git-cliff -->
