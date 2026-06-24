use actix_test::TestServer;
use actix_web::App;
use actix_web::web::Bytes;
use conduit::{download, upload};
use futures::future::join;
use futures::stream::{self, Stream};
use std::fs::{self, File};
use std::io::{self, Write};
use std::iter::repeat_with;
use std::path::Path;
use std::sync::{LazyLock, Once};
use std::time::Duration;
use tempfile::{TempDir, tempdir};
use tokio::time::sleep;

static INIT: Once = Once::new();
static TEMP_DIR: LazyLock<TempDir> = LazyLock::new(|| tempdir().unwrap());

// Import the test_server function from your main file
fn test_server() -> anyhow::Result<TestServer> {
    INIT.call_once(|| {
        tracing_subscriber::fmt().compact().init();
    });

    let serv = actix_test::start(move || {
        App::new().configure(|cfg| {
            cfg.service(upload);
            cfg.service(download);
        })
    });

    Ok(serv)
}

fn generate_random_name(length: usize) -> String {
    repeat_with(fastrand::alphanumeric).take(length).collect()
}

// Helper function to calculate MD5 hash of a file
fn calculate_md5(file_path: &Path) -> String {
    let bytes = fs::read(file_path).unwrap();
    let digest = md5::compute(bytes);
    format!("{:x}", digest)
}

// Helper function to generate a test file with specific pattern and size
fn generate_test_file(dir: &Path, size_mb: usize, pattern: &str) -> std::path::PathBuf {
    let file_path = dir.join(format!("test_{}.bin", generate_random_name(10)));
    let mut file = File::create(&file_path).expect("Failed to create test file");

    let total_bytes = size_mb * 1024 * 1024;
    let chunk_size = 1024 * 1024; // 1MB chunks
    let mut remaining_bytes = total_bytes;

    while remaining_bytes > 0 {
        let current_chunk_size = std::cmp::min(chunk_size, remaining_bytes);

        let data = match pattern {
            "zeros" => vec![0; current_chunk_size],
            "sequential" => {
                let repeats = current_chunk_size / 256 + 1;
                (0..repeats * 256)
                    .map(|i| (i % 256) as u8)
                    .take(current_chunk_size)
                    .collect()
            }
            _ => {
                // random
                (0..current_chunk_size).map(|_| fastrand::u8(..)).collect()
            }
        };

        file.write_all(&data).expect("Failed to write to test file");
        remaining_bytes -= current_chunk_size;
    }

    file_path
}

// Helper function to convert a Vec<u8> into a stream of chunks
fn bytes_to_stream(
    data: Vec<u8>,
    chunk_size: usize,
) -> impl Stream<Item = Result<Bytes, io::Error>> {
    let chunks = data
        .chunks(chunk_size)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();

    stream::iter(chunks)
}

// Helper function to run a complete test with both sender and receiver
async fn run_transfer_test(
    server: &TestServer,
    input_file: &Path,
    sender_first: bool,
    token: Option<&str>,
) -> anyhow::Result<()> {
    const RESPONSE_LIMIT: usize = 50 * 1024 * 1024;

    let session_name = generate_random_name(10);
    let output_file = TEMP_DIR
        .path()
        .join(format!("received_{}.bin", generate_random_name(10)));

    let original_hash = calculate_md5(input_file);

    let file_content = fs::read(input_file).expect("Failed to read test file");

    let mut url = format!("/{}", session_name);
    if let Some(t) = token {
        url = format!("{}?token={}", url, t);
    }

    let sender_future = async {
        if sender_first {
            // No delay
        } else {
            sleep(Duration::from_millis(200)).await;
        }

        let chunk_size = 64 * 1024; // 64KB chunks for streaming
        let content_stream = bytes_to_stream(file_content, chunk_size);

        let upload_resp = server
            .put(&url)
            .send_stream(content_stream)
            .await
            .expect("Failed to execute upload request");

        assert!(
            upload_resp.status().is_success(),
            "Upload failed with status: {}",
            upload_resp.status()
        );

        true
    };

    // Define receiver task (as a future, not spawned)
    let receiver_future = async {
        if sender_first {
            sleep(Duration::from_millis(200)).await;
        } else {
            // No delay
        }

        // Receive the file (download)
        let mut download_resp = server
            .get(&url)
            .send()
            .await
            .expect("Failed to execute download request");

        assert!(
            download_resp.status().is_success(),
            "Download failed with status: {}",
            download_resp.status()
        );

        // Save the response body to the output file
        let body_bytes = download_resp
            .body()
            .limit(RESPONSE_LIMIT)
            .await
            .expect("Failed to read response body");

        fs::write(&output_file, &body_bytes).expect("Failed to write output file");

        true
    };

    // Execute both futures concurrently using join
    let (sender_result, receiver_result) = join(sender_future, receiver_future).await;

    // Ensure both operations were successful
    assert!(sender_result, "Sender task failed");
    assert!(receiver_result, "Receiver task failed");

    // Verify file integrity
    let received_hash = calculate_md5(&output_file);

    assert_eq!(
        original_hash, received_hash,
        "File integrity check failed, hashes don't match"
    );

    Ok(())
}

#[actix_web::test]
async fn test_sender_first_small_file() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a small test file with random data (1MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 1, "random");

    // Run the test with sender starting first
    run_transfer_test(&server, &input_file, true, None).await?;

    Ok(())
}

#[actix_web::test]
async fn test_receiver_first_small_file() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a small test file with random data (1MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 1, "random");

    // Run the test with receiver starting first
    run_transfer_test(&server, &input_file, false, None).await?;

    Ok(())
}

#[actix_web::test]
async fn test_medium_file_zeros_pattern() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a medium test file with zeros pattern (5MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 5, "zeros");

    // Run the test with sender starting first
    run_transfer_test(&server, &input_file, true, None).await?;

    Ok(())
}

#[actix_web::test]
async fn test_medium_file_sequential_pattern() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a medium test file with sequential pattern (5MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 5, "sequential");

    // Run the test with sender starting first
    run_transfer_test(&server, &input_file, true, None).await?;

    Ok(())
}

#[actix_web::test]
async fn test_large_file_transfer() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a larger test file with random data (10MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 10, "random");

    // Run the test with sender starting first
    run_transfer_test(&server, &input_file, true, None).await?;

    Ok(())
}

#[actix_web::test]
async fn test_transfer_with_token() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a test file with random data (2MB)
    let input_file = generate_test_file(TEMP_DIR.path(), 2, "random");

    // Run the test with a token for authentication
    run_transfer_test(&server, &input_file, true, Some("secure_token_123")).await?;

    Ok(())
}

#[actix_web::test]
async fn test_transfer_error_timeout() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a session name for the transfer
    let session_name = generate_random_name(10);

    let download_fut = async {
        match tokio::time::timeout(
            Duration::from_secs(6),
            server.get(&format!("/{}", session_name)).send(),
        )
        .await
        {
            Ok(resp) => {
                assert!(
                    resp.expect("Failed to get response")
                        .status()
                        .is_client_error(),
                    "Expected error status for non-existent session"
                );
            }
            Err(_) => {
                panic!("Should have timed out in less than 6 seconds");
            }
        }
        true
    };

    let did_timeout = download_fut.await;
    assert!(did_timeout, "Download did not timeout as expected");

    Ok(())
}

// Test for invalid tokens
#[actix_web::test]
async fn test_invalid_token() -> anyhow::Result<()> {
    let server = test_server()?;

    // Generate a session name for the transfer
    let session_name = generate_random_name(10);
    let input_file = generate_test_file(TEMP_DIR.path(), 1, "random");
    let file_content = fs::read(&input_file).expect("Failed to read test file");

    // Upload with correct token
    let correct_token = "valid_token";
    let upload_url = format!("/{}?token={}", session_name, correct_token);

    // Define upload task
    let upload_fut = async {
        // Create a stream from the file content
        let chunk_size = 64 * 1024; // 64KB chunks for streaming
        let content_stream = bytes_to_stream(file_content.clone(), chunk_size);

        let upload_resp = server
            .put(&upload_url)
            .send_stream(content_stream)
            .await
            .expect("Failed to execute upload request");

        upload_resp.status().is_success()
    };

    // Try to download with incorrect token
    let download_bad_token_fut = async {
        let wrong_token = "invalid_token";
        let download_url = format!("/{}?token={}", session_name, wrong_token);

        let download_resp = server
            .get(&download_url)
            .send()
            .await
            .expect("Failed to execute download request");

        // This should fail with an unauthorized status
        download_resp.status().is_client_error()
    };

    let download_good_token_fut = async {
        let download_url = format!("/{}?token={}", session_name, correct_token);
        let mut download_resp = server
            .get(&download_url)
            .send()
            .await
            .expect("Failed to execute download request");

        let _body = download_resp.body().await.unwrap();

        // This should fail with an unauthorized status
        download_resp.status().is_success()
    };

    let (uploaded_fine, bad_token_failed, good_token_succeeded) =
        tokio::join!(upload_fut, download_bad_token_fut, download_good_token_fut);

    assert!(uploaded_fine, "Upload with valid token failed");
    assert!(
        bad_token_failed,
        "Download with invalid token should have failed but succeeded"
    );

    assert!(good_token_succeeded, "Download with correct token failed");

    Ok(())
}
