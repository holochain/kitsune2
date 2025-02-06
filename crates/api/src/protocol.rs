//! Kitsune2 wire protocol types.

use crate::*;

pub(crate) mod proto {
    include!("../proto/gen/kitsune2.wire.rs");
}

pub use proto::{k2_proto::K2WireType, K2Proto};

impl K2Proto {
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
        use prost::Message;

        let m = K2Proto {
            ty: K2WireType::Module as i32,
            data: bytes::Bytes::from_static(b"a"),
            space: Some(bytes::Bytes::from_static(b"b")),
            module: Some("c".into()),
        };

        let m_enc = m.encode_to_vec();

        let d = K2Proto {
            ty: K2WireType::Disconnect as i32,
            data: bytes::Bytes::from_static(b"d"),
            space: None,
            module: None,
        };

        let d_enc = d.encode_to_vec();

        // the disconnect message doesn't have a space or module,
        // the encoded message should be smaller.
        assert!(d_enc.len() < m_enc.len());

        let m_dec = K2Proto::decode(&m_enc).unwrap();

        assert_eq!(m, m_dec);

        let d_dec = K2Proto::decode(&d_enc).unwrap();

        assert_eq!(d, d_dec);
    }
}
