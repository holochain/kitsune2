# Kitsune2 Security Assessment Report

**Date:** 2025-12-11
**Version:** v0.4.0-dev.0
**Assessment Type:** Remote Attack Surface Analysis
**Focus:** Network-exploitable vulnerabilities in P2P/DHT networking system

---

## Executive Summary

This security assessment examines the Kitsune2 P2P networking library with a focus on remotely exploitable vulnerabilities. Kitsune2 is a networking framework for Holochain that uses QUIC (via Iroh) and WebRTC (via Tx5) transports with Protocol Buffers serialization.

The assessment identified **2 critical**, **5 high**, **5 medium**, and **4 low** severity security issues. The most concerning findings relate to unbounded resource consumption, lack of size validation before deserialization, and potential for targeted resource exhaustion attacks.

### Critical Findings Summary
1. **Unbounded peer store growth** - No limits on agent info storage
2. **Missing protobuf size validation** - Deserialization without pre-checks enables memory exhaustion

---

## Assessment Scope

This assessment examines:
- Network message handling and deserialization
- Resource limits and bounds checking
- Storage systems (peer store, op store, metadata)
- Gossip protocol state machines
- Fetch/publish modules
- Transport layer security (Iroh and Tx5)
- Bootstrap server attack surface

---

## Findings by Severity

## CRITICAL SEVERITY

### CRIT-1: Unbounded Peer Store Growth
**Severity:** Critical
**Attack Vector:** Remote
**Component:** `crates/core/src/factories/mem_peer_store.rs`

**Description:**
The peer store has no limits on the number of agent infos it will accept and store. While expired entries are pruned periodically (every 10 seconds by default), an attacker can continuously send agent infos with far-future expiration timestamps.

**Location:** `mem_peer_store.rs:260-306`

**Attack Scenario:**
```rust
// Attacker generates many agent infos with expiry far in the future
// Each one is unique and signed, so passes validation
for i in 0..1_000_000 {
    let malicious_agent_info = create_agent_info(
        expires_at: Timestamp::now() + Duration::from_days(365)
    );
    // Send via gossip Agents message
}
```

**Impact:**
- Memory exhaustion on target node
- DoS through OOM crash
- Degraded performance as peer store operations slow down

**Evidence:**
```rust
// mem_peer_store.rs:301
self.store.insert(agent.agent.clone(), agent.clone());
inserted.push(agent);
// No check on self.store.len() before insertion
```

**Recommended Fix:**
- Add configurable maximum peer store size (e.g., 10,000 agents)
- Implement LRU eviction when limit is reached
- Consider per-URL agent limits to prevent a single peer from filling the store
- Add metrics/logging when approaching limits

**Priority:** **IMMEDIATE** - This can cause node crashes

---

### CRIT-2: Protobuf Deserialization Without Size Pre-checks
**Severity:** Critical
**Attack Vector:** Remote
**Component:** `crates/api/src/protocol.rs:13`

**Description:**
The primary message deserialization path uses `prost::Message::decode()` without checking message size before attempting to decode. While Iroh transport has a 1MB frame limit, this is hardcoded and not configurable. A malicious peer can send maximum-sized messages repeatedly.

**Location:**
- `protocol.rs:13-17`
- `transport.rs:109`

**Attack Scenario:**
```rust
// Attacker sends maximum-sized protobuf messages
for _ in 0..1000 {
    let large_message = create_max_size_protobuf(1_048_576); // 1MB
    send_to_victim(large_message);
    // Each decode allocates ~1MB, can exhaust memory/CPU
}
```

**Impact:**
- CPU exhaustion from repeated deserialization
- Memory spikes
- Potential for carefully crafted protobufs to exploit prost vulnerabilities

**Evidence:**
```rust
// protocol.rs:13-17
pub fn decode(bytes: &[u8]) -> K2Result<Self> {
    prost::Message::decode(std::io::Cursor::new(bytes)).map_err(|err| {
        K2Error::other_src("Failed to decode K2Proto Message", err)
    })
}
// No size check on `bytes` before decode

// transport_iroh/src/lib.rs:44
const MAX_FRAME_BYTES: usize = 1024 * 1024;
// TODO comment says "make configurable" but it's not
```

**Recommended Fix:**
- Add configurable maximum message size with reasonable defaults (e.g., 256KB)
- Check message size before attempting deserialization
- Add rate limiting on large messages from individual peers
- Make MAX_FRAME_BYTES configurable per use case

**Priority:** **HIGH** - Can cause resource exhaustion

---

## HIGH SEVERITY

### HIGH-1: No Bounds on Op Store Size
**Severity:** High
**Attack Vector:** Remote
**Component:** `crates/core/src/factories/mem_op_store.rs`

**Description:**
The op store has no limits on the number or total size of ops it will store. An attacker can send unlimited ops through gossip or publish protocols.

**Location:** `mem_op_store.rs:198-202`

**Attack Scenario:**
```rust
// Attacker sends many unique ops
for i in 0..1_000_000 {
    let op = create_unique_op(i);
    // Gossip or publish this op
    // process_incoming_ops will store without limits
}
```

**Impact:**
- Memory exhaustion
- Disk exhaustion (if persistence layer is added)
- Performance degradation

**Evidence:**
```rust
// mem_op_store.rs:198-202
for (op_id, record) in ops_to_add {
    lock.op_list.entry(op_id.clone()).or_insert(record);
    op_ids.push(op_id);
}
// No check on lock.op_list.len() or total memory usage
```

**Recommended Fix:**
- Add configurable maximum op count
- Add configurable maximum total op data size
- Implement LRU or age-based eviction
- Consider op data deduplication

**Priority:** **HIGH**

---

### HIGH-2: Unbounded Fetch Request Queue
**Severity:** High
**Attack Vector:** Remote
**Component:** `crates/core/src/factories/core_fetch.rs`

**Description:**
The fetch module uses bounded channels (16,384 items) for request queues, but when the queue is full, requests are silently dropped rather than rejecting connections or applying backpressure. An attacker can flood the queue causing legitimate requests to be dropped.

**Location:**
- `core_fetch.rs:233-240`
- `message_handler.rs:42-56`

**Attack Scenario:**
```rust
// Attacker floods with fetch requests
for _ in 0..20_000 {
    send_fetch_request(random_op_ids());
}
// Queue fills (16,384 limit)
// New requests are dropped, including legitimate ones
```

**Impact:**
- DoS by preventing legitimate fetch operations
- Silent failures make debugging difficult
- No feedback to legitimate peers

**Evidence:**
```rust
// message_handler.rs:42-56
if let Err(err) = self.incoming_request_tx.try_send(...) {
    match err {
        tokio::sync::mpsc::error::TrySendError::Full(_) => {
            tracing::info!(?err, "could not insert incoming request into queue, dropping it");
            // Just logs and drops, doesn't close connection
        }
        // ...
    }
}
```

**Recommended Fix:**
- Implement connection-level backpressure
- Close connections from peers repeatedly filling queues
- Add per-peer request rate limiting
- Consider separate queues per peer to prevent one peer from crowding out others

**Priority:** **HIGH**

---

### HIGH-3: Missing Validation on Gossip Message Counts
**Severity:** High
**Attack Vector:** Remote
**Component:** `crates/gossip/src/protocol.rs`

**Description:**
Gossip messages can contain vectors of agent IDs, op IDs, and hashes without bounds checking. An attacker can send messages with extremely large vectors causing memory exhaustion during deserialization.

**Location:** Various message types in `protocol.rs`

**Attack Scenario:**
```rust
// Attacker sends gossip message with huge vector
let malicious_msg = K2GossipHashesMessage {
    session_id: valid_session_id,
    hashes: vec![random_hash(); 10_000_000], // 10M hashes
};
```

**Impact:**
- Memory exhaustion
- CPU exhaustion from processing
- Potential protobuf deserialization vulnerabilities

**Evidence:**
```rust
// Protocol definitions allow unbounded repeated fields
// No validation on vector sizes before processing
```

**Recommended Fix:**
- Add maximum element count for all vector fields
- Validate counts during deserialization
- Add size limits to protobuf schema with validation

**Priority:** **HIGH**

---

### HIGH-4: Bootstrap Server Body Size Limit Too Small
**Severity:** High (for legitimate use) / Medium (for attack surface)
**Attack Vector:** Remote
**Component:** `crates/bootstrap_srv/src/http.rs:235`

**Description:**
The bootstrap server has a hardcoded 1KB body size limit which is very restrictive for legitimate agent info but does prevent large payloads. However, there's no rate limiting on failed requests, allowing rapid-fire attacks.

**Location:** `http.rs:235`

**Evidence:**
```rust
// http.rs:235
.layer(extract::DefaultBodyLimit::max(1024))
```

**Impact:**
- Legitimate agent infos may be rejected if signed data exceeds 1KB
- No rate limiting on 413 responses

**Recommended Fix:**
- Increase limit to reasonable value (e.g., 4-8KB) for signed agent infos
- Add rate limiting on failed requests
- Consider different limits for different endpoints

**Priority:** **MEDIUM** (attack surface is limited by existing mitigations)

---

### HIGH-5: Lack of Gossip Session ID Validation
**Severity:** High
**Attack Vector:** Remote
**Component:** `crates/gossip/src/respond/*.rs`

**Description:**
While session IDs are used to track gossip rounds, there's insufficient validation to prevent session confusion attacks where an attacker sends messages from one session context in another.

**Impact:**
- State machine confusion
- Potential to cause rounds to fail or behave incorrectly
- Memory leaks if session state is not properly cleaned up

**Recommended Fix:**
- Add cryptographic binding between session ID and peer URL
- Validate session ID belongs to expected peer
- Add timeout tracking per session

**Priority:** **HIGH**

---

## MEDIUM SEVERITY

### MED-1: Hardcoded Frame Size Limit in Iroh Transport
**Severity:** Medium
**Attack Vector:** Remote
**Component:** `crates/transport_iroh/src/lib.rs:44`

**Description:**
The Iroh transport has a hardcoded 1MB frame size limit with a TODO comment to make it configurable. Different use cases may need different limits.

**Location:** `transport_iroh/src/lib.rs:44`

**Evidence:**
```rust
// transport_iroh/src/lib.rs:44
// TODO: make configurable
const MAX_FRAME_BYTES: usize = 1024 * 1024;
```

**Recommended Fix:**
- Make MAX_FRAME_BYTES configurable via `IrohTransportConfig`
- Add validation that configured value is reasonable (e.g., 64KB - 10MB)
- Document security implications of different values

**Priority:** **MEDIUM**

---

### MED-2: Timestamp Manipulation in Peer Meta Store
**Severity:** Medium
**Attack Vector:** Remote (indirect)
**Component:** `crates/core/src/factories/mem_peer_meta_store.rs`

**Description:**
Unresponsive peer tracking relies on timestamps without bounds checking. While timestamps come from local time, there's no validation that expiry times are reasonable.

**Impact:**
- Potential for incorrect peer reputation if system clock is manipulated
- No protection against far-future timestamps

**Recommended Fix:**
- Add maximum allowed expiry duration
- Validate timestamp sanity

**Priority:** **MEDIUM**

---

### MED-3: Complex Gossip State Machine Vulnerability to Edge Cases
**Severity:** Medium
**Attack Vector:** Remote
**Component:** `crates/gossip/src/respond/*.rs`

**Description:**
The gossip protocol has a complex state machine with multiple message types and transitions. While there's error handling, the complexity creates risk of:
- State transitions that should be impossible
- Race conditions in concurrent rounds
- Edge cases in snapshot comparison

**Location:** Multiple files in `gossip/src/respond/`

**Impact:**
- Potential crashes from unexpected state transitions
- Memory leaks from incomplete state cleanup
- Protocol deadlocks

**Recommended Fix:**
- Add comprehensive fuzz testing of gossip protocol
- Add state invariant assertions
- Simplify state machine where possible
- Add metrics to detect stuck rounds

**Priority:** **MEDIUM**

---

### MED-4: DHT Hash Collision Handling Not Documented
**Severity:** Medium
**Attack Vector:** Local (same DHT space)
**Component:** `crates/dht/src/hash.rs`

**Description:**
The DHT uses XOR to combine hashes without explicit collision detection. While cryptographic hashes make intentional collisions hard, the behavior when collisions occur is not documented.

**Impact:**
- Undefined behavior on hash collisions
- Potential for data loss or sync failures

**Recommended Fix:**
- Document hash collision handling strategy
- Add tests for collision scenarios
- Consider using a hash algorithm designed for DHT use

**Priority:** **MEDIUM**

---

### MED-5: Missing Rate Limiting on Preflight Connections
**Severity:** Medium
**Attack Vector:** Remote
**Component:** `crates/api/src/transport.rs:66-88`

**Description:**
The `peer_connect` function gathers preflight data without rate limiting. An attacker can open many connections forcing preflight processing.

**Location:** `transport.rs:66-88`

**Impact:**
- CPU exhaustion from preflight processing
- Connection table exhaustion

**Recommended Fix:**
- Add connection rate limiting per peer
- Add maximum concurrent connections
- Implement SYN-cookie-like mechanism for connection validation

**Priority:** **MEDIUM**

---

## LOW SEVERITY

### LOW-1: Agent Info URL Format Not Validated
**Severity:** Low
**Attack Vector:** Remote
**Component:** `crates/api/src/agent.rs`

**Description:**
Agent info URL fields are not validated for format correctness beyond basic parsing.

**Recommended Fix:**
- Add URL format validation (scheme, host, port ranges)
- Reject obviously invalid URLs

**Priority:** **LOW**

---

### LOW-2: Disconnect Message String Unbounded
**Severity:** Low
**Attack Vector:** Remote
**Component:** `crates/api/src/transport.rs:181`

**Description:**
Disconnect messages can contain arbitrary-length strings. While connections are closing anyway, very large disconnect reasons could cause issues.

**Location:** `transport.rs:181`

**Evidence:**
```rust
// transport.rs:181
let reason = String::from_utf8_lossy(&data).to_string();
```

**Recommended Fix:**
- Truncate disconnect reason to reasonable length (e.g., 256 bytes)

**Priority:** **LOW**

---

### LOW-3: Blocked Message Counters Can Overflow
**Severity:** Low
**Attack Vector:** Remote (long-running)
**Component:** `crates/api/src/transport.rs`

**Description:**
Blocked message counters use `u32` which could theoretically overflow on long-running nodes with heavily blocked peers.

**Location:** `transport.rs:815-822`

**Recommended Fix:**
- Use `u64` for counters or add overflow detection
- Reset counters periodically

**Priority:** **LOW**

---

### LOW-4: Missing Metrics for Attack Detection
**Severity:** Low (monitoring)
**Attack Vector:** N/A
**Component:** Various

**Description:**
The codebase lacks comprehensive metrics for detecting attack patterns:
- No metrics on message deserialization failures
- No metrics on queue fullness
- Limited visibility into peer behavior

**Recommended Fix:**
- Add metrics for:
  - Deserialization errors by peer
  - Queue fullness over time
  - Message sizes
  - Connection churn
- Add alerting on suspicious patterns

**Priority:** **LOW**

---

## Vulnerability Patterns Analysis

### 1. Crashes from Unexpected Network Payloads

**Finding:** The codebase generally handles malformed payloads well with error returns rather than panics.

**Good Examples:**
- Protobuf decode errors are caught and converted to `K2Result` errors
- Transport layer validates message types before routing
- Missing space IDs cause connection closure rather than crashes

**Concerns:**
- Deep protobuf nesting could cause stack overflow
- Some HashMap operations use `.expect("poisoned")` on mutex locks

**Note:** The in-memory op store (mem_op_store.rs) with JSON deserialization is only used for development/testing and is not exposed in production integrations. Production implementations provide their own persistent op store.

**Verdict:** **Good** - Production code handles errors appropriately

---

### 2. Handling of Oversized Payloads

**Finding:** Oversized payloads are inconsistently handled.

**Good:**
- Iroh transport enforces 1MB frame limit (transport_iroh/src/lib.rs:44, 553)
- Frame size is validated before decoding (frame.rs:69)
- Bootstrap server has 1KB body limit

**Problems:**
- No pre-check of message size before protobuf deserialization
- Hardcoded limits are not configurable
- No per-message-type size limits
- Gossip messages can contain unbounded vectors

**Verdict:** **NEEDS IMPROVEMENT** - Add size validation before deserialization

---

### 3. Unbounded Data Storage

**Finding:** Several unbounded storage issues identified.

**Problems Identified:**
1. **Peer Store** - No limit on agent infos (CRIT-1)
2. **Op Store** - No limit on ops (HIGH-1)
3. **Fetch Requests** - Bounded queue but no backpressure (HIGH-2)
4. **DHT Time Slices** - Unbounded HashMap for slice hashes

**Good:**
- **Gossip State** - Concurrent accepted rounds ARE limited (default 10, configurable via `max_concurrent_accepted_rounds`)

**Verdict:** **HIGH RISK** - Peer store and op store unbounded storage enable DoS

---

### 4. Resource Exhaustion via Hard Work

**Finding:** Several resource exhaustion vectors identified.

**Attack Vectors:**
1. **Fetch Flooding** - Request many non-existent op IDs forcing database lookups
2. **Large Message Spam** - Send maximum-sized messages repeatedly
3. **DHT Diff Computation** - Force complex snapshot comparisons

**Mitigations Present:**
- Bounded channels on fetch queues (16,384)
- **Gossip concurrent round limits** - Max 10 accepted rounds by default (configurable)
- **Gossip timeout task** - Cleans up stale rounds after 15s
- **Gossip burst limiting** - Rate limits gossip initiation per peer
- Blocking system can prevent messages from blocked agents
- Fetch requests are deduplicated

**Missing:**
- Per-peer request rate limits
- Computation cost tracking
- Connection-level backpressure
- Cost-based prioritization

**Verdict:** **HIGH RISK** - Needs rate limiting and cost tracking

---

### 5. Loops and Recursion Progress Failures

**Finding:** Examined loop constructs for potential infinite loops or stack overflow.

**Analysis:**
- Gossip state machine has timeout mechanisms
- DHT snapshot comparison has bounded recursion depth
- Peer store pruning has time-based guards against tight loops
- No obvious unbounded recursion

**Concerns:**
- Gossip protocol could theoretically loop forever if both parties keep sending snapshots that require more detail, but there's a finite number of snapshot detail levels
- Complex state machines could deadlock with malicious peer sequences

**Verdict:** **LOW RISK** - State machines are reasonably protected

---

## Positive Security Findings

The codebase demonstrates several good security practices:

1. **Signature Verification** - Agent infos are cryptographically signed and verified (agent.rs:218-271)

2. **Blocking System** - Comprehensive peer blocking system prevents malicious agents from participating (blocks.rs)

3. **Message Validation** - Wire protocol enforces space_id for scoped messages

4. **Preflight Validation** - Connections can be rejected before they're fully established

5. **Error Handling** - Generally uses `Result` types instead of panicking

6. **TLS Support** - Bootstrap server supports TLS

7. **Timeouts** - Gossip rounds timeout to prevent indefinite stalls (default 15s)

8. **Concurrent Gossip Round Limits** - Maximum concurrent accepted rounds is enforced (default 10, configurable via `max_concurrent_accepted_rounds`). When limit is reached, peers receive a "Busy" response (initiate.rs:29-36)

9. **Bounded Channels** - Most queues are bounded (though handling of fullness needs work)

10. **Expired Entry Pruning** - Peer store automatically removes expired entries

11. **IP Rate Limiting** - Bootstrap server has IP-based rate limiting (http.rs:190)

---

## Recommended Immediate Actions

**Priority 1 (Fix Immediately):**
1. Add maximum peer store size with LRU eviction (CRIT-1)
2. Add size validation before protobuf deserialization (CRIT-2)
3. Add op store size limits (HIGH-1)

**Priority 2 (Fix Soon):**
1. Implement connection-level backpressure for fetch (HIGH-2)
2. Add vector size limits to gossip messages (HIGH-3)
3. Make MAX_FRAME_BYTES configurable (MED-1)

**Priority 3 (Improvements):**
1. Add comprehensive fuzz testing for gossip protocol
2. Add metrics for attack detection
3. Implement per-peer rate limiting
4. Add cost-based request prioritization

---

## Configuration Recommendations

Recommended default limits for production deployments:

```toml
[security]
# Peer store limits
max_peer_store_agents = 10000
peer_store_per_url_limit = 100

# Op store limits
max_op_store_count = 1000000
max_op_store_bytes = 10737418240  # 10GB

# Gossip limits
max_concurrent_accepted_rounds = 100  # Already implemented, default is 10
max_gossip_message_vector_size = 10000
gossip_round_timeout_ms = 30000  # Already implemented, default is 15000

# Fetch limits
max_fetch_queue_size = 16384
max_fetch_requests_per_peer_per_second = 100

# Transport limits
max_frame_bytes = 1048576  # 1MB, make configurable
max_message_size = 262144   # 256KB

# Connection limits
max_connections_per_peer = 5
max_concurrent_connections = 1000
connection_rate_limit_per_peer = 10  # per second
```

---

## Testing Recommendations

1. **Fuzz Testing:**
   - Fuzz all protobuf deserialization paths
   - Fuzz gossip protocol state machines
   - Fuzz with maximum-sized messages

2. **Load Testing:**
   - Test with maximum peer store size
   - Test with maximum op store size
   - Test with many concurrent gossip rounds

3. **Adversarial Testing:**
   - Test with malicious peer sending only large messages
   - Test with peer initiating many gossip rounds
   - Test with peer flooding fetch requests
   - Test with deeply nested JSON in ops

4. **Integration Testing:**
   - Test behavior when all queues are full
   - Test behavior under memory pressure
   - Test cleanup of stale state

---

## Conclusion

Kitsune2 has a reasonably secure architecture with good use of cryptography and error handling. However, several critical issues around unbounded resource consumption need immediate attention. The primary risk is Denial of Service through memory exhaustion.

The most critical fixes are:
1. Adding bounds to all storage systems
2. Implementing proper resource limits
3. Adding rate limiting at multiple layers

Once these issues are addressed, Kitsune2 should be resilient to most remote attack vectors. The blocking system and signature verification provide a strong foundation for trust management.

**Overall Risk Level:** **HIGH** (before fixes) → **MEDIUM** (after critical fixes)

---

## Appendix: Attack Scenarios

### Scenario 1: Memory Exhaustion via Peer Store
```
1. Attacker generates 100,000 unique keypairs
2. For each keypair, create agent info with expires_at = now + 1 year
3. Sign each agent info properly
4. Send agent infos via gossip Agents messages
5. Victim stores all 100K agent infos (no limit)
6. Each agent info ~1KB = 100MB minimum
7. Repeat from multiple peer URLs to evade per-peer limits
Result: OOM crash
```

### Scenario 2: Gossip Round State Exhaustion (MITIGATED)
```
**NOTE: This attack is mitigated by max_concurrent_accepted_rounds (default 10)**

Theoretical attack if limit were disabled:
1. Attacker controls 1000 peer URLs (different IPs/relays)
2. From each URL, initiate gossip round
3. Don't complete rounds - let them timeout
4. Before timeout (15s), initiate more rounds
5. Gossip state HashMap grows to 1000+ entries
6. Each entry contains DHT snapshots, arc sets, etc.
7. Memory consumption: 1000 rounds * ~100KB = 100MB

Actual result with default config:
- Only 10 concurrent accepted rounds allowed
- 11th attacker receives "Busy" response
- Limited to ~1MB memory for gossip state
Status: MITIGATED by existing controls
```

### Scenario 3: Large Message Flood
```
1. Attacker sends 1MB protobuf messages repeatedly
2. Each message is valid but maximum size
3. 1000 messages/second = 1GB/s deserialization load
4. CPU pegged on protobuf decode
5. Memory spikes from allocation during decode
Result: DoS via resource exhaustion
```

---

**End of Report**
