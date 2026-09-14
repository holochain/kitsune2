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
- Work correctly when a space starts after a connection already exists, and
  when a space is torn down and re-created while the connection persists.
- Preserve block enforcement exactly as it is. The handshake makes enforcement
  work correctly across multiple spaces; it must not make it more permissive.
- Live in shared code, so that transport implementations need no knowledge of
  it.
- Leave room for access control to be added to the same exchange later.

## Non-goals

- Authentication or access control. The handshake shares agent information; it
  establishes nothing about whether the peer should be in the space. Deciding
  that is a separate concern to be added later.
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

The **handshake message** carries the sending space's signed agent infos. It
travels within the space as a module message under a reserved module identifier,
encoded as protobuf so that it can be extended later.

There is no reply. A handshake message is not a request, nothing is returned for
one, and a sender never waits on one. Both sides send a handshake message, for
the same reason and under the same rule: a space sends one before it first sends
anything else to a peer URL. Which side goes first is whichever space wants to
talk first, and if both want to at once, both send.

This works because the requirement is one-directional. A space needs the peer it
is about to talk to to know its agents; it needs nothing back in order to send
safely. The reverse direction is the peer's own concern, and the peer satisfies
it with its own handshake message when it next has something to send.

Little is lost by having no reply. A space can only address a peer URL it
discovered, and discovering it means holding an agent info that carries that
URL, so a sender already knows an agent at the peer it is sending to. The side
that can be ignorant is the one receiving an unsolicited first message, and that
is exactly the side a handshake message informs. What a reply would have added
is a refresh of the sender's possibly stale view of the peer, and the peer's own
handshake message supplies that as soon as it has something to say.

Agent information is additive, exactly as it is when it arrives from any other
source. A handshake message supplies agents; it never removes them, and it is
not a snapshot that supersedes what the receiver already knows. Superseding an
existing record follows the same rules that apply to agent information
generally.

A handshake message carrying no agent infos cannot serve its purpose and is
dropped. So is one that is malformed, oversized, or otherwise not understood.

### Exemption from block enforcement

Messages addressed to the handshake module are exempt from the rule that drops
traffic to and from a peer URL with no known agents in the space. Without this
the handshake could never run: it exists precisely to resolve that state.

The exemption is narrow, and two limits define it.

It applies only to the reserved module identifier used by the handshake.
Everything else addressed to that peer in that space is still dropped until the
handshake has supplied agents.

It does not extend to peers that are known to be blocked. If any agent known at
that URL is blocked in this space, handshake messages are dropped along with
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

Each space keeps, in memory, the set of peer URLs it has sent a handshake
message to.

A URL is marked once the handshake message has been sent successfully, and not
before. If the send fails, the URL stays unmarked and the space sends a
handshake message again the next time it has something to send to that peer.
Nothing else gates the mark, because there is no acknowledgement to wait for.

A successful send is not delivery. It means the transport accepted the message,
not that the peer received it, so a handshake message can still be lost with the
connection that was carrying it. The space therefore clears a URL from the set
when the connection to it is lost, whether or not the loss had anything to do
with a handshake message, and the next send to that peer sends one again over
the new connection. Without this, a handshake message lost in transit would
leave the URL marked and every later message to that peer silently discarded,
with nothing left to trigger a repair.

Clearing on connection loss is right for the same reason clearing on restart is:
a peer reached over a new connection may be a peer that restarted and lost
everything it knew.

Receiving a handshake message does not mark the sender's URL. The two directions
are independent: what a space records is that it has told a peer about its own
agents, never what a peer has told it. A space that has received a handshake
message from a peer it has not yet sent one to still sends its own before it
first speaks.

**A repeat handshake message for a URL already marked must be accepted and
processed normally. It is never rejected.** This is the invariant that removes
the need to treat restarts as a special event. Whichever side lost its state
simply sends again, and the side that kept its state absorbs the repeat
harmlessly. Neither side needs to detect that the other restarted, and there is
only one code path to get right.

The same invariant makes concurrency a non-event. Two sends racing towards the
same unmarked URL may each send a handshake message; the second is absorbed like
any other repeat, and the only cost is one redundant message. Suppressing the
duplicate is permitted but is not required for correctness.

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

The handshake is lazy. It runs when a space is about to send a message to a peer
URL it has not sent a handshake message to. Nothing is sent speculatively, and
no connection is opened for the sake of a handshake alone. Handshake traffic
therefore scales with how much a node actually communicates, not with how many
peers it has discovered.

Because sending is the trigger, the check belongs at the point where a space
hands a message to the transport, rather than inside each module that wants to
send. Gossip, publish, fetch, agent info broadcast and host notifications are
then all covered without any of them knowing the handshake exists.

If the handshake message cannot be sent, the triggering send fails. The
handshake has no retry loop of its own; the callers that trigger it already
retry on their own schedules, and letting them pace it avoids a second,
independent source of traffic aimed at a peer that may be unreachable.

### Ordering

The triggering message is sent immediately after the handshake message, on the
same connection. The sender does not block, and has nothing to block on, because
nothing comes back.

Two invariants make this safe, and both must hold.

**The sender must issue the handshake message and the triggering message in
order, not concurrently.** Concurrent sends may be interleaved on the
connection, and the triggering message arriving first would simply be dropped.

**The receiver must finish applying a handshake message — recording the agents
and updating whatever state block enforcement consults — before it processes the
next message from that peer.** A handshake message that is accepted and then
applied in the background reintroduces exactly the race the handshake exists to
remove: the message behind it is checked for permission before the agents have
landed, and is dropped. Accepting a handshake message and applying it must be
one step from the point of view of message processing.

Both invariants rest on a third, which is a requirement the handshake places on
the transport rather than on either peer: **messages sent to a peer URL must be
delivered in the order the space handed them over.** Issuing in order and
applying in order buy nothing if the path between may reorder. This is the only
demand the handshake makes of a transport, and it is worth stating plainly
because nothing else in the design depends on transport behaviour at all.

### Relationship to preflight

Preflight stops carrying agent information. That responsibility moves entirely
into the handshake message.

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

**Make the exchange a request and a response.** An explicit reply would confirm
that the peer applied what it was sent, and would refresh the sender's view of
the peer at the same time. It costs a second message kind, a rule for matching
responses to outstanding requests, and a decision about what to do with a
response that matches nothing. It buys little in return, because a sender
already knows an agent at any peer it is able to address, and the peer sends its
own handshake message as soon as it has something to say.

## Edge cases

**A handshake message arrives for a space the receiver does not have.** Drop it.
This should not normally occur: discovery is space-scoped, so a sender only
learns a URL in the context of a space both sides have joined. It can happen
transiently when the sender is acting on an agent info that is still cached
after the remote removed the space.

**The peer is unreachable.** This surfaces as an ordinary transport failure and
the existing handling for unresponsive peers applies. The handshake adds
nothing.

**The handshake message is sent but never applied.** A successful send is not
proof that the peer recorded the agents: it may have dropped the handshake
message as malformed or oversized, or not have the space at all. The sender
marks the URL regardless and is not told. Traffic behind the handshake message
is then discarded on arrival, which is the original symptom in miniature. It is
accepted because every case that reaches here is one where that traffic had no
prospect of being useful anyway — the peer does not have the space, has blocked
the sender, or cannot parse what the sender emits. Loss in transit is not one of
those cases, and is not left here: a handshake message that goes down with its
connection is recovered by clearing the URL when the connection is lost.

**The peer blocks the sender.** The handshake message is dropped, and the
sender's traffic to that peer continues to be dropped. The sender is not told,
which is the intended behaviour for blocking.

**A space has no local agents.** It has nothing to put in a handshake message,
so it does not send one, and the send that would have triggered it fails as it
does today. A handshake message arriving while a space is in this state is still
applied, because the sender's agents are worth recording.

**Both sides send a handshake message at the same time.** Both are applied.
Neither is a reply to the other and neither has to be matched to anything, so
simultaneity is not a case that needs handling.

**A space is torn down and re-created while the connection persists.** The
restarted space sends a handshake message again the next time it wants to send,
and the peer that kept its state accepts the repeat. The gap is in the other
direction: the peer still believes it has told the restarted space about its
agents, and the restarted space has lost them, so the peer's traffic is
discarded on arrival until the restarted space rediscovers it. That is a
narrower window than the problem this design removes, but it is not zero.

## Security considerations

The handshake module is the first thing an unknown peer can reach in a space,
so it is the exposed surface and should be treated as such. It should accept
only well-formed messages and bound both the message size and the number of
agent infos a single message may carry.

Bounding how often a peer may send handshake messages is a separate concern and
is not addressed here. Kitsune2 meters no message type per peer today, and
metering one type alone would not amount to much protection.

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

The handshake does not authenticate. Receiving a handshake message shows only
that the sender holds agent information for the space, which is public. It is
the point at which authentication should later be added, and the exchange is
shaped to allow that, but on its own it establishes no trust.
