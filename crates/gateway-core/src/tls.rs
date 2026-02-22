use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use rustls::ServerConfig;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use crate::error::{GatewayError, GatewayResult};

pub fn load_certificates(path: &Path) -> GatewayResult<Vec<CertificateDer<'static>>> {
    let certs: Vec<_> = CertificateDer::pem_file_iter(path)
        .map_err(|e| GatewayError::ConfigError(format!("Failed to parse certificates: {}", e)))?
        .filter_map(|r: Result<CertificateDer<'static>, _>| r.ok())
        .collect();

    if certs.is_empty() {
        return Err(GatewayError::ConfigError("No certificates found".to_string()));
    }

    Ok(certs)
}

pub fn load_private_key(path: &Path) -> GatewayResult<PrivateKeyDer<'static>> {
    PrivateKeyDer::pem_file_iter(path)
        .map_err(|e| GatewayError::ConfigError(format!("Failed to parse private key: {}", e)))?
        .next()
        .ok_or_else(|| GatewayError::ConfigError("No private key found".to_string()))?
        .map_err(|e| GatewayError::ConfigError(format!("Invalid private key: {}", e)))
}

pub fn build_server_config(
    cert_path: &str,
    key_path: &str,
    _ca_path: Option<&str>,
) -> GatewayResult<Arc<ServerConfig>> {
    let certs = load_certificates(Path::new(cert_path))?;
    let key = load_private_key(Path::new(key_path))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| GatewayError::ConfigError(format!("Failed to build TLS config: {}", e)))?;

    Ok(Arc::new(config))
}

pub fn build_client_config() -> Arc<rustls::ClientConfig> {
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
struct InsecureVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

use std::net::SocketAddr;

pub use crate::http::HttpServer;

pub async fn create_http_server(
    port: u16,
    gateway: oclaws_config::settings::Gateway,
    gateway_server: Arc<crate::server::GatewayServer>,
) -> GatewayResult<HttpServer> {
    let addr = format!("0.0.0.0:{}", port);
    let addr: SocketAddr = addr.parse().map_err(|e| {
        GatewayError::ConfigError(format!("Invalid address: {}", e))
    })?;

    let gateway = Arc::new(gateway);

    let http_server = HttpServer::new(addr, gateway.clone(), gateway_server);

    if let Some(tls) = &gateway.tls
        && tls.enabled.unwrap_or(false) {
            let cert_path = tls.cert_path.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("TLS enabled but cert_path not set".to_string())
            })?;
            let key_path = tls.key_path.as_ref().ok_or_else(|| {
                GatewayError::ConfigError("TLS enabled but key_path not set".to_string())
            })?;

            let config = build_server_config(cert_path, key_path, tls.ca_path.as_deref())?;
            info!("TLS enabled with certificate: {}", cert_path);
            return Ok(http_server.with_tls(config));
        }

    Ok(http_server)
}
