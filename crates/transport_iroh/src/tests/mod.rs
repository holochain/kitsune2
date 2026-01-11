//! iroh transport module tests

use super::*;

mod frame;
mod stream;
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
                ..Default::default()
            },
        })
        .unwrap();

    assert!(builder.validate_config().is_err());
}

#[test]
fn validate_allowed_plain_text_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
                relay_allow_plain_text: true,
                ..Default::default()
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_ok());
}

#[test]
fn validate_disallowed_plain_text_relay_url() {
    let builder = Builder {
        transport: IrohTransportFactory::create(),
        ..kitsune2_core::default_test_builder()
    };

    builder
        .config
        .set_module_config(&IrohTransportModConfig {
            iroh_transport: IrohTransportConfig {
                relay_url: Some("http://test.url".into()),
                relay_allow_plain_text: false,
                ..Default::default()
            },
        })
        .unwrap();

    let result = builder.validate_config();
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("disallowed plaintext relay url"));
}
