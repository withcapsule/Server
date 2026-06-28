#![allow(dead_code)]

use std::{
	net::SocketAddr,
	str::FromStr,
	sync::atomic::{ AtomicU32, Ordering },
	time::Duration,
};

use tokio::{
	net::TcpListener,
	fs::create_dir_all,
	sync::OnceCell,
};

use reqwest::Client;

use sqlx::{
	SqlitePool,
	sqlite::SqliteConnectOptions,
};

use lazy_limit::{
	init_rate_limiter,
	Duration as RateDuration,
	RuleConfig,
};

use capsule_server::{
	state::{ AppState, BandwidthTracker },
	router::build_router,
};

pub const UNLIMITED: u32 = 1_000_000;

static DB_ID: AtomicU32 = AtomicU32::new( 0 );
static LIMITER: OnceCell<()> = OnceCell::const_new();


pub async fn init_limiter( max_per_sec: u32 ) {
	LIMITER.get_or_init( || async {
		init_rate_limiter!(
			default: RuleConfig::new( RateDuration::seconds( 1 ), max_per_sec )
		).await;
	} ).await;
}


pub async fn spawn_server() -> String {
	let id = DB_ID.fetch_add( 1, Ordering::Relaxed );

	let opts = SqliteConnectOptions::from_str(
		&format!( "sqlite:file:capsuletest_{id}?mode=memory&cache=shared" )
	)
	.expect( "valid sqlite options" )
	.create_if_missing( true );

	let pool = SqlitePool::connect_with( opts ).await.expect( "open in-memory db" );

	sqlx::query( "CREATE TABLE IF NOT EXISTS filetable (ID VARCHAR(16) PRIMARY KEY, FileName VARCHAR(64) NOT NULL, UploadTime INTEGER NOT NULL, FileSize INTEGER NOT NULL, IsEncrypted INTEGER NOT NULL DEFAULT 0)" )
		.execute( &pool ).await.expect( "create table" );

	create_dir_all( "./uploads/temp" ).await.expect( "create uploads dir" );

	let state = AppState { database: pool, bandwidth: BandwidthTracker::new() };
	let app = build_router( state ).into_make_service_with_connect_info::<SocketAddr>();

	let listener = TcpListener::bind( "127.0.0.1:0" ).await.expect( "bind random port" );
	let port = listener.local_addr().unwrap().port();

	tokio::spawn( async move { axum::serve( listener, app ).await.unwrap(); } );

	format!( "http://127.0.0.1:{}", port )
}


pub fn client() -> Client {
	Client::builder()
		.timeout( Duration::from_secs( 60 ) )
		.pool_max_idle_per_host( 200 )
		.build()
		.expect( "build client" )
}


pub fn parse_file_id( body: &str ) -> Option<String> {
	body.split( "File ID for downloading is " )
		.nth( 1 )
		.and_then( |s| s.split( '.' ).next() )
		.map( |s| s.trim().to_string() )
}



pub fn make_test_data( size: usize ) -> Vec<u8> {
	( 0..size ).map( |i| ( i % 256 ) as u8 ).collect()
}


pub async fn cleanup_file_ids( ids: &[String] ) {
	for id in ids {
		let _ = tokio::fs::remove_dir_all( format!( "./uploads/temp/{}", id ) ).await;
	}
}
