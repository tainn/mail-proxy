use lettre::transport::smtp::Error;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use tracing::{info, warn};

use crate::config::{SmtpConfig, TlsMode};

pub type Transport = AsyncSmtpTransport<Tokio1Executor>;

pub fn transport(config: SmtpConfig) -> Result<Transport, Error> {
    let tls = tls(&config.domain, config.tls)?;
    let mut builder = Transport::builder_dangerous(config.domain)
        .port(config.port)
        .timeout(Some(config.timeout))
        .tls(tls);
    if let Some(credentials) = config.credentials {
        builder = builder.credentials(credentials);
    }
    if let Some(helo_name) = config.helo_name {
        builder = builder.hello_name(ClientId::Domain(helo_name));
    }
    Ok(builder.build())
}

fn tls(domain: &str, mode: TlsMode) -> Result<Tls, Error> {
    let parameters = || TlsParameters::new(domain.to_owned());
    let tls = match mode {
        TlsMode::StartTls => Tls::Required(parameters()?),
        TlsMode::Opportunistic => Tls::Opportunistic(parameters()?),
        TlsMode::Implicit => Tls::Wrapper(parameters()?),
        TlsMode::None => Tls::None,
    };
    Ok(tls)
}

pub async fn verify(transport: &Transport) {
    match transport.test_connection().await {
        Ok(true) => info!("smtp server is reachable"),
        Ok(false) => warn!("smtp server did not respond to the connection test"),
        Err(error) => warn!("smtp server is not reachable: {error}"),
    }
}
