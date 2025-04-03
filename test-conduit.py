#!/usr/bin/env python3
import os
import sys
import time
import random
import string
import hashlib
import argparse
import requests
import threading
from pathlib import Path
import tempfile

def generate_random_name(length=8):
    """Generate a random session name for the transfer."""
    letters = string.ascii_lowercase
    return ''.join(random.choice(letters) for _ in range(length))

def calculate_md5(file_path):
    """Calculate MD5 hash of a file for verification."""
    hash_md5 = hashlib.md5()
    with open(file_path, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            hash_md5.update(chunk)
    return hash_md5.hexdigest()

def generate_test_file(size_mb, pattern="random"):
    """Generate a test file of the specified size in MB.

    Args:
        size_mb: Size of the file in megabytes
        pattern: Data pattern - "random", "sequential", or "zeros"

    Returns:
        Path to the generated test file
    """
    fd, file_path = tempfile.mkstemp(suffix=".bin")
    total_bytes = size_mb * 1024 * 1024
    chunk_size = 1024 * 1024  # 1MB chunks

    try:
        with os.fdopen(fd, 'wb') as f:
            remaining_bytes = total_bytes

            print(f"Generating {size_mb}MB test file with {pattern} pattern...")

            while remaining_bytes > 0:
                # Determine size of this chunk
                current_chunk_size = min(chunk_size, remaining_bytes)

                if pattern == "zeros":
                    data = b'\0' * current_chunk_size
                elif pattern == "sequential":
                    repeats = current_chunk_size // 256 + 1
                    data = bytes(i % 256 for i in range(repeats * 256))[:current_chunk_size]
                else:  # random
                    data = os.urandom(current_chunk_size)

                # Write the chunk
                f.write(data)
                remaining_bytes -= current_chunk_size

                if total_bytes > 100 * 1024 * 1024 and remaining_bytes % (10 * 1024 * 1024) == 0:
                    percent_done = 100 - (remaining_bytes / total_bytes * 100)
                    print(f"  Generated {percent_done:.1f}%")

        print(f"Test file created: {file_path}")
        return file_path

    except Exception as e:
        print(f"Error generating test file: {e}")
        if os.path.exists(file_path):
            os.unlink(file_path)
        raise

def sender(base_url, session_name, file_path, token=None, delay=0):
    """Upload a file to the Conduit server."""
    print(f"[Sender] Starting upload of {file_path} to session '{session_name}'")

    # Wait if requested delay
    if delay > 0:
        print(f"[Sender] Waiting {delay} seconds before starting...")
        time.sleep(delay)

    url = f"{base_url}/{session_name}"
    if token:
        url += f"?token={token}"

    file_size = os.path.getsize(file_path)
    print(f"[Sender] File size: {file_size} bytes")

    try:
        with open(file_path, 'rb') as f:
            start_time = time.time()
            response = requests.put(url, data=f, stream=True)
            elapsed = time.time() - start_time

            if response.status_code == 200:
                print(f"[Sender] Upload completed successfully in {elapsed:.2f} seconds")
                print(f"[Sender] Average speed: {file_size / (elapsed * 1024 * 1024):.2f} MB/s")
                return True
            else:
                print(f"[Sender] Upload failed with status code {response.status_code}")
                print(f"[Sender] Response: {response.text}")
                return False
    except Exception as e:
        print(f"[Sender] Upload failed with error: {e}")
        return False

def receiver(base_url, session_name, output_path, token=None, delay=0):
    """Download a file from the Conduit server."""
    print(f"[Receiver] Starting download from session '{session_name}' to {output_path}")

    # Wait if requested delay
    if delay > 0:
        print(f"[Receiver] Waiting {delay} seconds before starting...")
        time.sleep(delay)

    url = f"{base_url}/{session_name}"
    if token:
        url += f"?token={token}"

    try:
        start_time = time.time()
        response = requests.get(url, stream=True)

        if response.status_code != 200:
            print(f"[Receiver] Download failed with status code {response.status_code}")
            print(f"[Receiver] Response: {response.text}")
            return False

        total_bytes = 0
        with open(output_path, 'wb') as f:
            for chunk in response.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
                    total_bytes += len(chunk)
                    # Print progress for large files
                    if total_bytes % (1024 * 1024) == 0:
                        print(f"[Receiver] Downloaded {total_bytes / (1024 * 1024):.2f} MB so far")

        elapsed = time.time() - start_time
        print(f"[Receiver] Download completed successfully in {elapsed:.2f} seconds")
        print(f"[Receiver] Downloaded {total_bytes} bytes")
        print(f"[Receiver] Average speed: {total_bytes / (elapsed * 1024 * 1024):.2f} MB/s")
        return True
    except Exception as e:
        print(f"[Receiver] Download failed with error: {e}")
        return False

def run_test(base_url, input_file, output_dir, sender_first=True, token=None):
    """Run a complete test with both sender and receiver."""
    session_name = generate_random_name()
    print(f"Using session name: {session_name}")

    if token:
        print(f"Using token: {token}")

    output_file = Path(output_dir) / f"received_{Path(input_file).name}"

    # Calculate source file hash
    original_hash = calculate_md5(input_file)
    print(f"Original file MD5: {original_hash}")

    # Decide which process starts first
    sender_delay = 0 if sender_first else 2
    receiver_delay = 2 if sender_first else 0

    # Start sender and receiver threads
    sender_thread = threading.Thread(
        target=sender,
        args=(base_url, session_name, input_file, token, sender_delay)
    )

    receiver_thread = threading.Thread(
        target=receiver,
        args=(base_url, session_name, output_file, token, receiver_delay)
    )

    sender_thread.start()
    receiver_thread.start()

    # Wait for both to finish
    sender_thread.join()
    receiver_thread.join()

    # Verify the transfer
    if os.path.exists(output_file):
        received_hash = calculate_md5(output_file)
        print(f"Received file MD5: {received_hash}")

        if original_hash == received_hash:
            print("✅ Success! Files match.")
            return True
        else:
            print("❌ Failure! Files do not match.")
            return False
    else:
        print("❌ Failure! Output file was not created.")
        return False

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Test the Conduit file transfer service")
    parser.add_argument("--url", default="http://localhost:8080", help="Base URL of the Conduit server")
    parser.add_argument("--input", help="Input file to send (instead of generating one)")
    parser.add_argument("--output-dir", default=".", help="Directory to save the received file")
    parser.add_argument("--receiver-first", action="store_true", help="Start receiver before sender")
    parser.add_argument("--token", help="Optional token for secure transfer")
    parser.add_argument("--size", type=int, default=10, help="Size of the generated test file in MB (if --input not specified)")
    parser.add_argument("--pattern", choices=["random", "sequential", "zeros"], default="random",
                        help="Data pattern for the generated file")
    parser.add_argument("--keep", action="store_true", help="Keep the generated test file after the test")

    args = parser.parse_args()

    if not os.path.exists(args.output_dir):
        os.makedirs(args.output_dir)

    # Determine the input file
    generated_file = None
    input_file = args.input

    if not input_file:
        # Generate a test file if none is provided
        generated_file = generate_test_file(args.size, args.pattern)
        input_file = generated_file
    elif not os.path.exists(input_file):
        print(f"Error: Input file {input_file} does not exist")
        sys.exit(1)

    try:
        # Run the test
        success = run_test(
            args.url,
            input_file,
            args.output_dir,
            not args.receiver_first,
            args.token
        )

        # Clean up the generated file if needed
        if generated_file and not args.keep and os.path.exists(generated_file):
            print(f"Removing generated test file: {generated_file}")
            os.unlink(generated_file)

        sys.exit(0 if success else 1)

    except KeyboardInterrupt:
        print("\nTest interrupted by user")
        # Cleanup on interrupt
        if generated_file and not args.keep and os.path.exists(generated_file):
            print(f"Removing generated test file: {generated_file}")
            os.unlink(generated_file)
        sys.exit(130)
    except Exception as e:
        print(f"Error during test: {e}")
        # Cleanup on error
        if generated_file and not args.keep and os.path.exists(generated_file):
            print(f"Removing generated test file: {generated_file}")
            os.unlink(generated_file)
        sys.exit(1)
