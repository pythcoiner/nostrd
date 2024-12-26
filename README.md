# Nostrd

Utility to run an [nostr-rs-relay](https://github.com/scsibug/nostr-rs-relay) 
nostr relay instance into rust integration tests.

This repo is (largely) inspired from [bitcoind](https://github.com/rust-bitcoin/bitcoind) & 
[electrsd](https://github.com/RCasatta/electrsd) projects.

# Binaries

This lib is shipped with a [nostr-rs-relay 0.9.0](https://github.com/scsibug/nostr-rs-relay/releases/tag/0.9.0)
linux binary that i've compiled myself for convenience but you should use binaries you build by yourself.

# Usage

## Running with the supplied linux binary

```rust
use nostrd::NostrD;

let nostrd = NostrD::new().unwrap();
let address = nostrd.url();
// returned address is an &str of the form: `ws://<ip_address>:<port>`

// do whatever you want w/ the relay

```

## Running with your binary

```rust
use nostrd::{NostrD, Conf};

let mut conf = Conf::default();
conf.binary = Some("path/to/your/binary".into());

let nostrd = NostrD::with_conf(&conf).unwrap();
let address = nostrd.url();
// returned address is an &str of the form: `ws://<ip_address>:<port>`

// do whatever you want w/ the relay

```

