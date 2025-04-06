use std::{
    borrow::Borrow,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::MaybeUninit,
    ops::Deref,
    slice,
};

use serde::{Deserialize, Deserializer, de};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Not enough capacity to store more data")]
    NotEnoughCapacity,
    #[error("Message has fewer bytes than the minimum expected")]
    LacksMinimumLength,
}

#[derive(Clone, Copy)]
pub struct SmallString<const MIN: usize, const MAX: usize> {
    buf: [MaybeUninit<u8>; MAX],
    len: u8,
}

impl<const MIN: usize, const MAX: usize> fmt::Display for SmallString<MIN, MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const MIN: usize, const MAX: usize> Hash for SmallString<MIN, MAX> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        <[u8] as Hash>::hash(self.as_slice(), state)
    }
}

impl<const MIN: usize, const MAX: usize> PartialEq for SmallString<MIN, MAX> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<const MIN: usize, const MAX: usize> Borrow<str> for SmallString<MIN, MAX> {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl<const MIN: usize, const MAX: usize> Deref for SmallString<MIN, MAX> {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<const MIN: usize, const MAX: usize> Eq for SmallString<MIN, MAX> {}

impl<const MIN: usize, const MAX: usize> fmt::Debug for SmallString<MIN, MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmallString<{MIN, MAX}>")
            .field("buf", &self.as_str())
            .field("len", &self.length())
            .finish()
    }
}

impl<const MIN: usize, const MAX: usize> SmallString<MIN, MAX> {
    const ELEM: MaybeUninit<u8> = MaybeUninit::uninit();
    const INIT: [MaybeUninit<u8>; MAX] = [Self::ELEM; MAX];

    pub fn new() -> Self {
        Self {
            len: 0,
            buf: Self::INIT,
        }
    }

    pub fn push_str(&mut self, value: &str) -> Result<(), Error> {
        self.extend(value.as_bytes())
    }

    pub fn extend(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.length() + bytes.len() > self.capacity() {
            return Err(Error::NotEnoughCapacity);
        }

        for &byte in bytes {
            unsafe { self.push_unchecked(byte) }
        }

        Ok(())
    }

    /// ```
    pub fn as_slice(&self) -> &[u8] {
        // NOTE(unsafe) avoid bound checks in the slicing operation
        // &buffer[..self.len]
        unsafe { slice::from_raw_parts(self.buf.as_ptr() as *const u8, self.length()) }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.as_slice()) }
    }

    /// # Safety: caller must ensure there's enough capacity
    #[inline]
    pub unsafe fn push_unchecked(&mut self, byte: u8) {
        debug_assert!(!self.is_full());

        let current_pos = self.length();
        unsafe {
            *self.buf.get_unchecked_mut(current_pos) = MaybeUninit::new(byte);
        }

        self.len += 1;
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len as usize == MAX
    }

    #[inline]
    pub fn length(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        MAX
    }
}

impl<'de, const MIN: usize, const MAX: usize> Deserialize<'de> for SmallString<MIN, MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ValueVisitor<'de, const MIN: usize, const MAX: usize>(PhantomData<&'de ()>);

        impl<'de, const MIN: usize, const MAX: usize> de::Visitor<'de> for ValueVisitor<'de, MIN, MAX> {
            type Value = SmallString<MIN, MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    formatter,
                    "a string of at least {MIN} bytes, and at most {MAX} bytes",
                )
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.len() < MIN {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut s = SmallString::new();
                s.push_str(v)
                    .map_err(|_| E::invalid_length(v.len(), &self))?;
                Ok(s)
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if v.len() < MIN {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut s = SmallString::new();

                s.push_str(
                    std::str::from_utf8(v)
                        .map_err(|_| E::invalid_value(de::Unexpected::Bytes(v), &self))?,
                )
                .map_err(|_| E::invalid_length(v.len(), &self))?;

                Ok(s)
            }
        }

        deserializer.deserialize_str(ValueVisitor::<'de, MIN, MAX>(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_plain;

    #[test]
    fn test_minimum_length_deserialize() {
        // Test minimum length constraint during deserialization
        let result: Result<SmallString<5, 20>, _> = serde_plain::from_str("abc");
        assert!(result.is_err());

        // Test valid minimum length
        let result: Result<SmallString<5, 20>, _> = serde_plain::from_str("abcde");
        assert!(result.is_ok());
    }

    #[test]
    fn test_maximum_length() {
        let mut small_string = SmallString::<5, 10>::new();

        // This should be ok as it's exactly at capacity
        let valid = small_string.push_str("abcdefghij");
        assert!(valid.is_ok());
        assert!(small_string.is_full());

        // Reset for next test
        let mut small_string = SmallString::<5, 10>::new();

        // This should fail as it exceeds capacity
        let invalid = small_string.push_str("abcdefghijk");
        assert!(matches!(invalid, Err(Error::NotEnoughCapacity)));
    }

    #[test]
    fn test_deserialize() {
        // Valid deserialization
        let result: Result<SmallString<5, 20>, _> = serde_plain::from_str("hello world");
        assert!(result.is_ok());
        let small_string = result.unwrap();
        assert_eq!(small_string.as_str(), "hello world");

        // Invalid deserialization (too short)
        let result: Result<SmallString<5, 20>, _> = serde_plain::from_str("abc");
        assert!(result.is_err());

        // Invalid deserialization (too long)
        let result: Result<SmallString<5, 10>, _> =
            serde_plain::from_str("this is way too long for a small string");
        assert!(result.is_err());
    }

    #[test]
    fn test_display() {
        let mut small_string = SmallString::<5, 20>::new();
        small_string.push_str("hello world").unwrap();

        assert_eq!(format!("{}", small_string), "hello world");
        assert_eq!(small_string.to_string(), "hello world");

        // Test empty string display
        let empty_string = SmallString::<0, 10>::new();
        assert_eq!(format!("{}", empty_string), "");
    }

    #[test]
    fn test_length() {
        let mut small_string = SmallString::<5, 20>::new();
        assert_eq!(small_string.length(), 0);

        small_string.push_str("hello").unwrap();
        assert_eq!(small_string.length(), 5);

        small_string.push_str(" world").unwrap();
        assert_eq!(small_string.length(), 11);

        // Ensure internal len field is correct
        assert_eq!(small_string.len, 11);
    }

    #[test]
    fn test_basic_operations() {
        let mut small_string = SmallString::<5, 20>::new();

        // Test new creates an empty string
        assert_eq!(small_string.length(), 0);
        assert_eq!(small_string.as_str(), "");

        // Test push_str works
        small_string.push_str("hello").unwrap();
        assert_eq!(small_string.as_str(), "hello");

        // Test extend works
        small_string.extend("world".as_bytes()).unwrap();
        assert_eq!(small_string.as_str(), "helloworld");

        // Test as_slice returns correct bytes
        assert_eq!(small_string.as_slice(), b"helloworld");
    }

    #[test]
    fn test_equality() {
        let mut string1 = SmallString::<5, 20>::new();
        string1.push_str("hello world").unwrap();

        let mut string2 = SmallString::<5, 20>::new();
        string2.push_str("hello world").unwrap();

        let mut string3 = SmallString::<5, 20>::new();
        string3.push_str("different").unwrap();

        assert_eq!(string1, string2);
        assert_ne!(string1, string3);

        // Test equality with SmallString of different generic parameters
        let mut string4 = SmallString::<3, 30>::new();
        string4.push_str("hello world").unwrap();

        // They're equal in terms of content, but different types
        assert_eq!(string1.as_str(), string4.as_str());
    }

    #[test]
    fn test_borrow() {
        let mut small_string = SmallString::<5, 20>::new();
        small_string.push_str("hello world").unwrap();

        let borrowed: &str = small_string.borrow();
        assert_eq!(borrowed, "hello world");
    }

    #[test]
    fn test_deref() {
        let mut small_string = SmallString::<5, 20>::new();
        small_string.push_str("hello world").unwrap();

        // Test deref to str
        let str_slice: &str = &small_string;
        assert_eq!(str_slice, "hello world");

        // Test str methods directly on SmallString through deref
        assert_eq!(small_string.len(), 11);
        assert!(small_string.starts_with("hello"));
        assert!(small_string.contains("world"));
    }

    #[test]
    fn test_debug() {
        let mut small_string = SmallString::<5, 20>::new();
        small_string.push_str("hello").unwrap();

        let debug_str = format!("{:?}", small_string);
        assert!(debug_str.contains("SmallString"));
        assert!(debug_str.contains("buf"));
        assert!(debug_str.contains("hello"));
        assert!(debug_str.contains("len"));
        assert!(debug_str.contains("5"));
    }

    #[test]
    fn test_capacity_methods() {
        let small_string = SmallString::<5, 20>::new();
        assert_eq!(small_string.capacity(), 20);
        assert!(!small_string.is_full());

        let mut full_string = SmallString::<5, 5>::new();
        full_string.push_str("12345").unwrap();
        assert!(full_string.is_full());
    }

    #[test]
    fn test_push_unchecked() {
        let mut small_string = SmallString::<5, 10>::new();
        small_string.push_str("hello").unwrap();

        unsafe {
            small_string.push_unchecked(b' ');
            small_string.push_unchecked(b'w');
            small_string.push_unchecked(b'o');
            small_string.push_unchecked(b'r');
            small_string.push_unchecked(b'l');
        }

        assert_eq!(small_string.as_str(), "hello worl");
        assert_eq!(small_string.length(), 10);
    }

    #[test]
    #[should_panic(expected = "assertion failed")]
    fn test_push_unchecked_debug_assert() {
        // This test verifies that debug_assert in push_unchecked works
        // Note: This will only panic in debug mode
        let mut small_string = SmallString::<5, 5>::new();
        small_string.push_str("12345").unwrap();

        unsafe {
            // This should trigger the debug_assert(!self.is_full())
            small_string.push_unchecked(b'!');
        }
    }

    #[test]
    fn test_incremental_build() {
        // Test that we can build a string incrementally
        let mut small_string = SmallString::<5, 20>::new();

        // Initial string is empty (below MIN)
        assert_eq!(small_string.length(), 0);

        // Add content in chunks
        small_string.push_str("a").unwrap();
        small_string.push_str("bc").unwrap();
        small_string.push_str("de").unwrap();

        // Now it meets MIN length requirement
        assert_eq!(small_string.length(), 5);
        assert_eq!(small_string.as_str(), "abcde");

        // Keep adding
        small_string.push_str("fghij").unwrap();
        assert_eq!(small_string.as_str(), "abcdefghij");
    }

    #[test]
    fn test_error_handling() {
        let mut small_string = SmallString::<5, 10>::new();

        // Fill it up
        small_string.push_str("1234567890").unwrap();

        // Now it should be full
        assert!(small_string.is_full());

        // Trying to add more should fail
        let err = small_string.push_str("more").unwrap_err();
        assert!(matches!(err, Error::NotEnoughCapacity));

        // String should remain unchanged
        assert_eq!(small_string.as_str(), "1234567890");
    }
}
