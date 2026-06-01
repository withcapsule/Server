// Load test — 1000 requests in under 5 seconds.
// Spawns its own in-process server with no rate limiting so all requests hit real handlers.
// Pass condition: zero 5xx, zero connection errors, completion within time budget.
// Results are written to audit_log.txt after each test.

use std::{
	io::{
		Write
	},
	net::{
		SocketAddr
	},
	str::{
		FromStr
	},
	sync::{
		atomic::{
			AtomicU32,
			Ordering
		},
		Arc,
		Mutex,
		OnceLock
	},
	time::{
		Duration,
		Instant,
		SystemTime,
		UNIX_EPOCH
	}
};

use filemover_server::{
	AppState, build_router
};

use sqlx::{
	Row,
	SqlitePool,
	sqlite::{
		SqliteRow,
		SqliteConnectOptions
	}
};

use reqwest::{
	Client,
	multipart::{
		Form,
		Part
	}
};

use tokio::{
	spawn,
	fs::{
		create_dir_all
	},
	net::{
		TcpListener
	}
};

static TRACING: std::sync::Once = std::sync::Once::new();

fn init_tracing() {
	TRACING.call_once(|| {
		let _ = tracing_subscriber::fmt()
			.with_test_writer()
			.with_env_filter( "info" )
			.try_init();
	});
}

static AUDIT_LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

fn audit_log() -> &'static Mutex<std::fs::File> {
	AUDIT_LOG.get_or_init(|| {
		let file = std::fs::OpenOptions::new()
			.create( true )
			.write( true )
			.truncate( true )
			.open( "audit_log.txt" )
			.expect( "failed to open audit_log.txt" );
		Mutex::new( file )
	})
}

fn write_audit( text: &str ) {
	audit_log().lock().unwrap().write_all( text.as_bytes() ).unwrap();
}

struct Stats {
	ok:               AtomicU32,
	not_found:        AtomicU32,
	rate_limited:     AtomicU32,
	server_error:     AtomicU32,
	connection_error: AtomicU32,
	latencies_us:     Mutex<Vec<u64>>,
}

impl Stats {
	fn new() -> Arc<Self> {
		Arc::new( Self {
			ok:               AtomicU32::new( 0 ),
			not_found:        AtomicU32::new( 0 ),
			rate_limited:     AtomicU32::new( 0 ),
			server_error:     AtomicU32::new( 0 ),
			connection_error: AtomicU32::new( 0 ),
			latencies_us:     Mutex::new( Vec::new() ),
		})
	}

	fn record( &self, status: u16, elapsed_us: u64 ) {
		match status {
			200..=299 => { self.ok.fetch_add( 1, Ordering::Relaxed ); }
			404       => { self.not_found.fetch_add( 1, Ordering::Relaxed ); }
			429       => { self.rate_limited.fetch_add( 1, Ordering::Relaxed ); }
			500..=599 => { self.server_error.fetch_add( 1, Ordering::Relaxed ); }
			_         => {}
		}
		self.latencies_us.lock().unwrap().push( elapsed_us );
	}

	fn record_conn_err( &self, elapsed_us: u64 ) {
		self.connection_error.fetch_add( 1, Ordering::Relaxed );
		self.latencies_us.lock().unwrap().push( elapsed_us );
	}

	fn total( &self ) -> u32 {
		self.ok.load( Ordering::Relaxed )
			+ self.not_found.load( Ordering::Relaxed )
			+ self.rate_limited.load( Ordering::Relaxed )
			+ self.server_error.load( Ordering::Relaxed )
			+ self.connection_error.load( Ordering::Relaxed )
	}

	fn server_errors( &self )    -> u32 { self.server_error.load( Ordering::Relaxed ) }
	fn connection_errors( &self ) -> u32 { self.connection_error.load( Ordering::Relaxed ) }

	fn latency_stats( &self ) -> LatencyStats {
		let mut lats = self.latencies_us.lock().unwrap().clone();

		if lats.is_empty() { return LatencyStats::default(); }
		lats.sort_unstable();

		let len = lats.len();
		let sum: u64 = lats.iter().sum();
		let pct = |p: f64| lats[ (( p / 100.0 ) * ( len - 1 ) as f64).round() as usize ];

		LatencyStats {
			min:  lats[0],
			p50:  pct( 50.0 ),
			p95:  pct( 95.0 ),
			p99:  pct( 99.0 ),
			max:  lats[len - 1],
			mean: sum / len as u64,
		}
	}

	fn print_to_stdout( &self, total: usize ) {
		let ok   = self.ok.load( Ordering::Relaxed );
		let nf   = self.not_found.load( Ordering::Relaxed );
		let rl   = self.rate_limited.load( Ordering::Relaxed );
		let se   = self.server_error.load( Ordering::Relaxed );
		let ce   = self.connection_error.load( Ordering::Relaxed );
		println!( "  2xx (success):    {}", ok );
		println!( "  404 (not found):  {}", nf );
		println!( "  429 (rate limit): {}", rl );
		println!( "  5xx (server err): {}", se );
		println!( "  conn errors:      {}", ce );
		println!( "  total accounted:  {}/{}", self.total(), total );
		let l = self.latency_stats();
		println!( "  latency (ms)  min={:.3}  p50={:.3}  p95={:.3}  p99={:.3}  max={:.3}  mean={:.3}",
			l.min_ms(), l.p50_ms(), l.p95_ms(), l.p99_ms(), l.max_ms(), l.mean_ms() );
	}
}

#[derive(Default)]
struct LatencyStats {
	min:  u64,
	p50:  u64,
	p95:  u64,
	p99:  u64,
	max:  u64,
	mean: u64,
}

impl LatencyStats {
	fn min_ms ( &self ) -> f64 { self.min  as f64 / 1_000.0 }
	fn p50_ms ( &self ) -> f64 { self.p50  as f64 / 1_000.0 }
	fn p95_ms ( &self ) -> f64 { self.p95  as f64 / 1_000.0 }
	fn p99_ms ( &self ) -> f64 { self.p99  as f64 / 1_000.0 }
	fn max_ms ( &self ) -> f64 { self.max  as f64 / 1_000.0 }
	fn mean_ms( &self ) -> f64 { self.mean as f64 / 1_000.0 }
}

fn write_test_audit(
	test_name: &str,
	config:    &str,
	stats:     &Stats,
	total:     usize,
	elapsed:   Duration,
	passed:    bool,
) {
	let unix_ts = SystemTime::now().duration_since( UNIX_EPOCH ).unwrap().as_secs();

	let ok = stats.ok.load( Ordering::Relaxed );
	let nf = stats.not_found.load( Ordering::Relaxed );
	let rl = stats.rate_limited.load( Ordering::Relaxed );
	let se = stats.server_error.load( Ordering::Relaxed );
	let ce = stats.connection_error.load( Ordering::Relaxed );
	let t  = stats.total();
	let l  = stats.latency_stats();

	let pct = |n: u32| if total == 0 { 0.0 } else { n as f64 / total as f64 * 100.0 };

	let entry = format!(
		"\
================================================================================\n\
TEST      : {test_name}\n\
TIMESTAMP : {unix_ts} (Unix)\n\
CONFIG    : {config}\n\
--------------------------------------------------------------------------------\n\
RESPONSES :\n\
  200 OK           : {:>6}  ({:>5.1}%)\n\
  404 Not Found    : {:>6}  ({:>5.1}%)\n\
  429 Too Many Req : {:>6}  ({:>5.1}%)\n\
  5xx Server Error : {:>6}  ({:>5.1}%)\n\
  Connection Error : {:>6}  ({:>5.1}%)\n\
  Total            : {t} / {total}\n\
\n\
LATENCY (ms):\n\
  min  : {:>8.3}\n\
  p50  : {:>8.3}\n\
  p95  : {:>8.3}\n\
  p99  : {:>8.3}\n\
  max  : {:>8.3}\n\
  mean : {:>8.3}\n\
\n\
WALL TIME : {:.3}s\n\
RESULT    : {}\n\
================================================================================\n\
\n",
		ok, pct(ok),
		nf, pct(nf),
		rl, pct(rl),
		se, pct(se),
		ce, pct(ce),
		l.min_ms(), l.p50_ms(), l.p95_ms(), l.p99_ms(), l.max_ms(), l.mean_ms(),
		elapsed.as_secs_f64(),
		if passed { "PASS" } else { "FAIL" },
	);

	write_audit( &entry );
}



async fn start_test_server() -> String {
	let opts = SqliteConnectOptions::from_str( "sqlite:file:loadtest?mode=memory&cache=shared" )
		.unwrap()
		.create_if_missing( true );

	let pool = SqlitePool::connect_with( opts ).await.unwrap();

	sqlx::query( "CREATE TABLE IF NOT EXISTS filetable (ID VARCHAR(16) PRIMARY KEY, FileName VARCHAR(64) NOT NULL, UploadTime INTEGER NOT NULL, FileSize INTEGER NOT NULL )" )
		.execute( &pool ).await.unwrap();

	create_dir_all( "./uploads/temp" ).await.unwrap();

	let state = AppState { database: pool };
	let app = build_router( state ).into_make_service_with_connect_info::<SocketAddr>();

	let listener = TcpListener::bind( "127.0.0.1:0" ).await.unwrap();
	let port = listener.local_addr().unwrap().port();

	spawn( async move { axum::serve( listener, app ).await.unwrap(); } );

	format!( "http://127.0.0.1:{}", port )
}

fn make_client() -> Client {
	Client::builder()
		.timeout( Duration::from_secs( 10 ) )
		.pool_max_idle_per_host( 200 )
		.build()
		.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_1000_ping_burst() {
	init_tracing();
	const COUNT: usize = 1000;
	const TIME_CONSTRAINT: Duration = Duration::from_secs( 5 );

	let base_url = start_test_server().await;
	println!( "-- Load test: {} concurrent GET /ping in <{}s --", COUNT, TIME_CONSTRAINT.as_secs() );

	let client = Arc::new( make_client() );
	let stats = Stats::new();
	let start = Instant::now();

	let mut handles = Vec::with_capacity( COUNT );
	for _ in 0..COUNT {
		let client = Arc::clone( &client );
		let stats = Arc::clone( &stats );
		let url = format!( "{}/ping", base_url );

		handles.push( spawn( async move {
			let t = Instant::now();
			match client.get( url ).send().await {
				Ok( r )  => stats.record( r.status().as_u16(), t.elapsed().as_micros() as u64 ),
				Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
			}
		} ) );
	}

	for h in handles { h.await.unwrap(); }

	let elapsed = start.elapsed();
	println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
	stats.print_to_stdout( COUNT );

	let passed = elapsed < TIME_CONSTRAINT
		&& stats.server_errors() == 0
		&& stats.connection_errors() == 0;

	write_test_audit(
		"test_load_1000_ping_burst",
		&format!( "requests={COUNT}, time_constraint={}s", TIME_CONSTRAINT.as_secs() ),
		&stats, COUNT, elapsed, passed,
	);

	assert!( elapsed < TIME_CONSTRAINT, "{COUNT} requests took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
	assert_eq!( stats.server_errors(), 0, "Server returned 5xx under load" );
	assert_eq!( stats.connection_errors(), 0, "Connection errors under load" );
	println!( "  PASS" );
}




#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_1000_mixed_endpoints() {
	init_tracing();

	const COUNT: usize = 1000;
	const TIME_CONSTRAINT: Duration = Duration::from_secs( 5 );

	let base_url = start_test_server().await;
	println!( "-- Load test: {} mixed requests (ping + 404 downloads) in <{}s --", COUNT, TIME_CONSTRAINT.as_secs() );

	let client = Arc::new( make_client() );
	let stats = Stats::new();
	let start = Instant::now();

	let mut handles = Vec::with_capacity( COUNT );

	for i in 0..COUNT {
		let client = Arc::clone( &client );
		let stats = Arc::clone( &stats );
		let url_ping = format!( "{}/ping", base_url );
		let url_dl   = format!( "{}/download/xxxxxxxx", base_url );

		handles.push( spawn( async move {
			let t = Instant::now();

			let res = if i % 2 == 0 {
				client.get( url_ping ).send().await
			} else {
				client.get( url_dl ).send().await
			};

			match res {
				Ok( r )  => stats.record( r.status().as_u16(), t.elapsed().as_micros() as u64 ),
				Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
			}
		} ) );
	}

	for h in handles { h.await.unwrap(); }

	let elapsed = start.elapsed();
	println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
	stats.print_to_stdout( COUNT );

	let passed = elapsed < TIME_CONSTRAINT
		&& stats.server_errors() == 0
		&& stats.connection_errors() == 0;

	write_test_audit(
		"test_load_1000_mixed_endpoints",
		&format!( "requests={COUNT} (50% ping / 50% download-404), budget={}s", TIME_CONSTRAINT.as_secs() ),
		&stats, COUNT, elapsed, passed,
	);

	assert!( elapsed < TIME_CONSTRAINT, "{COUNT} mixed requests took {:.2}s — exceeded {}s budget", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
	assert_eq!( stats.server_errors(), 0, "Server returned 5xx under mixed load" );
	assert_eq!( stats.connection_errors(), 0, "Connection errors under mixed load" );
	println!( "  PASS" );
}


#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_200_concurrent_uploads() {
	init_tracing();
	const COUNT: usize = 200;
	const TIME_CONSTRAINT: Duration = Duration::from_secs( 10 );

	let base_url = start_test_server().await;
	println!( "-- Load test: {} concurrent small-file uploads in <{}s --", COUNT, TIME_CONSTRAINT.as_secs() );

	let client = Arc::new( make_client() );
	let stats = Stats::new();
	let start = Instant::now();

	let mut handles = Vec::with_capacity( COUNT );
	for _ in 0..COUNT {
		let client = Arc::clone( &client );
		let stats = Arc::clone( &stats );
		let url = format!( "{}/curlup", base_url );
		handles.push( tokio::spawn( async move {
			let t = Instant::now();

			let form = reqwest::multipart::Form::new().part(
				"f",
				reqwest::multipart::Part::bytes( vec![ 0xABu8; 512 ] ).file_name( "loadtest.bin" ),
			);

			match client.post( url ).multipart( form ).send().await {
				Ok( r )  => stats.record( r.status().as_u16(), t.elapsed().as_micros() as u64 ),
				Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
			}
		} ) );
	}

	for h in handles { h.await.unwrap(); }

	let elapsed = start.elapsed();
	println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
	stats.print_to_stdout( COUNT );

	let passed = elapsed < TIME_CONSTRAINT
		&& stats.server_errors() == 0
		&& stats.connection_errors() == 0;

	write_test_audit(
		"test_load_200_concurrent_uploads",
		&format!( "requests={COUNT} (512-byte uploads), time_constraint={}s", TIME_CONSTRAINT.as_secs() ),
		&stats, COUNT, elapsed, passed,
	);

	assert!( elapsed < TIME_CONSTRAINT, "{COUNT} uploads took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
	assert_eq!( stats.server_errors(), 0, "Server returned 5xx during upload" );
	assert_eq!( stats.connection_errors(), 0, "Connection errors during upload" );
	println!( "  PASS" );
}


#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_sustained_throughput() {
	init_tracing();
	const WAVES:    usize    = 4;
	const PER_WAVE: usize    = 250;
	const TOTAL:    usize    = WAVES * PER_WAVE;
	const TIME_CONSTRAINT:   Duration = Duration::from_secs( 5 );

	let base_url = start_test_server().await;
	println!(
		"-- Load test: {} waves of {} ping requests ({} total), sustained in <{}s --",
		WAVES, PER_WAVE, TOTAL, TIME_CONSTRAINT.as_secs()
	);

	let client = Arc::new( make_client() );
	let total_stats = Stats::new();
	let overall_start = Instant::now();

	for wave in 0..WAVES {
		let wave_start = Instant::now();
		let mut handles = Vec::with_capacity( PER_WAVE );

		for _ in 0..PER_WAVE {
			let client = Arc::clone( &client );
			let stats  = Arc::clone( &total_stats );
			let url    = format!( "{}/ping", base_url );

			handles.push( spawn( async move {
				let t = Instant::now();
				match client.get( url ).send().await {
					Ok( r )  => stats.record( r.status().as_u16(), t.elapsed().as_micros() as u64 ),
					Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
				}
			} ) );
		}

		for h in handles { h.await.unwrap(); }

		println!( "  Wave {} done — {:.0}ms", wave + 1, wave_start.elapsed().as_millis() );
	}

	let elapsed = overall_start.elapsed();
	println!( "  All {} requests completed in {:.3}s", TOTAL, elapsed.as_secs_f64() );
	total_stats.print_to_stdout( TOTAL );

	let passed = elapsed < TIME_CONSTRAINT
		&& total_stats.server_errors() == 0
		&& total_stats.connection_errors() == 0;

	write_test_audit(
		"test_load_sustained_throughput",
		&format!( "waves={WAVES}, per_wave={PER_WAVE}, total={TOTAL}, time_constraint={}s", TIME_CONSTRAINT.as_secs() ),
		&total_stats, TOTAL, elapsed, passed,
	);

	assert!( elapsed < TIME_CONSTRAINT, "Sustained load took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
	assert_eq!( total_stats.server_errors(), 0, "Server returned 5xx during sustained load" );
	assert_eq!( total_stats.connection_errors(), 0, "Connection errors during sustained load" );
	println!( "  PASS" );
}
