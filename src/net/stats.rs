use discv5::{ConnectionDirection, ConnectionState, Discv5};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Prints discv5 server stats on a regular cadence.
pub fn run(discv5: Arc<Discv5>, break_time: Option<Duration>, stats: u64) {
    let break_time = break_time.unwrap_or_else(|| Duration::from_secs(10));
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(break_time).await;
            print_stats(Arc::clone(&discv5), stats);
        }
    });
}

/// A bucket statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BucketStatistic {
    /// The associated bucket number.
    pub bucket: u64,
    /// Connected Peer Count
    pub connected_peers: u64,
    /// Disconnected Peer Count
    pub disconnected_peers: u64,
    /// Incoming Peer Count
    pub incoming_peers: u64,
    /// Outgoing Peer Count
    pub outgoing_peers: u64,
}

/// Prints discv5 server stats.
pub fn print_stats(discv5: Arc<Discv5>, stats: u64) {
    let table_entries = discv5.table_entries();
    let self_id: discv5::Key<_> = discv5.local_enr().node_id().into();

    let mut bucket_values = HashMap::new();

    // Reconstruct the buckets
    for (node_id, enr, status) in table_entries {
        let key: discv5::Key<_> = node_id.into();
        let bucket_no = key.log2_distance(&self_id);
        if let Some(bucket_no) = bucket_no {
            bucket_values
                .entry(bucket_no)
                .or_insert_with(Vec::new)
                .push((enr, status));
        }
    }

    // Build some stats
    let mut bucket_stats = Vec::<BucketStatistic>::new();
    for (bucket, entries) in bucket_values {
        let mut connected_peers = 0;
        let mut connected_incoming_peers = 0;
        let mut connected_outgoing_peers = 0;
        let mut disconnected_peers = 0;

        for (_enr, status) in entries {
            match (status.state, status.direction) {
                (ConnectionState::Connected, ConnectionDirection::Incoming) => {
                    connected_peers += 1;
                    connected_incoming_peers += 1;
                }
                (ConnectionState::Connected, ConnectionDirection::Outgoing) => {
                    connected_peers += 1;
                    connected_outgoing_peers += 1;
                }
                (ConnectionState::Disconnected, _) => {
                    disconnected_peers += 1;
                }
            }
        }

        bucket_stats.push(BucketStatistic {
            bucket,
            connected_peers,
            disconnected_peers,
            incoming_peers: connected_incoming_peers,
            outgoing_peers: connected_outgoing_peers,
        });
    }

    // Sort the buckets
    bucket_stats.sort_by_key(|stat| stat.connected_peers);

    // Print only the top `stats` number of buckets
    for bucket_stat in bucket_stats.iter().take(stats as usize) {
        let BucketStatistic {
            bucket,
            connected_peers,
            disconnected_peers,
            incoming_peers: connected_incoming_peers,
            outgoing_peers: connected_outgoing_peers,
        } = bucket_stat;
        tracing::info!(
            target: "peers",
            "Bucket {} statistics: Connected peers: {} (Incoming: {}, Outgoing: {}), Disconnected Peers: {}",
            bucket,
            connected_peers,
            connected_incoming_peers,
            connected_outgoing_peers,
            disconnected_peers
        );
    }
}
