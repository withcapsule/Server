use std::{
	time::{
		Duration,
		SystemTime
	}
};

use axum::http::StatusCode;

use sqlx::{
	Row,
	SqlitePool,
};

use tokio::fs::{
	ReadDir,
	DirEntry,
	read_dir,
	remove_dir,
	remove_file,
	remove_dir_all,
};

pub fn spawn_cleanup_task( db: SqlitePool ) {
	tokio::spawn( async move {
		loop {
			tokio::time::sleep( Duration::from_secs( 60 ) ).await;
			println!( "Cleanup function" );

			let current_time: u64 = match SystemTime::now().duration_since( SystemTime::UNIX_EPOCH ) {
				Ok( time ) => time.as_secs(),
				Err( error_message ) => panic!( "Critical system time issue, possibly before UNIX_EPOCH. Details: {}", error_message ),
			};

			let res = match sqlx::query( "SELECT ID, FileName FROM filetable WHERE UploadTime < ?" )
				.bind( (current_time as i64) - 3600 )
				.fetch_all( &db )
				.await
				.map_err( |e| ( StatusCode::INTERNAL_SERVER_ERROR, format!( "Failed to search db, error: {}", e ) ) ) {
					Err( error_message ) => { println!( "error: {:?}", error_message ); continue; }
					Ok( res ) => res
				};

			for row in res {
				let file_id: String   = row.get( "ID" );
				let file_name: String = row.get( "FileName" );

				match remove_file( format!( "./uploads/temp/{}/{}", file_id, file_name ) ).await {
					Err( error ) => {
						println!( "Error in cleanup with file id {} | more info: {}", file_id, error );
						match sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( file_id.to_string() ).execute( &db ).await {
							Ok(..) => {
								println!( "File {} removed and database updated", file_id );
								match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
									Ok(()) => { println!( "File {} directory deleted", file_id ); }
									Err( error_msg ) => { println!( "File {} directory deletion failed. Details: {}", file_id, error_msg ); }
								}
							}
							Err( error ) => { println!( "file {} deleted but database entry still exists. details: {}", file_id, error ) }
						};
					},
					Ok(()) => {
						match sqlx::query( "DELETE FROM filetable WHERE ID = ?" ).bind( file_id.to_string() ).execute( &db ).await {
							Ok(..) => {
								println!( "File {} removed and database updated", file_id );
								match remove_dir( format!( "./uploads/temp/{}", file_id ) ).await {
									Ok(()) => { println!( "File {} directory deleted", file_id ); }
									Err( error_msg ) => { println!( "File {} directory deletion failed. Details: {}", file_id, error_msg ); }
								}
							}
							Err( error ) => { println!( "file {} deleted but database entry still exists. details: {}", file_id, error ) }
						};
					}
				};
			}

			let mut directories: ReadDir = match read_dir( "./uploads/temp" ).await {
				Err( error_msg ) => { println!( "failed to read dirs, error: {}", error_msg ); continue; }
				Ok( dir ) => dir
			};

			loop {
				let entry: DirEntry = match directories.next_entry().await {
					Err( error_msg ) => { println!( "directory read error: {}", error_msg ); break; }
					Ok( dir ) => match dir {
						Some( dir2 ) => dir2,
						None => break
					}
				};

				match sqlx::query( "SELECT ID FROM filetable WHERE ID = ?" )
					.bind( entry.file_name().to_string_lossy().to_string() )
					.fetch_optional( &db )
					.await {
						Err( error_msg ) => { println!( "error in deleting orphaned dir: {}", error_msg ) }
						Ok( None ) => {
							match remove_dir_all( format!( "./uploads/temp/{}", entry.file_name().to_string_lossy().to_string() ) ).await {
								Err( error_msg ) => { println!( "failed to delete orphaned dir {}, error: {}", entry.file_name().to_string_lossy().to_string(), error_msg ); }
								Ok(()) => { println!( "orphaned directory {} deleted", entry.file_name().to_string_lossy().to_string() ) }
							}
						}
						_ => {}
					}
			}
		}
	});
}
