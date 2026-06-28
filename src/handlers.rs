use std::{
	net::{
		IpAddr,
	},
	sync::{
		LazyLock
	},
	time::{
		SystemTime
	},
};

use axum::{
	body::{
		Body
	},
	extract::{
		Multipart,
		Path,
		Query,
		State,
		multipart::{
			Field
		},
	},
	http::{
		HeaderMap,
		StatusCode,
		header,
	},
	response::{
		Json,
		Response,
	},
};

use rand::{
	RngExt,
};

use tokio::{
	io::{
		AsyncWriteExt
	},
	fs::{
		File,
		try_exists,
		remove_dir,
		remove_file,
		create_dir_all,
	},
};

use tokio_util::{
	io::{
		ReaderStream
	}
};

use serde_json::{
	json
};

use tracing::{
	info
};

use sqlx::{
	Row,
	SqlitePool,
	sqlite::{
		SqliteRow
	},
};

use fs2::{
	available_space
};

use real::{
	RealIp
};

use crate::{
	state::{
		AppState,
		MINIMUM_FREE_SPACE,
	}
};

#[derive(serde::Deserialize)]
pub(crate) struct UploadQuery {
	encrypted: Option<bool>,
}

static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new( || {
	include_str!( "words.txt" )
		.lines()
		.map( str::trim )
		.filter( |word| !word.is_empty() )
		.collect()
} );

fn generate_file_id() -> String {
	let mut rng = rand::rng();
	( 0..3 )
		.map( |_| WORDS[ rng.random_range( 0..WORDS.len() ) ].to_lowercase() )
		.collect::<Vec<_>>()
		.join( "-" )
}

fn sanitize_filename( raw: &str ) -> Option<String> {
	let name = std::path::Path::new( raw ).file_name()?.to_str()?;

	if name.is_empty() || name == "." || name == ".." || name.len() > 255 {
		return None;
	}

	let is_forbidden = |c: char| {
		c.is_control() || matches!( c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' )
	};
	if name.chars().any( is_forbidden ) {
		return None;
	}

	Some( name.to_string() )
}

pub( crate ) async fn upload_file( State( state ): State<AppState>, ip: IpAddr, mut parsed_field: Field<'_>, is_encrypted: bool ) -> Result<String, ( StatusCode, String )> {
	let mut file_id = generate_file_id();
	for _ in 0..5 {
		let exists = sqlx::query( "SELECT 1 FROM filetable WHERE ID = ?" )
			.bind( &file_id )
			.fetch_optional( &state.database )
			.await
			.map_err( |error_message| ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to check id uniqueness, error: {}", error_message ) ) )?;

		if exists.is_none() { break; }
		file_id = generate_file_id();
	}

	let raw_file_name = parsed_field.file_name().unwrap_or( "" ).to_string();

	let file_name = match sanitize_filename( &raw_file_name ) {
		Some( name ) => name,
		None => return Err( ( StatusCode::BAD_REQUEST, "Invalid or unsafe file name.\n".to_string() ) ),
	};

	if available_space( "./uploads" ).unwrap_or( u64::MAX ) < MINIMUM_FREE_SPACE {
		return Err( ( StatusCode::SERVICE_UNAVAILABLE, "Server storage is full. Try again later.\n".to_string() ) );
	}

	// info!( file_name, file_id, "upload started" );
	info!( encrypted = is_encrypted, "upload started" );
	create_dir_all( format!( "./uploads/temp/{}", file_id ) ).await.unwrap();
	let path = format!( "./uploads/temp/{}/{}", file_id, file_name );

	let file = File::create( path ).await;

	let mut file_fd = match file {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 1 error: {}", error_msg ) ) ) }
		Ok( filefd ) => filefd
	};

	let mut chunk_loops: u16 = 0;
	let mut total_bytes_written: usize = 0;

	let upload_time: u64 = match SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ) {
		Ok( time ) => time.as_secs(),
		Err( error_message ) => panic!( "Critical system time issue, possibly before UNIX_EPOCH. Details: {}", error_message ),
	};

	loop {
		let chunk_piece = parsed_field.chunk().await;
		chunk_loops += 1;

		let chunk = match chunk_piece {
			Err( error ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "upload_file() location 2 error: {}", error ) ) ); },
			Ok( inner_option_and_bytes ) => inner_option_and_bytes
		};

		let bytes = match chunk {
			Some( bytes ) => bytes,
			None => break
		};

		file_fd.write_all( &bytes ).await.map_err( |error_message| { return ( StatusCode::INTERNAL_SERVER_ERROR, format!( "writing error: {}", error_message ) ); } )?;
		total_bytes_written += bytes.len();
	}

	// info!( file_name, file_id, bytes = total_bytes_written, chunks = chunk_loops - 1, "upload complete" );
	info!( bytes = total_bytes_written, chunks = chunk_loops - 1, encrypted = is_encrypted, "upload complete" );

	if let Err( error_message ) = sqlx::query( "INSERT INTO filetable(ID, FileName, UploadTime, FileSize, IsEncrypted) VALUES(?, ?, ?, ?, ?)" )
		.bind( &file_id )
		.bind( &file_name )
		.bind( upload_time as i64 )
		.bind( total_bytes_written as i64 )
		.bind( is_encrypted )
		.execute( &state.database )
		.await {
			remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await.ok();
			remove_dir( format!( "./uploads/temp/{}", file_id ) ).await.ok();
			return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to add to db, error: {}", error_message ) ) );
		}

	state.bandwidth.record( ip, total_bytes_written as u64 );

	return Ok( format!( "Success, uploaded {} of {} bytes. File ID for downloading is {}.\n", file_name, total_bytes_written, file_id ) )
}

pub async fn lookup_file_record( id: &str, db: &SqlitePool ) -> Result<SqliteRow, ( StatusCode, String )> {
	match sqlx::query( "SELECT ID, FileName, UploadTime, FileSize, IsEncrypted FROM filetable WHERE ID = ?" )
		.bind( id )
		.fetch_optional( db )
		.await {
			Err( e ) => Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to search db, error: {}", e ) ) ),
			Ok( None ) => Err( ( StatusCode::NOT_FOUND, "No file with that ID exists.".to_string() ) ),
			Ok( Some( row ) ) => Ok( row )
		}
}

pub(crate) async fn search_file( state: State<AppState>, parsed_field: Field<'_> ) -> Result<String, ( StatusCode, String )> {
	let id = match parsed_field.text().await {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Could not read the file ID you entered, error: {}", error_msg ) ) ); }
		Ok( id ) => id.to_lowercase()
	};

	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );
	let _upload_time: i64 = res.get( "UploadTime" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) ); }
		Ok( file ) => file
	};

	// info!( file_id, file_name, upload_time, "file lookup" );
	info!( "file lookup" );

	if file_exists {
		return Ok( format!( "File {} found!", file_name  ) );
	} else {
		return Err( ( StatusCode::NOT_FOUND, format!( "File record exists in database but the file is missing on disk." ) ) );
	}
}

pub async fn file_status( state: State<AppState>, Path( id ): Path<String> ) -> Result<Json<serde_json::Value>, ( StatusCode, String )> {
	let id = id.to_lowercase();
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_name: String  = res.get( "FileName" );
	let file_size: i64     = res.get( "FileSize" );
	let upload_time: i64   = res.get( "UploadTime" );
	let is_encrypted: bool = res.get( "IsEncrypted" );

	const EXPIRY_SECS: i64 = 3600;
	let now = SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ).map( |d| d.as_secs() as i64 ).unwrap_or( 0 );
	let time_remaining = ( upload_time + EXPIRY_SECS - now ).max( 0 );

	Ok( Json( json!( {
		"file_name":      file_name,
		"file_size":      file_size,
		"upload_time":    upload_time,
		"time_remaining": time_remaining,
		"is_encrypted":   is_encrypted,
	} ) ) )
}

pub async fn delete_file( state: State<AppState>, Path( id ): Path<String> ) -> Result<String, ( StatusCode, String )> {
	let id = id.to_lowercase();
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String   = res.get( "ID" );
	let file_name: String = res.get( "FileName" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) ); }
		Ok( file ) => file
	};

	if !file_exists {
		return Err( ( StatusCode::NOT_FOUND, "File record exists in database but the file is missing on disk.".to_string() ) );
	}

	if let Err( error ) = remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to remove file {}, error: {}", file_id, error ) ) );
	}

	if let Err( error ) = sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( &file_id ).execute( &state.database ).await {
		return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "file {} deleted but database entry still exists. details: {}", file_id, error ) ) );
	}

	// info!( file_id, file_name, "file deleted" );
	info!( "file deleted" );

	match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
		Ok(()) => Ok( format!( "File {} deleted", file_name ) ),
		Err( error_msg ) => Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "File {} directory deletion failed. Details: {}", file_id, error_msg ) ) ),
	}
}

pub async fn download_file( State( state ): State<AppState>, real_ip: RealIp, Path( id ): Path<String> ) -> Result<Response, ( StatusCode, String )> {
	let id = id.to_lowercase();
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String    = res.get( "ID" );
	let file_name: String  = res.get( "FileName" );
	let file_size: i64     = res.get( "FileSize" );
	let is_encrypted: bool = res.get( "IsEncrypted" );

	let ip = real_ip.ip();

	if state.bandwidth.would_exceed( ip, file_size as u64 ) {
		return Err( ( StatusCode::TOO_MANY_REQUESTS, "Bandwidth limit exceeded. Try again later.".to_string() ) );
	}

	// info!( file_id, file_name, file_size, is_encrypted, "download started" );
	info!( bytes = file_size, encrypted = is_encrypted, "download started" );

	let file_to_send = match File::open( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "unable to open file, details: {}", error_msg ) ) ) }
		Ok( file ) => file
	};

	state.bandwidth.record( ip, file_size as u64 );

	let file_stream = ReaderStream::new( file_to_send );
	let file_body = Body::from_stream( file_stream );

	let response_object = Response::builder()
		.status( StatusCode::OK )
		.header( "Content-Type", "application/octet-stream" )
		.header( "Content-Length", file_size )
		.header( "Content-Disposition", format!( "attachment; filename=\"{}\"", file_name ) )
		.header( "X-Encrypted", if is_encrypted { "true" } else { "false" } )
		.body( file_body )
		.unwrap();

	return Ok( response_object );
}

pub(crate) async fn curl_upload_processor( State( state ): State<AppState>, real_ip: RealIp, Query( query ): Query<UploadQuery>, headers: HeaderMap, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	let ip = real_ip.ip();
	let is_encrypted = query.encrypted.unwrap_or( false );

	if let Some( bytes ) = headers.get( header::CONTENT_LENGTH )
		.and_then( |v| v.to_str().ok() )
		.and_then( |s| s.parse::<u64>().ok() )
	{
		if state.bandwidth.would_exceed( ip, bytes ) {
			return Err( ( StatusCode::TOO_MANY_REQUESTS, "Bandwidth limit exceeded. Try again later.\n".to_string() ) );
		}
	}

	loop {
		let parts_of_curl = part.next_field().await;

		let current_part = match parts_of_curl {
			Err( error ) => { return Err( ( StatusCode::BAD_REQUEST, format!( "curl error location 1: {}\n", error ) ) ); }
			Ok( part_found ) => part_found
		};

		let field = match current_part {
			Some( inner_field ) => inner_field,
			None => break
		};

		let field_name = field.name().unwrap_or( "unknown" ).to_string();

		if field_name == "f" {
			match upload_file( State( state.clone() ), ip, field, is_encrypted ).await {
				Err( error_message ) => { return Err( error_message ); }
				Ok( message_from_uploader ) => { return Ok( message_from_uploader ); }
			}
		}
	}

	return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
}

#[cfg(test)]
mod tests {
	use super::sanitize_filename;

	#[test]
	fn rejects_path_traversal_and_separators() {
		assert_eq!( sanitize_filename( "../../../../home/sean/.bashrc" ).as_deref(), Some( ".bashrc" ) );
		assert_eq!( sanitize_filename( "a/b/c.txt" ).as_deref(), Some( "c.txt" ) );
		assert_eq!( sanitize_filename( "/etc/passwd" ).as_deref(), Some( "passwd" ) );

		assert_eq!( sanitize_filename( ".." ), None );
		assert_eq!( sanitize_filename( "." ), None );
		assert_eq!( sanitize_filename( "" ), None );
		assert_eq!( sanitize_filename( "/" ), None );

		assert_eq!( sanitize_filename( "..\\..\\evil" ), None );
	}

	#[test]
	fn rejects_header_injection_and_control_chars() {
		assert_eq!( sanitize_filename( "a.txt\r\nX-Evil: 1" ), None );

		assert_eq!( sanitize_filename( "a\".txt" ), None );

		assert_eq!( sanitize_filename( "a\0b" ), None );
	}

	#[test]
	fn keeps_normal_filenames_intact() {
		assert_eq!( sanitize_filename( "report.pdf" ).as_deref(), Some( "report.pdf" ) );
		assert_eq!( sanitize_filename( "My Vacation (2026).jpg" ).as_deref(), Some( "My Vacation (2026).jpg" ) );
		assert_eq!( sanitize_filename( "Ubuntu_24.04.iso" ).as_deref(), Some( "Ubuntu_24.04.iso" ) );

		assert_eq!( sanitize_filename( "résumé.txt" ).as_deref(), Some( "résumé.txt" ) );
	}

	#[test]
	fn rejects_overlong_names() {
		let long = "a".repeat( 256 );
		assert_eq!( sanitize_filename( &long ), None );

		let ok = "a".repeat( 255 );
		assert_eq!( sanitize_filename( &ok ).as_deref(), Some( ok.as_str() ) );
	}
}
