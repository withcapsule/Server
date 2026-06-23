mod state;
mod handlers;
mod pages;
mod cleanup;
mod router;

use std::{
	net::{
		SocketAddr
	},
	process::{
		exit
	},
	str::{
		FromStr
	},
};

use lazy_limit::{
	init_rate_limiter,
	Duration as RateDuration,
	RuleConfig,
};

use sqlx::{
	SqlitePool,
	sqlite::{
		SqliteConnectOptions,
		SqliteJournalMode::{
			Wal
		},
	},
};

use tokio::{
	fs::{
		create_dir_all
	},
	net::{
		TcpListener
	},
};

use tracing_subscriber::{
	EnvFilter
};

use state::{
	AppState,
	BandwidthTracker
};

use router::{
	build_router
};

use cleanup::{
	spawn_cleanup_task
};

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt()
		.with_env_filter( EnvFilter::from_default_env().add_directive( "info".parse().unwrap() ) )
		.init();

	let options = SqliteConnectOptions::from_str( "sqlite:capsule.db" )
		.expect( "Expected to create db, failed" )
		.create_if_missing( true )
		.journal_mode( Wal )
		.read_only( false );

	let sqlite_db = SqlitePool::connect_with( options ).await;
	let state = AppState {
		database: match sqlite_db {
			Err( error_message ) => { println!( "Failed to create database. Error: {}", error_message ); exit( 1 ); }
			Ok( db ) => db
		},
		bandwidth: BandwidthTracker::new(),
	};

	sqlx::query( "CREATE TABLE IF NOT EXISTS filetable (ID VARCHAR(16) PRIMARY KEY, FileName VARCHAR(64) NOT NULL, UploadTime INTEGER NOT NULL, FileSize INTEGER NOT NULL, IsEncrypted INTEGER NOT NULL DEFAULT 0)" )
		.execute( &state.database ).await.expect( "Failed to create table; as table didn't exist." );

	create_dir_all( "./uploads/temp" ).await.unwrap();

	init_rate_limiter!(
		default: RuleConfig::new( RateDuration::seconds( 1 ), 1 )
	).await;

	spawn_cleanup_task( state.database.clone() );

	let app = build_router( state );

	let listener = TcpListener::bind( "0.0.0.0:9001" ).await.unwrap();
	axum::serve( listener, app.into_make_service_with_connect_info::<SocketAddr>() ).await.unwrap();
}
