# Kitsune2 Gossip

[![Project](https://img.shields.io/badge/project-kitsune2-purple.svg?style=flat-square)](https://github.com/holochain/kitsune2)
[![Crate](https://img.shields.io/crates/v/kitsune2_gossip.svg)](https://crates.io/crates/kitsune2_gossip)
[![API Docs](https://docs.rs/kitsune2_gossip/badge.svg)](https://docs.rs/kitsune2_gossip)
[![Discord](https://img.shields.io/badge/Discord-blue.svg?style=flat-square)](https://discord.gg/k55DS5dmPH)

The gossip protocol for the Kitsune2 project.

## The gossip message flows are described in the following diagram:

```mermaid
---
title: Gossip Protocol
---
stateDiagram-v2
    [*] --> init: Initiator sends agent ids, arc set and bookmark
    init --> accept: Acceptor sends init fields + missing agents, new op ids, new bookmark, and a snapshot
    
    accept --> no_diff: Initiator snapshot matches, send missing agents, new ops, new bookmark
    accept --> disc_sectors_diff: Initiator sends disc diff + missing agents, new ops, new bookmark
    accept --> ring_sector_details_diff: Initiator sends ring diff + missing agents, new ops, new bookmark
    accept --> terminate: Must stop
    
    no_diff --> agents: Acceptor send agents
    disc_sectors_diff --> agents: Acceptor cannot compare, so sends only agents
    ring_sector_details_diff --> agents: Acceptor cannot compare, so sends only agents 
    
    disc_sectors_diff --> terminate: Must stop
    ring_sector_details_diff --> terminate: Must stop
    
    disc_sectors_diff --> disc_sector_details_diff: Acceptor sends disc diff
    disc_sector_details_diff --> disc_sector_details_diff_response: Initiator sends disc diff + hashes
    disc_sector_details_diff_response --> hashes: Acceptor sends hashes
    
    disc_sectors_diff --> terminate: Must stop
    disc_sector_details_diff --> terminate: Must stop
    disc_sector_details_diff_response --> terminate: Must stop
    
    ring_sector_details_diff --> ring_sector_details_diff_response: Acceptor sends ring diff + hashes
    ring_sector_details_diff_response --> hashes: Initiator sends hashes
    
    ring_sector_details_diff --> terminate: Must stop
    ring_sector_details_diff_response --> terminate: Must stop
    
    hashes --> [*]
    agents --> [*]
    terminate --> [*]
```

## License
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Copyright (C) 2024 - 2025, Holochain Foundation

This program is free software: you can redistribute it and/or modify it under the terms of the license
provided in the LICENSE file (Apache 2.0).  This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
PURPOSE.
