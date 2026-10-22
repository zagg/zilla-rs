//! Connexion TCP/TLS, avec proxy SOCKS5 / HTTP CONNECT, façon FileZilla.

use crate::hub::LogHub;
use crate::settings::Settings;
use base64::Engine as _;
use std::io;
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{self, pki_types::ServerName, ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

/// Flux applicatif : TCP simple ou tunnel TLS.
pub enum Sock {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl Sock {
    pub fn is_tls(&self) -> bool {
        matches!(self, Sock::Tls(_))
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Sock::Plain(s) => s.read(buf).await,
            Sock::Tls(s) => s.read(buf).await,
        }
    }

    pub async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Sock::Plain(s) => s.write_all(data).await,
            Sock::Tls(s) => s.write_all(data).await,
        }
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        match self {
            Sock::Plain(s) => s.flush().await,
            Sock::Tls(s) => s.flush().await,
        }
    }

    pub async fn close(&mut self) {
        let r = match self {
            Sock::Plain(s) => s.shutdown().await,
            Sock::Tls(s) => s.shutdown().await,
        };
        let _ = r;
        let _ = self.flush().await;
    }

    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.close().await;
        Ok(())
    }

    pub fn nodelay(&mut self) {
        if let Sock::Plain(s) = self {
            let _ = s.set_nodelay(true);
        }
    }

    pub fn peer_addr(&self) -> String {
        match self {
            Sock::Plain(s) => s.peer_addr().map(|a| a.to_string()).unwrap_or_default(),
            Sock::Tls(s) => {
                    let (io, _) = s.get_ref();
                    io.peer_addr().map(|a| a.to_string()).unwrap_or_default()
                }
        }
    }

    pub fn local_addr(&self) -> String {
        match self {
            Sock::Plain(s) => s.local_addr().map(|a| a.to_string()).unwrap_or_default(),
            Sock::Tls(s) => {
                    let (io, _) = s.get_ref();
                    io.local_addr().map(|a| a.to_string()).unwrap_or_default()
                }
        }
    }

    /// (version TLS, suite) — « clar » si non chiffré.
    pub fn tls_info(&self) -> (String, String) {
        let cs = match self {
            Sock::Plain(_) => return ("clar".into(), String::new()),
            Sock::Tls(s) => s.get_ref().1.negotiated_cipher_suite(),
        };
        let cs = match cs {
            Some(c) => c,
            None => return ("TLS".into(), String::new()),
        };
        let v = cs.version();
        let ver = if v == &rustls::version::TLS13 {
            "TLSv1.3"
        } else if v == &rustls::version::TLS12 {
            "TLSv1.2"
        } else {
            "TLS"
        };
        let suite = format!("{:?}", cs.suite());
        (ver.to_string(), suite)
    }
}

/// Vérificateur « tolérant », comme FileZilla qui ne vérifie que si on le lui demande.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _ee: &rustls::pki_types::CertificateDer<'_>,
        _int: &[rustls::pki_types::CertificateDer<'_>],
        _name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _m: &[u8],
        _c: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

/// Configuration TLS, selon les réglages.
pub fn tls_config(settings: &Settings) -> ClientConfig {
    const BOTH: [&'static rustls::SupportedProtocolVersion; 2] =
        [&rustls::version::TLS12, &rustls::version::TLS13];
    const ONLY13: [&'static rustls::SupportedProtocolVersion; 1] = [&rustls::version::TLS13];
    let versions: &[&'static rustls::SupportedProtocolVersion] =
        if settings.tls.min_version.starts_with("1.2") {
            &BOTH
        } else {
            &ONLY13
        };
    let builder = ClientConfig::builder_with_protocol_versions(versions);
    if settings.tls.verify_cert {
        let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // Les fichiers d'autorité locaux (DER ou PEM) sont ajoutés s'ils peuvent l'être.
        for f in &settings.tls.ca_files {
            load_ca(f, &mut roots);
        }
        builder
            .with_root_certificates(roots)
            .with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoVerifier))
            .with_no_client_auth()
    }
}

/// Charge une autorité de certification depuis un fichier DER ou PEM.
fn load_ca(path: &str, store: &mut RootCertStore) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes).to_string();
    if text.contains("-----BEGIN") {
        // découpage manuel des blocs PEM, sans dépendance supplémentaire
        let mut rest = text.as_str();
        while let Some(i) = rest.find("-----BEGIN CERTIFICATE-----") {
            rest = &rest[i + "-----BEGIN CERTIFICATE-----".len()..];
            let end = rest
                .find("-----END CERTIFICATE-----")
                .unwrap_or(rest.len());
            let b64: String = rest[..end].chars().filter(|c| !c.is_whitespace()).collect();
            rest = &rest[end..];
            if let Ok(der) = base64::engine::general_purpose::STANDARD.decode(&b64) {
                let _ = store.add(rustls::pki_types::CertificateDer::from(der));
            }
        }
    } else {
        let _ = store.add(rustls::pki_types::CertificateDer::from(bytes));
    }
}

pub fn connector(settings: &Settings) -> TlsConnector {
    TlsConnector::from(std::sync::Arc::new(tls_config(settings)))
}

fn server_name(host: &str) -> ServerName<'static> {
    let h = host.trim_matches(['[', ']']);
    if let Ok(ip) = h.parse::<IpAddr>() {
        return ServerName::try_from(ip)
            .unwrap_or_else(|_| ServerName::try_from("localhost".to_string()).unwrap());
    }
    ServerName::try_from(h.to_string())
        .unwrap_or_else(|_| ServerName::try_from("localhost".to_string()).unwrap())
}

/// Résout l'hôte selon la famille d'adresses forcée.
async fn resolve(host: &str, family: &str) -> io::Result<Vec<IpAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let hostport = format!("{}:0", host);
    let found: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&hostport)
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::AddrNotAvailable, e.to_string()))?
        .collect();
    let mut ips: Vec<IpAddr> = found
        .into_iter()
        .map(|a| a.ip())
        .filter(|ip| match family {
            "v4" => ip.is_ipv4(),
            "v6" => ip.is_ipv6(),
            _ => true,
        })
        .collect();
    if ips.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("échec de résolution de « {host} »"),
        ));
    }
    ips.sort_by_key(|a| if a.is_ipv4() { 0 } else { 1 });
    Ok(ips)
}

/// TCP brut vers une paire ip:port, avec délai.
async fn tcp_to(ip: IpAddr, port: u16, t: Duration) -> io::Result<TcpStream> {
    let sock = std::net::SocketAddr::new(ip, port);
    let s = tokio::time::timeout(t, TcpStream::connect(sock))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "délai dépassé à la connexion TCP"))??;
    let _ = s.set_nodelay(true);
    Ok(s)
}

/// Établit la connexion distante (proxy pris en compte).
pub async fn connect(host: &str, port: u16, settings: &Settings, hub: &LogHub) -> io::Result<Sock> {
    let t = Duration::from_secs(settings.net.connect_timeout.max(1));
    let proxy_on = settings.proxy.enabled
        && settings.proxy.kind != "none"
        && !settings.proxy.host.is_empty();

    let stream = if proxy_on {
        hub.info(
            "general",
            format!(
                "Proxy {} {}:{}",
                settings.proxy.kind, settings.proxy.host, settings.proxy.port
            ),
        );
        connect_proxy(host, port, settings, hub, t).await?
    } else {
        let ips = resolve(host, &settings.net.ip_family).await?;
        let mut last = io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("aucune adresse pour {host}"),
        );
        let mut stream: Option<TcpStream> = None;
        for ip in ips {
            match tcp_to(ip, port, t).await {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => {
                    hub.trace("general", format!("{ip}:{port} — {e}"));
                    last = e;
                }
            }
        }
        let stream = match stream {
            Some(s) => s,
            None => {
                hub.err("general", format!("Impossible de rejoindre {host}:{port} — {last}"));
                return Err(last);
            }
        };
        let mut sock = Sock::Plain(stream);
        sock.nodelay();
        return Ok(sock);
    };
    let mut sock = Sock::Plain(stream);
    sock.nodelay();
    Ok(sock)
}

/// Place une TLS sur un flux déjà connecté.
pub async fn upgrade_tls(sock: Sock, host: &str, settings: &Settings, hub: &LogHub) -> io::Result<Sock> {
    let tcp = match sock {
        Sock::Plain(t) => t,
        other => return Ok(other),
    };
    let conn = connector(settings);
    let t = Duration::from_secs(settings.net.connect_timeout.max(1));
    let name = server_name(host);
    let out = tokio::time::timeout(t, conn.connect(name, tcp))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "délai TLS dépassé"))?;
    match out {
        Ok(s) => {
            hub.info("general", "TLS établi.");
            Ok(Sock::Tls(Box::new(s)))
        }
        Err(e) => {
            hub.err("general", format!("La négociation TLS a échoué : {e}"));
            Err(e)
        }
    }
}

async fn connect_proxy(
    host: &str,
    port: u16,
    settings: &Settings,
    hub: &LogHub,
    t: Duration,
) -> io::Result<TcpStream> {
    let ph = settings.proxy.host.clone();
    let pp = settings.proxy.port;
    let mut s = tokio::time::timeout(t, TcpStream::connect((ph.as_str(), pp)))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "le proxy ne répond pas"))??;

    match settings.proxy.kind.as_str() {
        "http" => {
            let mut req = format!(
                "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n"
            );
            if !settings.proxy.user.is_empty() {
                let cred = format!("{}:{}", settings.proxy.user, settings.proxy.password);
                req.push_str(&format!(
                    "Proxy-Authorization: Basic {}\r\n",
                    base64::engine::general_purpose::STANDARD.encode(cred)
                ));
            }
            req.push_str("Proxy-Connection: keep-alive\r\n\r\n");
            s.write_all(req.as_bytes()).await?;
            s.flush().await?;
            let mut buf = vec![0u8; 8192];
            let n = tokio::time::timeout(t, s.read(&mut buf))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "proxy muet"))??;
            let resp = String::from_utf8_lossy(&buf[..n]).to_string();
            let status = resp
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("???")
                .to_string();
            if !status.starts_with('2') {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("proxy HTTP : {resp}"),
                ));
            }
            hub.info("general", format!("Proxy HTTP : tunnel établi ({status})."));
            Ok(s)
        }
        "socks5" => {
            let use_auth = !settings.proxy.user.is_empty();
            let greet = if use_auth {
                vec![0x05u8, 0x02, 0x00, 0x02]
            } else {
                vec![0x05u8, 0x01, 0x00]
            };
            s.write_all(&greet).await?;
            let mut rep = [0u8; 2];
            s.read_exact(&mut rep).await?;
            if rep[0] != 5 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "le proxy a renvoyé une réponse invalide",
                ));
            }
            if use_auth && rep[1] == 2 {
                let u = settings.proxy.user.as_bytes();
                let p = settings.proxy.password.as_bytes();
                let mut msg = vec![1u8, u.len() as u8];
                msg.extend_from_slice(u);
                msg.push(p.len() as u8);
                msg.extend_from_slice(p);
                s.write_all(&msg).await?;
                let mut ar = [0u8; 2];
                s.read_exact(&mut ar).await?;
                if ar[1] != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "auth proxy refusée",
                    ));
                }
            }
            let mut req = vec![0x05u8, 0x01, 0x00];
            let ip = if let Ok(i) = host.parse::<IpAddr>() {
                Some(i)
            } else if settings.proxy.resolve_dns {
                resolve(host, "auto").await.ok().map(|v| v[0])
            } else {
                None
            };
            match ip {
                Some(IpAddr::V4(v4)) => {
                    req.push(1);
                    req.extend_from_slice(&v4.octets());
                }
                Some(IpAddr::V6(v6)) => {
                    req.push(4);
                    req.extend_from_slice(&v6.octets());
                }
                None => {
                    req.push(3);
                    req.push(host.len() as u8);
                    req.extend_from_slice(host.as_bytes());
                }
            }
            req.extend_from_slice(&port.to_be_bytes());
            s.write_all(&req).await?;
            let mut hdr = [0u8; 4];
            s.read_exact(&mut hdr).await?;
            if hdr[1] != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    format!("proxy SOCKS5, code d'erreur {}", hdr[1]),
                ));
            }
            let extra = match hdr[3] {
                1 => 4,
                3 => {
                    let mut l = [0u8; 1];
                    s.read_exact(&mut l).await?;
                    l[0] as usize
                }
                4 => 16,
                _ => 0,
            };
            let mut junk = vec![0u8; extra + 2];
            s.read_exact(&mut junk).await?;
            hub.info("general", "Proxy SOCKS5 : tunnel établi.");
            Ok(s)
        }
        k => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("type de proxy inconnu : {k}"),
        )),
    }
}

/// Écouteur de mode actif, avec plage de ports optionnelle.
pub async fn bind_active(
    settings: &Settings,
) -> io::Result<(tokio::net::TcpListener, u16)> {
    let min = settings.ftp.active_port_min;
    let max = settings.ftp.active_port_max.max(min);
    if min > 0 {
        for p in min..=max {
            if let Ok(l) = tokio::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], p)))
                .await
            {
                let port = l.local_addr()?.port();
                return Ok((l, port));
            }
        }
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "aucun port libre dans la plage du mode actif",
        ));
    }
    let l = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = l.local_addr()?.port();
    Ok((l, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_loopback() {
        let v = resolve("127.0.0.1", "auto").await.unwrap();
        assert!(v[0].is_ipv4());
        assert!(resolve("no-such-host.invalid", "auto").await.is_err());
    }

    #[tokio::test]
    async fn bind_active_works() {
        let (l, p) = bind_active(&Settings::default()).await.unwrap();
        assert!(p > 0);
        drop(l);
    }
}
