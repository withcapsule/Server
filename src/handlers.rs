use std::{
	net::{
		IpAddr,
		SocketAddr,
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
		ConnectInfo,
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
	distr::{
		Alphanumeric
	},
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

pub(crate) async fn upload_file( State( state ): State<AppState>, ip: IpAddr, mut parsed_field: Field<'_>, is_encrypted: bool ) -> Result<String, ( StatusCode, String )> {
	let file_id: String = rand::rng().sample_iter( Alphanumeric ).take( 8 ).map( char::from ).collect();
	let file_name = parsed_field.file_name().unwrap_or( "__failure_upload_file()__" ).to_string();

	if file_name == "" || file_name == "__failure_upload_file()__" {
		return Err( ( StatusCode::BAD_REQUEST, "No file found in request".to_string() ) );
	}

	if available_space( "./uploads" ).unwrap_or( u64::MAX ) < MINIMUM_FREE_SPACE {
		return Err( ( StatusCode::SERVICE_UNAVAILABLE, "Server storage is full. Try again later.\n".to_string() ) );
	}

	info!( file_name, file_id, "upload started" );
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

	info!( file_name, file_id, bytes = total_bytes_written, chunks = chunk_loops - 1, "upload complete" );

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
		Ok( id ) => id
	};

	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String = res.get( "ID" );
	let file_name: String = res.get( "FileName" );
	let upload_time: i64 = res.get( "UploadTime" );

	let file_exists: bool = match try_exists( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
		Err( error_msg ) => { return Err( ( StatusCode::NOT_FOUND, format!( "File not found, error: {:?}", error_msg ) ) ); }
		Ok( file ) => file
	};

	info!( file_id, file_name, upload_time, "file lookup" );

	if file_exists {
		return Ok( format!( "File {} found!", file_name  ) );
	} else {
		return Err( ( StatusCode::NOT_FOUND, format!( "File record exists in database but the file is missing on disk." ) ) );
	}
}

pub async fn file_status( state: State<AppState>, Path( id ): Path<String> ) -> Result<Json<serde_json::Value>, ( StatusCode, String )> {
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

	info!( file_id, file_name, "file deleted" );

	match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
		Ok(()) => Ok( format!( "File {} deleted", file_name ) ),
		Err( error_msg ) => Err( ( StatusCode::INTERNAL_SERVER_ERROR, format!( "File {} directory deletion failed. Details: {}", file_id, error_msg ) ) ),
	}
}

pub async fn download_file( State( state ): State<AppState>, ConnectInfo( addr ): ConnectInfo<SocketAddr>, Path( id ): Path<String> ) -> Result<Response, ( StatusCode, String )> {
	let res: SqliteRow = lookup_file_record( &id, &state.database ).await?;

	let file_id: String    = res.get( "ID" );
	let file_name: String  = res.get( "FileName" );
	let file_size: i64     = res.get( "FileSize" );
	let is_encrypted: bool = res.get( "IsEncrypted" );

	let ip = addr.ip();

	if state.bandwidth.would_exceed( ip, file_size as u64 ) {
		return Err( ( StatusCode::TOO_MANY_REQUESTS, "Bandwidth limit exceeded. Try again later.".to_string() ) );
	}

	info!( file_id, file_name, file_size, is_encrypted, "download started" );

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

pub async fn curl_upload_processor( State( state ): State<AppState>, ConnectInfo( addr ): ConnectInfo<SocketAddr>, Query( query ): Query<UploadQuery>, headers: HeaderMap, mut part: Multipart ) -> Result<String, ( StatusCode, String )> {
	let ip = addr.ip();
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
