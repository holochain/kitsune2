//! Kitsune2 wire protocol types.

use crate::*;

include!("../../../proto/gen/kitsune2.space.rs");

impl K2SpaceProto {
    /// Decode this message from a byte array.
    pub fn decode(bytes: &[u8]) -> K2Result<Self> {
        prost::Message::decode(std::io::Cursor::new(bytes)).map_err(|err| {
            K2Error::other_src("Failed to decode K2Proto Message", err)
        })
    }

    /// Encode this message as a bytes::Bytes buffer.
    pub fn encode(&self) -> K2Result<bytes::Bytes> {
        let mut out = bytes::BytesMut::new();

        prost::Message::encode(self, &mut out).map_err(|err| {
            K2Error::other_src("Failed to encode K2Proto Message", err)
        })?;

        Ok(out.freeze())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn happy_encode_decode() {
        let t = K2SpaceProto {
            to_agent: bytes::Bytes::from_static(b"a1"),
            from_agent: bytes::Bytes::from_static(b"a2"),
            data: bytes::Bytes::from_static(b"d"),
        };

        let e = t.encode().unwrap();

        let d = K2SpaceProto::decode(&e).unwrap();

        assert_eq!(t, d);
    }
}
