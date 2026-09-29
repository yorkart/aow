use std::{fmt, str::FromStr};

use crate::{
    Error,
    generator::{MAX_NODE, MAX_SEQUENCE, SEQUENCE_BITS, TIME_SHIFT},
};

const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
// Preserve the upstream alphabet, which differs from Bitcoin's Base58 alphabet.
const BASE58: &[u8; 58] = b"123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";

/// A non-negative 63-bit Snowflake value. Numeric ordering follows timestamp,
/// then node ID, then sequence; this is not a cross-node event ordering guarantee.
///
/// `Display` and `FromStr` use canonical, unpadded lowercase Base36, safe for
/// case-insensitive filenames and at most 13 characters. Variable-width text is not numerically
/// sortable; use the value's `Ord` implementation for sorting.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Snowflake(u64);

impl Snowflake {
    pub const fn from_u64(value: u64) -> Result<Self, Error> {
        if value > i64::MAX as u64 {
            Err(Error::IdOutOfRange)
        } else {
            Ok(Self(value))
        }
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Milliseconds since the generator's configured epoch, not Unix time.
    pub const fn timestamp_millis(self) -> u64 {
        self.0 >> TIME_SHIFT
    }

    pub const fn node_id(self) -> u16 {
        ((self.0 >> SEQUENCE_BITS) & MAX_NODE as u64) as u16
    }

    pub const fn sequence(self) -> u16 {
        (self.0 & MAX_SEQUENCE as u64) as u16
    }

    /// Encode using `0-9a-z` without padding (at most 13 characters).
    pub fn to_base36(self) -> String {
        encode(self.0, BASE36)
    }

    /// Decode canonical lowercase Base36, rejecting uppercase and leading zeros.
    pub fn from_base36(text: &str) -> Result<Self, Error> {
        decode(text, BASE36)
    }

    /// Encode using `0-9A-Za-z` without padding.
    pub fn to_base62(self) -> String {
        encode(self.0, BASE62)
    }

    /// Decode canonical Base62, rejecting leading zeros and overflowing values.
    pub fn from_base62(text: &str) -> Result<Self, Error> {
        decode(text, BASE62)
    }

    /// Encode using bwmarrin/snowflake's exact Base58 alphabet.
    pub fn to_base58(self) -> String {
        encode(self.0, BASE58)
    }

    /// Decode canonical upstream Base58, rejecting leading zero digits (`1`).
    pub fn from_base58(text: &str) -> Result<Self, Error> {
        decode(text, BASE58)
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_base36())
    }
}

impl FromStr for Snowflake {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::from_base36(text)
    }
}

fn encode(mut value: u64, alphabet: &[u8]) -> String {
    let mut buffer = [0; 13];
    let mut start = buffer.len();
    let radix = alphabet.len() as u64;
    loop {
        start -= 1;
        buffer[start] = alphabet[(value % radix) as usize];
        value /= radix;
        if value == 0 {
            break;
        }
    }
    // The private alphabets contain only ASCII.
    String::from_utf8(buffer[start..].to_vec()).expect("ID alphabet is ASCII")
}

fn decode(text: &str, alphabet: &[u8]) -> Result<Snowflake, Error> {
    let bytes = text.as_bytes();
    let max_length = if alphabet.len() == 36 { 13 } else { 11 };
    if bytes.is_empty() || bytes.len() > max_length || (bytes.len() > 1 && bytes[0] == alphabet[0])
    {
        return Err(Error::InvalidEncoding);
    }
    let mut value = 0_u64;
    for byte in bytes {
        let digit = alphabet
            .iter()
            .position(|candidate| candidate == byte)
            .ok_or(Error::InvalidEncoding)?;
        value = value
            .checked_mul(alphabet.len() as u64)
            .and_then(|value| value.checked_add(digit as u64))
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(Error::IdOutOfRange)?;
    }
    Snowflake::from_u64(value)
}

#[cfg(test)]
mod tests;
