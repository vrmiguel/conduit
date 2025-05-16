use crate::{Error, Token, Result};
use rand::{RngCore, Rng};
use std::time::{Instant, Duration};
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString
    },
    Argon2
};

// Token requirements
const TOKEN_MIN_LENGTH: usize = 12;
const TOKEN_REQUIRE_UPPERCASE: bool = true;
const TOKEN_REQUIRE_LOWERCASE: bool = true;
const TOKEN_REQUIRE_DIGITS: bool = true;
const TOKEN_REQUIRE_SPECIAL: bool = true;
const TOKEN_SPECIAL_CHARS: &[char] = &['!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '_', '+', '=', '{', '}', '[', ']', '|', ':', ';', ',', '.', '?'];

// Rate limiting constants
const MAX_FAILED_ATTEMPTS: usize = 5;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(15 * 60); // 15 minutes

// Rate limiter for failed token attempts
static RATE_LIMITER: Lazy<Mutex<HashMap<String, (usize, Instant)>>> = Lazy::new(|| {
    Mutex::new(HashMap::new())
});

/// Validates a token against security requirements
pub fn validate_token(token: &Token) -> Result<()> {
    // Minimum length check (redundant with SmallString, but keeping for clarity)
    if token.len() < TOKEN_MIN_LENGTH {
        return Err(Error::TokenLength);
    }

    // Character type requirements
    let mut has_uppercase = !TOKEN_REQUIRE_UPPERCASE;
    let mut has_lowercase = !TOKEN_REQUIRE_LOWERCASE;
    let mut has_digit = !TOKEN_REQUIRE_DIGITS;
    let mut has_special = !TOKEN_REQUIRE_SPECIAL;

    for c in token.chars() {
        if c.is_ascii_uppercase() {
            has_uppercase = true;
        } else if c.is_ascii_lowercase() {
            has_lowercase = true;
        } else if c.is_ascii_digit() {
            has_digit = true;
        } else if TOKEN_SPECIAL_CHARS.contains(&c) {
            has_special = true;
        }
    }

    // Check if token meets complexity requirements
    if !has_uppercase || !has_lowercase || !has_digit || !has_special {
        return Err(Error::WeakToken);
    }

    Ok(())
}

/// Checks for rate limiting on session authentication attempts
pub fn check_rate_limit(session_name: &str) -> Result<()> {
    let now = Instant::now();
    let mut rate_limiter = RATE_LIMITER.lock().unwrap();
    
    if let Some((attempts, timestamp)) = rate_limiter.get(session_name) {
        // If we have previous failed attempts within the window
        if *attempts >= MAX_FAILED_ATTEMPTS && now.duration_since(*timestamp) < RATE_LIMIT_WINDOW {
            return Err(Error::RateLimited);
        }
        
        // If window has expired, reset the counter
        if now.duration_since(*timestamp) >= RATE_LIMIT_WINDOW {
            rate_limiter.remove(session_name);
        }
    }
    
    Ok(())
}

/// Record a failed authentication attempt
pub fn record_failed_attempt(session_name: &str) {
    let now = Instant::now();
    let mut rate_limiter = RATE_LIMITER.lock().unwrap();
    
    match rate_limiter.get_mut(session_name) {
        Some((attempts, timestamp)) => {
            // If within window, increment attempts
            if now.duration_since(*timestamp) < RATE_LIMIT_WINDOW {
                *attempts += 1;
            } else {
                // If window expired, reset counter
                *attempts = 1;
                *timestamp = now;
            }
        }
        None => {
            // First failed attempt
            rate_limiter.insert(session_name.to_string(), (1, now));
        }
    }
}

/// Generate a cryptographically secure random token
pub fn generate_token() -> Token {
    let mut rng = rand::thread_rng();
    let mut token = String::with_capacity(24); // Use a reasonably long token

    // Add at least one of each required character type
    if TOKEN_REQUIRE_UPPERCASE {
        token.push(rng.gen_range(b'A'..=b'Z') as char);
    }

    if TOKEN_REQUIRE_LOWERCASE {
        token.push(rng.gen_range(b'a'..=b'z') as char);
    }

    if TOKEN_REQUIRE_DIGITS {
        token.push(rng.gen_range(b'0'..=b'9') as char);
    }

    if TOKEN_REQUIRE_SPECIAL {
        token.push(TOKEN_SPECIAL_CHARS[rng.gen_range(0..TOKEN_SPECIAL_CHARS.len())]);
    }

    // Fill the rest with random characters from all allowed sets
    while token.len() < 24 {
        let char_type = rng.gen_range(0..4);

        match char_type {
            0 => token.push(rng.gen_range(b'A'..=b'Z') as char),
            1 => token.push(rng.gen_range(b'a'..=b'z') as char),
            2 => token.push(rng.gen_range(b'0'..=b'9') as char),
            3 => token.push(TOKEN_SPECIAL_CHARS[rng.gen_range(0..TOKEN_SPECIAL_CHARS.len())]),
            _ => unreachable!(),
        }
    }

    // Shuffle to avoid predictable patterns
    let mut chars: Vec<char> = token.chars().collect();
    for i in 0..chars.len() {
        let j = rng.gen_range(0..chars.len());
        chars.swap(i, j);
    }

    let shuffled: String = chars.into_iter().collect();

    // Convert to Token type
    let mut result = Token::new();
    result.push_str(&shuffled).expect("Token size should be valid");
    result
}

/// Hash a token using Argon2 for secure storage
pub fn hash_token(token: &str) -> Result<String> {
    let salt = SaltString::generate(&OsRng);

    // Using Argon2id variant with default parameters
    let argon2 = Argon2::default();

    // Hash the token
    match argon2.hash_password(token.as_bytes(), &salt) {
        Ok(hash) => Ok(hash.to_string()),
        Err(_) => Err(Error::TokenValidation("Failed to hash token".to_string())),
    }
}

/// Verify a token against its stored hash
pub fn verify_token(token: &str, hash: &str) -> Result<bool> {
    // Parse the hash string
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(parsed) => parsed,
        Err(_) => return Err(Error::TokenValidation("Invalid hash format".to_string())),
    };

    // Verify the token against the hash
    match Argon2::default().verify_password(token.as_bytes(), &parsed_hash) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}