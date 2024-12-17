/// Combine a series of op hashes into a single hash.
///
/// Requires that the op hashes are already ordered.
/// If the input is empty, then the output is an empty byte array.
pub fn combine_op_hashes<
    T: IntoIterator<Item = I>,
    I: Clone + Into<bytes::Bytes>,
>(
    hashes: T,
) -> bytes::BytesMut {
    let mut hashes = hashes.into_iter().peekable();
    let mut out = if let Some(first) = hashes.peek() {
        bytes::BytesMut::zeroed(first.clone().into().len())
    } else {
        // `Bytes::new` does not allocate, so if there was no input, then return an empty
        // byte array without allocating.
        return bytes::BytesMut::new();
    };

    for hash in hashes {
        combine_hashes(&mut out, hash.into());
    }

    out
}

pub fn combine_hashes(into: &mut bytes::BytesMut, other: bytes::Bytes) {
    // Properly initialise the target from the source if the target is empty.
    // Otherwise, the loop below would run 0 times.
    if into.is_empty() && !other.is_empty() {
        into.extend_from_slice(&other);
        return;
    }

    if into.len() != other.len() {
        tracing::debug!("Combining hashes of different lengths ({} != {}). This is undefined behaviour.", into.len(), other.len());
    }

    for (into_byte, other_byte) in into.iter_mut().zip(other.iter()) {
        *into_byte ^= other_byte;
    }
}
