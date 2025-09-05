use kitsune2_api::*;
use std::sync::Arc;

/// A default no-op report module.
#[derive(Debug)]
pub struct CoreReportFactory {}

impl CoreReportFactory {
    /// Construct a new [`CoreReportFactory`]
    pub fn create() -> DynReportFactory {
        let out: DynReportFactory = Arc::new(Self {});
        out
    }
}

impl ReportFactory for CoreReportFactory {
    fn default_config(&self, _config: &mut Config) -> K2Result<()> {
        Ok(())
    }

    fn validate_config(&self, _config: &Config) -> K2Result<()> {
        Ok(())
    }

    fn create(
        &self,
        _builder: Arc<Builder>,
        _tx: DynTransport,
    ) -> BoxFut<'static, K2Result<DynReport>> {
        Box::pin(async move {
            let out: DynReport = Arc::new(CoreReport);
            Ok(out)
        })
    }
}

#[derive(Debug)]
struct CoreReport;

impl Report for CoreReport {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
