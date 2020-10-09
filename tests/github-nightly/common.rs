pub fn dock_ubuntu(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update && apt-get install -y ca-certificates sudo gnupg2 apt-transport-https curl software-properties-common
        RUN curl -fsSL https://download.docker.com/linux/ubuntu/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/ubuntu \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

pub fn dock_centos(codename: u32) -> String {
    format!(r###"
        FROM centos:{codename}
        RUN yum -y install sudo yum-utils
        RUN yum-config-manager \
            --add-repo \
            https://download.docker.com/linux/centos/docker-ce.repo
        RUN yum -y install docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --group users \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

pub fn dock_debian(codename: &str) -> String {
    format!(r###"
        FROM debian:{codename}
        RUN apt-get update && apt-get install -y ca-certificates sudo gnupg2 apt-transport-https curl software-properties-common
        RUN curl -fsSL https://download.docker.com/linux/debian/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/debian \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}
