# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.9] - 2025-06-20

### Added

- Add changelog for previous versions
- Add workflow that tags and publishes a release upon merge to main
- Add a manually-triggered workflow to prepare a release
- Add common functions to filter unresponsive URLs out of agent
- Add get/set to mark peer unresponsive in peer meta store

### Changed

- Match all common unresponsive filter function comments
- Filter unresponsive peers for gossip
- Backport faster initial sync 0.1 (#240)

### Fixed

- The sharding feature was using old function name

## [0.1.8] - 2025-05-01

### Changed

- Fix bad tx5-core version (#204)

## [0.1.7] - 2025-04-29

### Changed

- Moar tx5 config (#203)

## [0.1.6] - 2025-04-28

### Changed

- Backport #195 (#196)

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

[0.1.8]: https://github.com/holochain/kitsune2/compare/v0.1.7..v0.1.8
[0.1.7]: https://github.com/holochain/kitsune2/compare/v0.1.6..v0.1.7
[0.1.6]: https://github.com/holochain/kitsune2/compare/v0.1.5..v0.1.6
[0.1.5]: https://github.com/holochain/kitsune2/compare/v0.1.4..v0.1.5
[0.1.4]: https://github.com/holochain/kitsune2/compare/v0.1.3..v0.1.4
[0.1.3]: https://github.com/holochain/kitsune2/compare/v0.1.2..v0.1.3
[0.1.2]: https://github.com/holochain/kitsune2/compare/v0.1.1..v0.1.2
[0.1.1]: https://github.com/holochain/kitsune2/compare/v0.1.0..v0.1.1

<!-- generated by git-cliff -->
