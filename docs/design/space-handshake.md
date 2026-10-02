# Space handshake

**Status:** Accepted

## Problem

Kitsune2 nodes are multi-tenant. A single node can run many spaces at once, and
the transport keeps one connection per remote peer URL that all of those spaces
share.

Agent information is currently exchanged in the connection preflight. The
preflight runs exactly once, when the connection is established, and it is not
space-aware — it is produced by the host at the level of the whole node, with
no knowledge of which spaces the connection will end up carrying.

Blocking is enforced per space, by looking at the agents known to be at a peer
URL. A peer URL with no known agents in a space is treated as blocked for that
space, and its messages are dropped silently.

The loss is one-directional. The side that starts a conversation necessarily
already knows an agent at the peer in that space, because that is how it
learned the URL, so it is never the side that drops. It is the receiver that
discards what arrives.

Putting those together: the first space to reach a peer causes a connection to
be established and a preflight to be exchanged. Every space that starts after
that point reuses the same connection, so no further preflight happens, and the
peer never learns that space's agents. Traffic for those spaces is discarded on
arrival. Nothing reports an error; the sender sees its messages accepted and the
receiver never sees them.

The condition clears only if the receiving space happens to discover the
sender's agents some other way, in practice by polling the bootstrap server.
That can take minutes.

Sharing several spaces with the same peer is a supported case rather than an
unusual one, so this affects ordinary operation. The observable symptoms are
gossip rounds that are initiated and never answered, and peers that appear
reachable but exchange nothing.

**The underlying mismatch is that a connection-scoped, node-level mechanism is
being used to satisfy a per-space requirement.** Everything below follows from
moving that responsibility to where it belongs.

## Goals

- Give every space its own handshake with a remote peer, independent of when
  the underlying connection was established or how long it lives.
- Ensure that before a space sends anything to a peer, that peer knows at least
  one agent the space has, so that block enforcement has something to act on and
  does not fall back to dropping everything.
- Work correctly when a space starts after a connection already exists
- Recover when a space is torn down and re-created while the connection persists.
- Preserve block enforcement exactly as it is. The handshake makes enforcement
  work correctly across multiple spaces; it must not make it more permissive.
- Live in shared code, so that transport implementations need no knowledge of
  it.

## Non-goals

- Authentication or access control. The handshake shares agent information; it
  establishes nothing about whether the peer should be in the space. Deciding
  that is a separate concern, handled by a separate module.
- Connection management. How connections are established, how failures are
  handled, and how gossip chooses targets are all unchanged. In particular, the
  handshake does not address peers that are slow or impossible to connect to.

## Design

### Shape

The handshake is performed by a per-space module. Every space has one. Kitsune2
provides a default implementation that all nodes can use; a host may substitute
its own, but is not expected to have to.

It is tracked once between a space and a remote peer URL, not once per agent.
Kitsune2 identifies remote peers by URL throughout, and the block enforcement it
feeds is expressed per URL, so the handshake is tracked the same way.

There are two messages, both travelling within the space as module messages
under a reserved module identifier and encoded as protobuf so that they can be
extended later. A **hello** carries the sending space's signed agent infos. A
**response** carries nothing; it says only that the hello it answers arrived and
was applied.

A space sends a hello ahead of any message to a peer URL it has not confirmed.
It also sends one when a hello arrives from a URL it has not confirmed, so that
an exchange started by either side finishes without waiting for the second side
to have traffic of its own.

Receiving a hello always produces a response. If the receiver has not confirmed
the sender's URL either, it sends its own hello alongside that response, so the
ordinary exchange runs to three messages: a hello, a response carrying a hello
back, and a response to that. Where both sides start at once it costs a few
more, because each sends a hello the other had already answered. It terminates
either way, because a confirmed URL stops producing hellos.

The response carries no agent infos because the side receiving it does not need
them. A space can only address a peer URL it discovered, and discovering it
means holding an agent info that carries that URL, so it already knows an agent
at the peer it is talking to. What it lacks is confirmation, and confirmation is
all the response has to supply. Keeping it empty also leaves nothing on the
exempt path that has to be verified.

**A response must not be sent until the agents in the hello have been recorded
and whatever state block enforcement consults has been updated.** A response
sent ahead of that confirms something that has not happened, and the sender acts
on it by sending traffic that the receiver then drops. Applying a hello and
answering it are one step.

Agent information is additive, exactly as it is when it arrives from any other
source. A hello supplies agents; it never removes them, and it is not a snapshot
that supersedes what the receiver already knows. Superseding an existing record
follows the same rules that apply to agent information generally.

A hello carrying no agent infos cannot serve its purpose and is dropped, as is
one that is malformed, oversized, or otherwise not understood. A dropped hello
is not answered. A response that answers no outstanding hello is dropped.

### Exemption from block enforcement

Messages addressed to the handshake module are exempt from the rule that drops
traffic to and from a peer URL with no known agents in the space. Without this
the handshake could never run: it exists precisely to resolve that state.

The exemption is narrow, and two limits define it.

It applies only to the reserved module identifier used by the handshake.
Everything else addressed to that peer in that space is still dropped until the
handshake has supplied agents.

It does not extend to peers that are known to be blocked. If any agent known at
that URL is blocked in this space, hellos and responses are dropped along with
everything else. The exemption covers "we do not know who is there yet", never
"we know who is there and have refused them".

One path narrows as a result. Agent information can today arrive from a URL with
a blocked agent at it, because that information travels in the preflight and the
preflight is always allowed. Once it travels in the handshake instead it no
longer can, because the exemption stops at URLs known to be blocked and such a
URL receives nothing. Nothing changes in practice: one blocked agent at a URL
blocks the whole URL, so learning of another agent there would not have changed
the decision.

### Handshake state

Each space keeps, in memory, the set of peer URLs it has confirmed. A URL is
confirmed when a response arrives for a hello that this space sent to it and has
not yet had answered. Nothing else confirms a URL: not sending a hello, and not
receiving one.

The set exists to stop a space prefixing a hello to every message it ever sends.
It does not gate the traffic itself, which goes out either way.

Confirming on a successful send instead would mean very little. A successful
send tells the sender that the transport accepted the message, not that the peer
received it, so a hello can still be lost with the connection carrying it, or be
dropped on arrival. The response is the only thing that shows the agents landed.

While a URL is unconfirmed, the space sends a hello every time it has something
to send there, without tracking whether one is already in flight. Duplicates are
expected rather than merely tolerated, and each costs one small message.

**A repeat hello must be accepted, applied and answered normally, whether or not
the URL it came from is confirmed. It is never rejected.** This is the invariant
that removes the need to treat restarts as a special event. Whichever side lost
its state simply sends again, and the side that kept its state absorbs the
repeat harmlessly. Neither side needs to detect that the other restarted, and
there is only one code path to get right. It also makes concurrency a non-event:
two sends racing towards the same unconfirmed URL may each send a hello, and the
second is absorbed like any other repeat.

Receiving a hello does not confirm the sender's URL. The two directions are
independent: what a space records is that a peer has confirmed the agents it was
told about, never what a peer has told it. A space that has answered a hello
still needs a hello of its own answered before it can speak freely.

A URL is cleared from the set when the connection to it is lost, whether or not
the loss had anything to do with the handshake. Clearing on connection loss is
right for the same reason clearing on restart is: a peer reached over a new
connection may be a peer that restarted and lost everything it knew.

The state is not persisted. It belongs to the space rather than to the
connection — a space that is torn down and re-created starts with an empty set
even where the connection survived, which is correct, because it has also lost
the agents it learned — but it does not outlive a connection either, for the
reason above.

Note that the state cannot be derived from whether the space knows any agents at
the URL. A sender always knows some — that is how it found the peer — so a
derived check would never fire on the side that needs it. The state has to be
recorded explicitly.

### When the handshake runs

The handshake is lazy in one direction and reactive in the other. A space sends
a hello when it is about to send a message to a peer URL it has not confirmed,
and when a hello arrives from a URL it has not confirmed. Nothing is sent
speculatively, and no connection is opened for the sake of a handshake alone.
Handshake traffic therefore scales with how much a node actually communicates,
not with how many peers it has discovered.

Because sending is one of the two triggers, that check belongs at the point
where a space hands a message to the transport, rather than inside each module
that wants to send. Gossip, publish, fetch, agent info broadcast and host
notifications are then all covered without any of them knowing the handshake
exists.

If the hello cannot be sent, the triggering send fails with it. Otherwise the
triggering message follows immediately, without waiting for a response. The
handshake has no retry loop of its own; the callers that trigger it already
retry on their own schedules, and letting them pace it avoids a second,
independent source of traffic aimed at a peer that may be unreachable.

### Ordering

The triggering message is sent immediately after the hello, on the same
connection. The sender does not wait for the response, which governs whether
later sends need a hello of their own rather than whether this one goes out.

Two things have to hold for the triggering message to stand a chance of being
accepted.

**The sender must issue the hello and the triggering message in order, not
concurrently.** Concurrent sends may be interleaved on the connection, and the
triggering message arriving first would simply be dropped.

**The receiver must finish applying a hello before it processes the next message
from that peer.** A hello that is accepted and then applied in the background
reintroduces exactly the race the handshake exists to remove: the message behind
it is checked for permission before the agents have landed, and is dropped.

Neither helps if the transport reorders what it is handed, and that is not
stated here as a requirement on transports. A triggering message that overtakes
the hello ahead of it arrives while the receiver still knows nothing about the
sender's agents, and is dropped. That is no worse than a hello that never
arrives, and it recovers the same way: the URL stays unconfirmed, so the next
send carries another hello.

In practice it does not arise. Both transports Kitsune2 provides, the iroh
transport and the memory transport, deliver messages to a peer URL in the order
they were handed over, because each keeps one connection per peer URL and writes
to it as an ordered stream.

### Relationship to preflight

Preflight stops carrying agent information. That responsibility moves entirely
into the hello.

Preflight is still exchanged, and it is still exempt from block enforcement, but
for a different reason than it is today. Today it is exempt because it is the
only way to learn about new agents at a peer URL. Afterwards it is exempt
because it is not space-scoped: it is exchanged before any space traffic, so
there is no space in which a block could be evaluated.

What stays in preflight is connection-level concerns whose correct outcome is to
reject the whole connection, such as protocol compatibility checks. Because
those checks happen before any space traffic, the handshake never has to
consider peers running a known-incompatible protocol.

Two things follow. The host no longer has to assemble agent information across
spaces for a connection whose eventual use it cannot predict. And the
requirement that a local agent must already have joined before a preflight can
be produced disappears, along with the special handling that requirement needs
today.

### Approaches ruled out

**Treat "no agents known at this URL" as permitted rather than blocked.** This
removes the symptom by removing the enforcement, and it makes blocking
unreliable in exactly the situation where it is supposed to work. It also
leaves the underlying problem — that a space may never learn who it is talking
to — in place.

**Confirm a URL when the hello is sent rather than when it is answered.** This
removes the response, and with it the only evidence that the agents landed. A
hello lost on a connection that survives, or dropped by a receiver that could
not apply it, would leave the URL confirmed, no further hello would ever be sent
to that peer, and every later message to it would be silently discarded with
nothing to trigger a repair. That failure is invisible from both sides, and it
is the failure this design exists to remove.

## Edge cases

**A hello arrives for a space the receiver does not have.** Drop it, and send
nothing back. This should not normally occur: discovery is space-scoped, so a
sender only learns a URL in the context of a space both sides have joined. It
can happen transiently when the sender is acting on an agent info that is still
cached after the remote removed the space.

**The peer is unreachable.** This surfaces as an ordinary transport failure and
the existing handling for unresponsive peers applies. The handshake adds
nothing.

**No response arrives.** The URL stays unconfirmed, so every later send to that
peer carries another hello ahead of it. The traffic still goes out, and may
still be dropped on arrival, but the handshake is attempted again with every
message rather than once, so a peer that failed to apply one hello is offered
another. Where no response ever arrives — the peer does not have the space, has
blocked the sender, cannot parse what the sender emits, or is not there — the
traffic behind it had no prospect of being useful anyway.

**The peer blocks the sender.** The hello is dropped and nothing is returned, so
the sender's traffic to that peer is dropped with it and the URL is never
confirmed. The sender goes on prefixing a hello that will never be answered, and
is not told, which is the intended behaviour for blocking.

**A space has no local agents.** It has nothing to put in a hello, so it does
not send one, and the send that would have triggered it fails as it does today.
It can still apply a hello that arrives and answer it, because a response
carries nothing of its own.

**Both sides send a hello at the same time.** Each applies the other's hello and
answers it, and because neither had confirmed the other yet, each also sends a
hello the other has already answered. Both sides still end up confirmed, for the
cost of a few redundant messages. No case needs detecting: the extra hellos are
absorbed as any other repeat is, and the responses to them answer no outstanding
hello and are dropped.

**A space is torn down and re-created while the connection persists.** The
restarted space starts with an empty set, so it runs the handshake again the
next time it wants to send, and the peer that kept its state applies the repeat
and answers it. The gap is in the other direction: the peer still has the URL
confirmed and has lost nothing, while the restarted space has lost the agents it
learned, so the peer's traffic is discarded on arrival until the restarted space
learns them again. That is a narrower window than the problem this design
removes, but it is not zero.

## Security considerations

The handshake module is the first thing an unknown peer can reach in a space,
so it is the exposed surface and should be treated as such. It should accept
only well-formed messages and bound both the message size and the number of
agent infos a hello may carry. A response carries nothing, so there is little in
one to get wrong, but a response that answers no outstanding hello must be
dropped rather than treated as confirmation of anything.

Answering every hello means a peer can induce a message by sending one. The
ratio is one to one and a response is smaller than the hello that caused it, so
this is not an amplification route, and the reply goes only to the peer URL the
hello came from.

Bounding how often a peer may send hellos is a separate concern and is not
addressed here. Kitsune2 meters no message type per peer today, and metering one
type alone would not amount to much protection.

Agent information is verified as it is decoded from the wire, before anything
is recorded, which is where verification happens for agent information from any
source. Verification is a property of producing the value at all rather than a
step someone downstream could forget: unverified agent information is not
something that exists to be passed on, so nothing further along has to re-check
it.

The exemption does not weaken blocking. A peer URL with a known blocked agent
in the space receives nothing at all, handshake included. The exemption applies
only where the space has no information about who is at the URL.

Recording agents from a handshake confers nothing that discovering the same
agents through bootstrap would not. The handshake changes how a space learns
about agents, not what learning about them permits.

The handshake does not authenticate. Receiving a hello shows only that the
sender holds agent information for the space, which is public, and a response
shows only that a hello was applied. Neither establishes any trust.
