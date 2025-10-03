//! # CoreFetch documentation.
//!
//! CoreFetch is a Kitsune2 module for fetching ops from peers.
//!
//! In particular it tracks which ops need to be fetched from which peers,
//! sends fetch requests and processes incoming requests and responses.
//!
//! It consists of multiple parts:
//! - State object that tracks op and peer urls in memory
//! - Fetch tasks that request tracked ops from peers
//! - An incoming request task that retrieves ops from the op store and responds with the
//!   ops to the requester
//! - An incoming response task that writes ops to the op store and removes their ids
//!   from the state object
//!
//! ### State object CoreFetch
//!
//! - Exposes public method CoreFetch::add_ops that takes a list of op ids and a peer url.
//! - Stores pairs of ([OpId], [Url]) in a hash set. Hash set is used to look up elements
//!   by key efficiently. Ops may be added redundantly to the set with different sources
//!   to fetch from, so the set is keyed by op and peer url combined.
//!
//! ### Fetch tasks
//!
//! #### Outgoing requests
//!
//! A channel acts as the queue for outgoing fetch requests. Ops to fetch are sent
//! one by one through the channel to the receiving tasks, running in parallel and processing
//! requests in incoming order. The flow of sending a fetch request is as follows:
//!
//! - Check if request for ([OpId], [Url]) is still in the set of requests to send.
//!     - In case the op to request has been received in the meantime and no longer needs to be
//!       fetched, it will have been removed from the set. Do nothing.
//!     - Otherwise proceed.
//! - Check if the peer is unresponsive. If so, do not send a request.
//! - Dispatch request for op id from peer to transport module.
//! - If the request fails, remove this and all other requests to the peer from the set.
//!
//! #### Incoming requests
//!
//! Similarly to outgoing requests, a channel serves as a queue for incoming requests. The queue
//! has the following properties:
//! - Simple queue which processes items in the order of the incoming requests.
//! - Requests consist of a list of requested op ids and the URL of the requesting peer.
//! - The task attempts to look up the requested op in the data store and send it in a response.
//! - Requests for data that the host doesn't hold should be logged.
//! - If none of the requested ops could be read from the store, no response is sent.
//! - If sending or receiving the response fails, it's the requester's responsibility to request again.
//!
//! ### Incoming responses
//!
//! A channel acts as a queue for incoming responses. When a response to a requested op is received,
//! it must be processed as follows:
//! - Incoming op is written to the op store.
//! - Once persisted successfully, the op is removed from the set of ops to fetch.
//! - If persisting fails, the op is not removed from the set.
//!
//! [OpId]: crate::OpId
//! [Url]: crate::Url
