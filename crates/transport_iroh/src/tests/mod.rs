//! iroh transport module tests

use super::*;

mod frame;
mod url;

#[test]
fn validate_bad_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("<bad-url>".into()),
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[test]
fn validate_plain_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("disallowed plaintext relay url"));
}
