//! Kitsune2 wire protocol types.

include!(concat!(env!("OUT_DIR"), "/kitsune2.wire.rs"));

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn happy_encode_decode() {
        use prost::Message;

        let m = K2Proto {
            ty: k2_proto::Ty::Module as i32,
            data: bytes::Bytes::from_static(b"a"),
            space: Some(bytes::Bytes::from_static(b"b")),
            module: Some("c".into()),
        };

        let m_enc = m.encode_to_vec();

        let d = K2Proto {
            ty: k2_proto::Ty::Disconnect as i32,
            data: bytes::Bytes::from_static(b"d"),
            space: None,
            module: None,
        };

        let d_enc = d.encode_to_vec();

        // the disconnect message doesn't have a space or module,
        // the encoded message should be smaller.
        assert!(d_enc.len() < m_enc.len());

        let m_dec = K2Proto::decode(std::io::Cursor::new(m_enc)).unwrap();

        assert_eq!(m, m_dec);

        let d_dec = K2Proto::decode(std::io::Cursor::new(d_enc)).unwrap();

        assert_eq!(d, d_dec);
    }
}
