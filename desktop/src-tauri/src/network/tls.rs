#[cfg(target_os = "android")]
pub(crate) fn apply_android_tls_backend(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let tls_config = rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("create Android TLS config failed: {error}"))?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(builder.tls_backend_preconfigured(tls_config))
}

#[cfg(not(target_os = "android"))]
pub(crate) fn apply_android_tls_backend(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, String> {
    Ok(builder)
}
