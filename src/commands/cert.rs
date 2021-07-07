use fs_err as fs;

use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType};

use crate::platform::tmp_file_path;
use crate::options::GenerateDevCert;


pub fn generate_self_signed_cert() -> anyhow::Result<(String, String)> {
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, "EdgeDB Development Server");
    let mut cert_params = CertificateParams::new(
        vec!["localhost".to_string()]
    );
    cert_params.distinguished_name = distinguished_name;
    let cert = Certificate::from_params(cert_params)?;

    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();
    Ok((cert_pem, key_pem))
}

pub fn generate_dev_cert(options: &GenerateDevCert) -> anyhow::Result<()> {
    let (cert_pem, key_pem) = generate_self_signed_cert()?;
    let tmp_key = tmp_file_path(&options.key_file);
    let tmp_cert = tmp_file_path(&options.cert_file);
    fs::write(&tmp_key, key_pem)?;
    fs::write(&tmp_cert, cert_pem)?;
    fs::rename(&tmp_key, &options.key_file)?;
    fs::rename(&tmp_cert, &options.cert_file)?;
    Ok(())
}
