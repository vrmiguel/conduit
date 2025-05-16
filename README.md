# Conduit

> 🔌 _Files flow through, not to_

Conduit is a lightweight file transfer service that streams files directly between clients, with no server-side storage.

## Features
- **Pure streaming**: Files flow through the server, never residing there
- **Ephemeral sessions**: Transfer sessions exist only for the duration of the transfer
- **Zero installation**: Works with tools you already have (`curl`, browsers, etc.)
- **Simple security**: Optional token protection for sensitive transfers
- **Minimal buffering**: Server maintains only 64KB of buffer space for each transfer (configurable)

## Quick Start

### Sending a file
```bash
# Basic transfer (no password)
curl -T file.zip https://conduit.vrmiguel.org/my-vacation-photos

# Transfer with password protection
curl -T file.zip https://conduit.vrmiguel.org/my-protected-vacation-photos?token=P@ssw0rd_S3cur3!X9z
```

### Receiving a file
```bash
# Download an unprotected file
curl https://conduit.vrmiguel.org/vacation-photos -o file.zip

# Download a protected file (must provide the same token)
curl https://conduit.vrmiguel.org/my-protected-vacation-photos?token=P@ssw0rd_S3cur3!X9z -o file.zip
```

## How It Works

1. **Create a session**: When you initiate an upload, Conduit creates a temporary session
2. **Sender waits**: The sender's connection remains open, waiting for a recipient
3. **Recipient connects**: When a recipient connects to the same session
4. **Simultaneous streaming**: Data flows from sender → server → recipient in real-time
5. **Minimal buffering**: The server maintains only a small buffer (default: 64KB)
6. **Session closes**: Once the transfer completes, the session disappears

## API Reference

### Initiate a File Upload Session
```
PUT /{session_name}
```
**Parameters:**
- `session_name`: A unique identifier (10-30 ASCII characters)
- `token` (optional): Security token (12-64 ASCII characters, must include uppercase, lowercase, numbers, and special characters)

**Behavior:**
- The connection remains open, waiting for a recipient
- Nothing is uploaded until a recipient connects

**Response:**
- `200 OK`: Upload successful (after transfer completes)
- `409 Conflict`: Session name already in use
- `400 Bad Request`: Invalid session name or token

### Download a File
```
GET /{session_name}
```
**Parameters:**
- `session_name`: The identifier used during upload
- `token` (optional): Same security token used during upload

**Behavior:**
- Connects to an existing upload session
- Triggers the actual file transfer to begin streaming
- Receives data in real-time as it's sent from the uploader

**Response:**
- `200 OK`: Download stream begins
- `404 Not Found`: Session doesn't exist
- `403 Forbidden`: Incorrect token

## Security Considerations

### Transport Security
- Files are encrypted in transit when using HTTPS (which is the default for conduit.vrmiguel.org)
- However, files are not end-to-end encrypted - the server briefly processes unencrypted data in its buffer
- Consider encrypting sensitive files before transfer for true end-to-end security

### Token Protection
Conduit provides enhanced token protection with the following security measures:
- Tokens are securely hashed using Argon2 (an industry-standard password hashing algorithm)
- Minimum token length of 12 characters required
- Tokens must contain a mix of uppercase, lowercase, numbers, and special characters
- Rate limiting protection against brute force attempts
- Tokens are never stored in plain text

### Security Limitations
- The token is still transmitted as a URL parameter, which may be logged by proxies or client software
- For highest security, consider additional measures:
  - Use a secure channel to communicate the token separately from the session name
  - Use unique, random session names that are difficult to guess
  - Encrypt sensitive files before transmission
  - Use conduit only within trusted networks when transferring sensitive data

## Limitations
- No resume capability for interrupted transfers
- Session names must be unique across all Conduit users
- Both sender and recipient must be online simultaneously for transfer to succeed

## Configuration
Server administrators can configure:
- Maximum buffer size (default: 64KB)
- Session timeout length
- Maximum file size

## License
[MIT License](LICENSE)

## Contributing
Contributions are welcome!
