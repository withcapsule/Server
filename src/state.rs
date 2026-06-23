use std::{
	net::{
		IpAddr
	},
	sync::{
		Arc
	},
	time::{
		Instant
	},
};

use dashmap::{
	DashMap
};

use sqlx::{
	SqlitePool
};

pub const BANDWIDTH_WINDOW_IN_SECONDS: u64 = 3600;
pub const BANDWIDTH_LIMIT_IN_BYTES:    u64 = 2 * 1024 * 1024 * 1024;

struct BandwidthEntry {
	bytes_used:   u64,
	window_start: Instant,
}

#[derive(Clone)]
pub struct BandwidthTracker( Arc<DashMap<IpAddr, BandwidthEntry>> );

impl BandwidthTracker {
	pub fn new() -> Self {
		Self( Arc::new( DashMap::new() ) )
	}

	pub fn would_exceed( &self, ip: IpAddr, bytes: u64 ) -> bool {
		if let Some( entry ) = self.0.get( &ip ) {
			if entry.window_start.elapsed().as_secs() < BANDWIDTH_WINDOW_IN_SECONDS {
				return entry.bytes_used.saturating_add( bytes ) > BANDWIDTH_LIMIT_IN_BYTES;
			}
		}
		false
	}

	pub fn record( &self, ip: IpAddr, bytes: u64 ) {
		let mut entry = self.0.entry( ip ).or_insert_with( || BandwidthEntry {
			bytes_used:   0,
			window_start: Instant::now(),
		} );

		if entry.window_start.elapsed().as_secs() >= BANDWIDTH_WINDOW_IN_SECONDS {
			entry.bytes_used   = bytes;
			entry.window_start = Instant::now();
		} else {
			entry.bytes_used = entry.bytes_used.saturating_add( bytes );
		}
	}
}

#[derive(Clone)]
pub struct AppState {
	pub database:  SqlitePool,
	pub bandwidth: BandwidthTracker,
}
