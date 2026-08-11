//! Allocation-safe primitives for decoding length-prefixed rwasm sections.
//!
//! Every section in the rwasm binary format starts with a `u64` length taken straight from
//! untrusted input. Handing that length to `Vec::with_capacity` (which is what both the hand-written
//! and bincode-derived decoders used to do) lets an 11-byte binary request an arbitrary allocation
//! before a single element is read: `u64::MAX` panics with `capacity overflow`, and a large
//! non-overflowing value aborts the process through `handle_alloc_error`.
//!
//! The helpers below reserve a bounded amount up front and grow only as elements arrive, so the
//! peak allocation stays proportional to the input that is actually present. A truncated section
//! fails with `UnexpectedEnd` as soon as the reader runs dry, and any section backed by real input
//! still decodes exactly as it did before — this is not a size limit on the format.

use alloc::vec::Vec;
use bincode::{
    de::{read::Reader, Decoder},
    error::DecodeError,
    Decode,
};

/// Upper bound on how many elements a section decoder reserves before reading any input.
/// Anything beyond this grows on demand as elements are decoded.
const N_MAX_SECTION_PREALLOC: usize = 4096;

/// How many bytes of a byte section are reserved and read at a time.
const N_MAX_BYTES_CHUNK: usize = 64 * 1024;

/// Decodes a section length prefix and converts it to `usize` without trusting its magnitude.
fn decode_section_length<Context, D: Decoder<Context = Context>>(
    decoder: &mut D,
) -> Result<usize, DecodeError> {
    let length: u64 = Decode::decode(decoder)?;
    usize::try_from(length).map_err(|_| DecodeError::OutsideUsizeRange(length))
}

/// Reserves capacity for at most [`N_MAX_SECTION_PREALLOC`] elements of `length`.
fn reserve_capped<T>(vec: &mut Vec<T>, length: usize) -> Result<(), DecodeError> {
    vec.try_reserve(length.min(N_MAX_SECTION_PREALLOC))
        .map_err(|_| DecodeError::Other("rwasm: failed to allocate section"))
}

/// Decodes a length-prefixed `Vec<T>` element by element.
pub(crate) fn decode_section_vec<Context, T, D>(decoder: &mut D) -> Result<Vec<T>, DecodeError>
where
    T: Decode<Context>,
    D: Decoder<Context = Context>,
{
    let length = decode_section_length(decoder)?;
    decoder.claim_container_read::<T>(length)?;
    let mut items = Vec::new();
    reserve_capped(&mut items, length)?;
    for _ in 0..length {
        // See bincode's `unclaim_bytes_read` docs: the container read is claimed for the whole
        // section, so every element must give its share back before decoding itself.
        decoder.unclaim_bytes_read(size_of::<T>());
        items.push(T::decode(decoder)?);
    }
    Ok(items)
}

/// Decodes a length-prefixed `Vec<u8>` in bounded chunks, growing only as bytes arrive.
pub(crate) fn decode_section_bytes<Context, D: Decoder<Context = Context>>(
    decoder: &mut D,
) -> Result<Vec<u8>, DecodeError> {
    let length = decode_section_length(decoder)?;
    decoder.claim_container_read::<u8>(length)?;
    let mut bytes = Vec::new();
    let mut filled = 0;
    while filled < length {
        let target = length.min(filled + N_MAX_BYTES_CHUNK);
        bytes
            .try_reserve(target - filled)
            .map_err(|_| DecodeError::Other("rwasm: failed to allocate section"))?;
        // The reservation above covers the whole chunk, so this cannot allocate again.
        bytes.resize(target, 0);
        decoder.reader().read(&mut bytes[filled..target])?;
        filled = target;
    }
    Ok(bytes)
}
