// TODO - Define API with configuration but no methods
//      - Receive peer store
//      - Select peers for gossip and kick off dummy gossip
//      - Handle incoming gossip initiation, also into dummy gossip
//      - Register active gossip rounds with empty state machine
//      - Include the DHT model and figure out when to update it
//      - Add some simple tests with a memory transport

mod gossip;
pub use gossip::*;

mod protocol;
