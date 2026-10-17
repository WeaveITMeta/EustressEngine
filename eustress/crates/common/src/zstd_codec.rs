//! Zstandard frames, compressed and decompressed the same way in every shell.
//!
//! A desktop build uses the `zstd` crate, the C library, which is fast. The
//! browser (`wasm32-unknown-unknown`) has no C compiler for its target, so it
//! uses ruzstd, which is pure Rust: [`decode_all_pure`] reads every frame the C
//! encoder writes, and [`encode_all_pure`] writes frames any zstd decoder
//! reads. Callers use [`decode_all`] and [`encode_all`] and never name either
//! crate.

use std::io::{self, Read};

/// Every frame in `compressed`, decompressed and joined, as `zstd::decode_all`
/// returns them; skippable frames are passed over.
pub fn decode_all(compressed: &[u8]) -> io::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        zstd::decode_all(compressed)
    }
    #[cfg(target_arch = "wasm32")]
    {
        decode_all_pure(compressed)
    }
}

/// `data` as one Zstandard frame. `level` is the C encoder's level; the
/// browser's encoder has one level, about zstd's level 1, and ignores it.
pub fn encode_all(data: &[u8], level: i32) -> io::Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        zstd::encode_all(data, level)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = level;
        Ok(encode_all_pure(data))
    }
}

/// [`decode_all`] in pure Rust, on any target: the browser's decoder. Each
/// frame takes a fresh decoder, since ruzstd's decodes exactly one; a
/// skippable frame's header is read by the decoder, which reports its length,
/// and its body is passed over here.
pub fn decode_all_pure(compressed: &[u8]) -> io::Result<Vec<u8>> {
    use ruzstd::decoding::errors::{FrameDecoderError, ReadFrameHeaderError};
    let mut input = compressed;
    let mut out = Vec::new();
    while !input.is_empty() {
        let skip = match ruzstd::decoding::StreamingDecoder::new(&mut input) {
            Ok(mut frame) => {
                frame.read_to_end(&mut out)?;
                None
            }
            Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame { length, .. })) => {
                Some(length as usize)
            }
            Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        };
        if let Some(length) = skip {
            if length > input.len() {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a skippable zstd frame runs past the end"));
            }
            input = &input[length..];
        }
    }
    Ok(out)
}

/// `data` as one Zstandard frame in pure Rust, on any target: the browser's
/// encoder, at ruzstd's fastest level.
pub fn encode_all_pure(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The browser's decoder reads what the desktop's C encoder writes: one
    /// frame, and two frames joined with a skippable frame between them.
    #[test]
    fn the_pure_decoder_reads_what_the_c_encoder_writes() {
        let first: Vec<u8> = (0..20_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let second = b"a second frame ".repeat(64);
        let one = zstd::encode_all(&first[..], 3).unwrap();
        assert_eq!(decode_all_pure(&one).unwrap(), first);

        let mut joined = one.clone();
        // A skippable frame: a magic number from 0x184D2A50 to 0x184D2A5F, a
        // 4-byte length, then that many bytes.
        joined.extend_from_slice(&0x184D_2A50u32.to_le_bytes());
        joined.extend_from_slice(&5u32.to_le_bytes());
        joined.extend_from_slice(b"skip!");
        joined.extend(zstd::encode_all(&second[..], 19).unwrap());
        let mut both = first.clone();
        both.extend_from_slice(&second);
        assert_eq!(decode_all_pure(&joined).unwrap(), both);
        assert_eq!(zstd::decode_all(&joined[..]).unwrap(), both, "the C decoder reads the same bytes");
    }

    /// The browser's encoder writes frames the C decoder reads.
    #[test]
    fn the_c_decoder_reads_what_the_pure_encoder_writes() {
        let data = b"Eustress ".repeat(4096);
        let frame = encode_all_pure(&data);
        assert!(frame.len() < data.len(), "it compresses");
        assert_eq!(zstd::decode_all(&frame[..]).unwrap(), data);
        assert_eq!(decode_all_pure(&frame).unwrap(), data);
    }

    #[test]
    fn a_cut_frame_or_a_cut_skippable_frame_is_an_error() {
        let one = zstd::encode_all(&b"x".repeat(1000)[..], 3).unwrap();
        assert!(decode_all_pure(&one[..one.len() - 3]).is_err());
        let mut skip = 0x184D_2A50u32.to_le_bytes().to_vec();
        skip.extend_from_slice(&100u32.to_le_bytes());
        skip.extend_from_slice(b"short");
        assert!(decode_all_pure(&skip).is_err());
    }
}
