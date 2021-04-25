use std::fs;
use std::process::Child;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

use test_case::test_case;

use crate::certs::Certs;
use crate::docker;
use once_cell::sync::Lazy;

static HTTP: Lazy<HttpGuard> = Lazy::new(|| HttpGuard::start());

const HTTP_CONTAINER: &str = "edgedb-test-http";

struct HttpGuard {
    certs: Certs,
    shutdown_info: Mutex<ShutdownInfo>,
}

pub struct ShutdownInfo {
    process: Child,
}

impl HttpGuard {
    fn start() -> HttpGuard {
        HttpGuard::_start().expect("can start http")
    }
    fn _start() -> anyhow::Result<HttpGuard> {
        let certs = Certs::new()?;
        docker::build_image(
            docker::Context::new()
                .add_file("Dockerfile", dockerfile_http())?
                .add_file("default.conf", default_conf())?
                .add_file("nginx-selfsigned.crt", &certs.nginx_cert)?
                .add_file("nginx-selfsigned.key", &certs.nginx_key)?
                .add_file("edgedb-init.sh", fs::read("./edgedb-init.sh")?)?
                .add_sudoers()?
                .add_bin()?,
            "edgedb_test:http_server",
        )?;
        let process = docker::run_bg(HTTP_CONTAINER, "edgedb_test:http_server");
        shutdown_hooks::add_shutdown_hook(stop_process);
        sleep(Duration::from_secs(1));
        Ok(HttpGuard {
            certs,
            shutdown_info: Mutex::new(ShutdownInfo { process }),
        })
    }
    fn url(&self) -> &'static str {
        // docker DNS
        "https://edgedb-test-http"
    }
    fn container(&self) -> &'static str {
        HTTP_CONTAINER
    }
    fn cert(&self) -> &[u8] {
        &self.certs.ca_cert
    }
}

pub fn dockerfile_centos(release: &str) -> String {
    format!(
        r###"
        FROM centos:{release}
        RUN yum -y install sudo curl
        ADD ./selfsigned.crt /etc/pki/ca-trust/source/anchors/selfsigned.crt
        ADD ./sudoers /etc/sudoers
        RUN update-ca-trust
        RUN useradd --uid 1000 --home /home/user1 \
            --shell /bin/bash --group users \
            user1
        ENV EDGEDB_PKG_ROOT {url}
    "###,
        url = HTTP.url(),
        release = release
    )
}

pub fn dockerfile_deb(distro: &str, codename: &str) -> String {
    format!(
        r###"
        FROM {distro}:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates curl \
            sudo gnupg2 apt-transport-https
        ADD ./selfsigned.crt /usr/local/share/ca-certificates/selfsigned.crt
        ADD ./sudoers /etc/sudoers
        RUN update-ca-certificates
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users \
            user1
        ENV EDGEDB_PKG_ROOT {url}
    "###,
        url = HTTP.url(),
        distro = distro,
        codename = codename
    )
}

pub fn default_conf() -> &'static str {
    r###"
        server {
                listen 80 default_server;
                listen [::]:80 default_server;
                listen 443 ssl http2 default_server;
                listen [::]:443 ssl http2 default_server;
                ssl_certificate /etc/ssl/certs/nginx-selfsigned.crt;
                ssl_certificate_key /etc/ssl/private/nginx-selfsigned.key;
                location / {
                        root /http;
                }
                location = /404.html {
                        internal;
                }
        }
    "###
}

pub fn dockerfile_http() -> &'static str {
    r###"
        FROM nginx
        ADD ./default.conf /etc/nginx/conf.d/default.conf
        ADD ./nginx-selfsigned.crt /etc/ssl/certs/nginx-selfsigned.crt
        ADD ./nginx-selfsigned.key /etc/ssl/private/nginx-selfsigned.key
        ADD ./edgedb-init.sh /http/init.sh
        ADD ./edgedb /http/dist/linux-x86_64/edgedb-cli_latest
    "###
}

#[test_case("cli_bionic", dockerfile_deb("ubuntu", "bionic"))]
#[test_case("cli_centos7", dockerfile_centos("7"))]
#[test_case("cli_buster", dockerfile_deb("debian", "buster"))]
fn cli_install(tagname: &str, dockerfile: String) -> anyhow::Result<()> {
    docker::build_image(
        docker::Context::new()
            .add_file("Dockerfile", dockerfile)?
            .add_file("selfsigned.crt", HTTP.cert())?
            .add_sudoers()?,
        tagname,
    )?;
    docker::run_with(
        tagname,
        &format!(
            r###"
            echo "HOME $HOME"
            whoami
            cat /etc/passwd
            curl --proto '=https' --tlsv1.2 -sSf {url}/init.sh | sh -s -- -y
            . ~/.profile
            echo --- DONE ---
            edgedb --version
        "###,
            url = HTTP.url()
        ),
        HTTP.container(),
    )
    .success()
    .stdout(predicates::str::contains("--- DONE ---"))
    .stdout(predicates::function::function(|data: &str| {
        let tail = &data[data.find("--- DONE ---").unwrap()..];
        assert!(tail.contains(concat!("edgedb-cli ", env!("CARGO_PKG_VERSION"))));
        true
    }));
    Ok(())
}

extern "C" fn stop_process() {
    let mut sinfo = HTTP.shutdown_info.lock().expect("shutdown mutex works");
    docker::stop(HTTP.container());
    sinfo.process.wait().ok();
}
