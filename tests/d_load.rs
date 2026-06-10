#[cfg(unix)]
use libc;

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

use capsule_server::{
    AppState,
    build_router
};

use sqlx::{
    SqlitePool,
    sqlite::{
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

static TRACING:    std::sync::Once = std::sync::Once::new();
static TEST_DB_ID: AtomicU32       = AtomicU32::new( 0 );

fn init_tracing() {
    TRACING.call_once(|| {
        #[cfg(unix)]
        unsafe {
            let mut rlim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::getrlimit( libc::RLIMIT_NOFILE, &mut rlim );
            rlim.rlim_cur = rlim.rlim_max.min( 10_240 );
            libc::setrlimit( libc::RLIMIT_NOFILE, &rlim );
        }
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

    fn server_errors( &self )     -> u32 { self.server_error.load( Ordering::Relaxed ) }
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
    let db_id = TEST_DB_ID.fetch_add( 1, Ordering::Relaxed );

    let opts = SqliteConnectOptions::from_str(
        &format!( "sqlite:file:loadtest_{db_id}?mode=memory&cache=shared" )
    )
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
        .timeout( Duration::from_secs( 60 ) )
        .pool_max_idle_per_host( 200 )
        .build()
        .unwrap()
}

fn parse_file_id( body: &str ) -> Option<String> {
    body.split( "File ID for downloading is " )
        .nth( 1 )
        .and_then( |s| s.split( '.' ).next() )
        .map( |s| s.trim().to_string() )
}

async fn seed_files( base_url: &str, client: &Client, count: usize, file_size: usize ) -> Vec<String> {
    let mut ids = Vec::with_capacity( count );
    for _ in 0..count {
        let form = Form::new().part(
            "f",
            Part::bytes( vec![ 0xABu8; file_size ] ).file_name( "seed.bin" ),
        );
        let body = client
            .post( format!( "{}/curlup", base_url ) )
            .multipart( form )
            .send()
            .await
            .expect( "seed upload failed" )
            .text()
            .await
            .expect( "seed response read failed" );
        let id = parse_file_id( &body ).expect( "could not parse file ID from seed response" );
        ids.push( id );
    }
    ids
}

async fn cleanup_file_ids( ids: &[String] ) {
    for id in ids {
        let _ = tokio::fs::remove_dir_all( format!( "./uploads/temp/{}", id ) ).await;
    }
}



#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_large_file_uploads() {
    init_tracing();
    const COUNT:     usize    = 50;
    const FILE_SIZE: usize    = 2 * 1024 * 1024;
    const TIME_CONSTRAINT:    Duration = Duration::from_secs( 30 );

    let base_url = start_test_server().await;
    println!(
        "-- Load test: {} concurrent {}MB uploads in <{}s --",
        COUNT, FILE_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs()
    );

    let client       = Arc::new( make_client() );
    let stats        = Stats::new();
    let uploaded_ids: Arc<Mutex<Vec<String>>> = Arc::new( Mutex::new( Vec::new() ) );
    let start        = Instant::now();

    let mut handles = Vec::with_capacity( COUNT );
    for _ in 0..COUNT {
        let client      = Arc::clone( &client );
        let stats       = Arc::clone( &stats );
        let ids         = Arc::clone( &uploaded_ids );
        let url         = format!( "{}/curlup", base_url );

        handles.push( spawn( async move {
            let t    = Instant::now();
            let form = Form::new().part(
                "f",
                Part::bytes( vec![ 0xABu8; FILE_SIZE ] ).file_name( "loadtest.bin" ),
            );
            match client.post( url ).multipart( form ).send().await {
                Ok( r ) => {
                    let status = r.status().as_u16();
                    if let Ok( body ) = r.text().await {
                        if let Some( id ) = parse_file_id( &body ) {
                            ids.lock().unwrap().push( id );
                        }
                    }
                    stats.record( status, t.elapsed().as_micros() as u64 );
                }
                Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
            }
        } ) );
    }

    for h in handles { h.await.unwrap(); }

    let elapsed = start.elapsed();
    println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
    stats.print_to_stdout( COUNT );

    let ids = uploaded_ids.lock().unwrap().clone();
    cleanup_file_ids( &ids ).await;

    let passed = elapsed < TIME_CONSTRAINT
        && stats.server_errors() == 0
        && stats.connection_errors() == 0;

    write_test_audit(
        "test_load_large_file_uploads",
        &format!( "requests={COUNT}, file_size={}MB, time_constraint={}s", FILE_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs() ),
        &stats, COUNT, elapsed, passed,
    );

    assert!( elapsed < TIME_CONSTRAINT, "{COUNT} uploads took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
    assert_eq!( stats.server_errors(), 0, "Server returned 5xx during large uploads" );
    assert_eq!( stats.connection_errors(), 0, "Connection errors during large uploads" );
    println!( "  PASS" );
}



#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_concurrent_downloads() {
    init_tracing();
    const SEED_COUNT:     usize    = 8;
    const SEED_SIZE:      usize    = 2 * 1024 * 1024;
    const DOWNLOAD_COUNT: usize    = 200;
    const TIME_CONSTRAINT:         Duration = Duration::from_secs( 20 );

    let base_url = start_test_server().await;
    let client   = Arc::new( make_client() );

    println!(
        "-- Seeding {} files of {}MB --",
        SEED_COUNT, SEED_SIZE / ( 1024 * 1024 )
    );
    let file_ids = Arc::new( seed_files( &base_url, &client, SEED_COUNT, SEED_SIZE ).await );

    println!(
        "-- Load test: {} concurrent downloads cycling {} real files in <{}s --",
        DOWNLOAD_COUNT, SEED_COUNT, TIME_CONSTRAINT.as_secs()
    );

    let stats = Stats::new();
    let start = Instant::now();

    let mut handles = Vec::with_capacity( DOWNLOAD_COUNT );
    for i in 0..DOWNLOAD_COUNT {
        let client   = Arc::clone( &client );
        let stats    = Arc::clone( &stats );
        let id       = file_ids[ i % SEED_COUNT ].clone();
        let url      = format!( "{}/download/{}", base_url, id );

        handles.push( spawn( async move {
            let t = Instant::now();
            match client.get( url ).send().await {
                Ok( r ) => {
                    let status = r.status().as_u16();
                    let _ = r.bytes().await;
                    stats.record( status, t.elapsed().as_micros() as u64 );
                }
                Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
            }
        } ) );
    }

    for h in handles { h.await.unwrap(); }

    let elapsed = start.elapsed();
    println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
    stats.print_to_stdout( DOWNLOAD_COUNT );

    cleanup_file_ids( &file_ids ).await;

    let passed = elapsed < TIME_CONSTRAINT
        && stats.server_errors() == 0
        && stats.connection_errors() == 0;

    write_test_audit(
        "test_load_concurrent_downloads",
        &format!( "downloads={DOWNLOAD_COUNT}, pool={SEED_COUNT} files of {}MB, time_constraint={}s", SEED_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs() ),
        &stats, DOWNLOAD_COUNT, elapsed, passed,
    );

    assert!( elapsed < TIME_CONSTRAINT, "{DOWNLOAD_COUNT} downloads took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
    assert_eq!( stats.server_errors(), 0, "Server returned 5xx during concurrent downloads" );
    assert_eq!( stats.connection_errors(), 0, "Connection errors during concurrent downloads" );
    println!( "  PASS" );
}



#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_mixed_upload_download() {
    init_tracing();
    const SEED_COUNT:  usize    = 10;
    const SEED_SIZE:   usize    = 1 * 1024 * 1024;
    const UPLOAD_SIZE: usize    = 1 * 1024 * 1024;
    const CONCURRENT:  usize    = 100;
    const TIME_CONSTRAINT:      Duration = Duration::from_secs( 25 );

    let base_url = start_test_server().await;
    let client   = Arc::new( make_client() );

    println!(
        "-- Seeding {} files of {}MB for mixed test --",
        SEED_COUNT, SEED_SIZE / ( 1024 * 1024 )
    );
    let seeded_ids = Arc::new( seed_files( &base_url, &client, SEED_COUNT, SEED_SIZE ).await );

    println!(
        "-- Load test: {} concurrent mixed ({} uploads {}MB + {} downloads) in <{}s --",
        CONCURRENT, CONCURRENT / 2, UPLOAD_SIZE / ( 1024 * 1024 ), CONCURRENT / 2, TIME_CONSTRAINT.as_secs()
    );

    let stats        = Stats::new();
    let uploaded_ids: Arc<Mutex<Vec<String>>> = Arc::new( Mutex::new( Vec::new() ) );
    let start        = Instant::now();

    let mut handles = Vec::with_capacity( CONCURRENT );
    for i in 0..CONCURRENT {
        let client      = Arc::clone( &client );
        let stats       = Arc::clone( &stats );
        let seeded      = Arc::clone( &seeded_ids );
        let upload_ids  = Arc::clone( &uploaded_ids );
        let base        = base_url.clone();

        handles.push( spawn( async move {
            let t = Instant::now();

            if i % 2 == 0 {
                let form = Form::new().part(
                    "f",
                    Part::bytes( vec![ 0xCDu8; UPLOAD_SIZE ] ).file_name( "mixed.bin" ),
                );
                match client.post( format!( "{}/curlup", base ) ).multipart( form ).send().await {
                    Ok( r ) => {
                        let status = r.status().as_u16();
                        if let Ok( body ) = r.text().await {
                            if let Some( id ) = parse_file_id( &body ) {
                                upload_ids.lock().unwrap().push( id );
                            }
                        }
                        stats.record( status, t.elapsed().as_micros() as u64 );
                    }
                    Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
                }
            } else {
                let id  = seeded[ i % SEED_COUNT ].clone();
                let url = format!( "{}/download/{}", base, id );
                match client.get( url ).send().await {
                    Ok( r ) => {
                        let status = r.status().as_u16();
                        let _ = r.bytes().await;
                        stats.record( status, t.elapsed().as_micros() as u64 );
                    }
                    Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
                }
            }
        } ) );
    }

    for h in handles { h.await.unwrap(); }

    let elapsed = start.elapsed();
    println!( "  Completed in {:.3}s", elapsed.as_secs_f64() );
    stats.print_to_stdout( CONCURRENT );

    let mut all_ids: Vec<String> = (*seeded_ids).clone();
    all_ids.extend( uploaded_ids.lock().unwrap().clone() );
    cleanup_file_ids( &all_ids ).await;

    let passed = elapsed < TIME_CONSTRAINT
        && stats.server_errors() == 0
        && stats.connection_errors() == 0;

    write_test_audit(
        "test_load_mixed_upload_download",
        &format!( "concurrent={CONCURRENT} (50% upload {}MB / 50% download), time_constraint={}s", UPLOAD_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs() ),
        &stats, CONCURRENT, elapsed, passed,
    );

    assert!( elapsed < TIME_CONSTRAINT, "{CONCURRENT} mixed ops took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
    assert_eq!( stats.server_errors(), 0, "Server returned 5xx during mixed load" );
    assert_eq!( stats.connection_errors(), 0, "Connection errors during mixed load" );
    println!( "  PASS" );
}



#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_load_sustained_throughput() {
    init_tracing();
    const WAVES:            usize    = 4;
    const UPLOADS_PER_WAVE: usize    = 50;
    const FILE_SIZE:        usize    = 1 * 1024 * 1024;
    const TIME_CONSTRAINT:           Duration = Duration::from_secs( 45 );

    let base_url     = start_test_server().await;
    let client       = Arc::new( make_client() );
    let total_stats  = Stats::new();
    let all_ids: Arc<Mutex<Vec<String>>> = Arc::new( Mutex::new( Vec::new() ) );
    let overall_start = Instant::now();

    println!(
        "-- Load test: {} waves of {} uploads ({}MB) then download all, in <{}s --",
        WAVES, UPLOADS_PER_WAVE, FILE_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs()
    );

    for wave in 0..WAVES {
        let wave_start = Instant::now();
        let wave_ids: Arc<Mutex<Vec<String>>> = Arc::new( Mutex::new( Vec::new() ) );

        let mut upload_handles = Vec::with_capacity( UPLOADS_PER_WAVE );
        for _ in 0..UPLOADS_PER_WAVE {
            let client    = Arc::clone( &client );
            let stats     = Arc::clone( &total_stats );
            let ids       = Arc::clone( &wave_ids );
            let url       = format!( "{}/curlup", base_url );

            upload_handles.push( spawn( async move {
                let t    = Instant::now();
                let form = Form::new().part(
                    "f",
                    Part::bytes( vec![ 0xEFu8; FILE_SIZE ] ).file_name( "wave.bin" ),
                );
                match client.post( url ).multipart( form ).send().await {
                    Ok( r ) => {
                        let status = r.status().as_u16();
                        if let Ok( body ) = r.text().await {
                            if let Some( id ) = parse_file_id( &body ) {
                                ids.lock().unwrap().push( id );
                            }
                        }
                        stats.record( status, t.elapsed().as_micros() as u64 );
                    }
                    Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
                }
            } ) );
        }

        for h in upload_handles { h.await.unwrap(); }

        let wave_id_list    = wave_ids.lock().unwrap().clone();
        let download_count  = wave_id_list.len();
        let mut dl_handles  = Vec::with_capacity( download_count );

        for id in &wave_id_list {
            let client = Arc::clone( &client );
            let stats  = Arc::clone( &total_stats );
            let url    = format!( "{}/download/{}", base_url, id );

            dl_handles.push( spawn( async move {
                let t = Instant::now();
                match client.get( url ).send().await {
                    Ok( r ) => {
                        let status = r.status().as_u16();
                        let _ = r.bytes().await;
                        stats.record( status, t.elapsed().as_micros() as u64 );
                    }
                    Err( _ ) => stats.record_conn_err( t.elapsed().as_micros() as u64 ),
                }
            } ) );
        }
        for h in dl_handles { h.await.unwrap(); }

        all_ids.lock().unwrap().extend( wave_id_list );
        println!(
            "  Wave {} done ({} uploads + {} downloads) — {:.0}ms",
            wave + 1, UPLOADS_PER_WAVE, download_count, wave_start.elapsed().as_millis()
        );
    }

    let elapsed    = overall_start.elapsed();
    let total_ops  = WAVES * UPLOADS_PER_WAVE * 2;
    println!( "  All {} ops completed in {:.3}s", total_ops, elapsed.as_secs_f64() );
    total_stats.print_to_stdout( total_ops );

    let ids = all_ids.lock().unwrap().clone();
    cleanup_file_ids( &ids ).await;

    let passed = elapsed < TIME_CONSTRAINT
        && total_stats.server_errors() == 0
        && total_stats.connection_errors() == 0;

    write_test_audit(
        "test_load_sustained_throughput",
        &format!( "waves={WAVES}, uploads_per_wave={UPLOADS_PER_WAVE}, file_size={}MB, time_constraint={}s", FILE_SIZE / ( 1024 * 1024 ), TIME_CONSTRAINT.as_secs() ),
        &total_stats, total_ops, elapsed, passed,
    );

    assert!( elapsed < TIME_CONSTRAINT, "Sustained load took {:.2}s — exceeded {}s time constraint", elapsed.as_secs_f64(), TIME_CONSTRAINT.as_secs() );
    assert_eq!( total_stats.server_errors(), 0, "Server returned 5xx during sustained load" );
    assert_eq!( total_stats.connection_errors(), 0, "Connection errors during sustained load" );
    println!( "  PASS" );
}
