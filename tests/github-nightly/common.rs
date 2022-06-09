pub fn dock_ubuntu(codename: &str) -> String {
    format!(
        r###"
        FROM ubuntu:{codename}
        ENV DEBIAN_FRONTEND=noninteractive
        RUN apt-get update && apt-get install -y \
            ca-certificates sudo gnupg2 apt-transport-https curl \
            software-properties-common dbus-user-session
        RUN curl -fsSL https://download.docker.com/linux/ubuntu/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/ubuntu \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "Test User" \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###,
        codename = codename
    )
}

pub fn dock_ubuntu_jspy(codename: &str) -> String {
    format!(
        r###"
        FROM ubuntu:{codename}
        ENV DEBIAN_FRONTEND=noninteractive
        RUN apt-get update && apt-get install -y \
            ca-certificates sudo gnupg2 apt-transport-https curl \
            software-properties-common dbus-user-session \
            python3-pip npm
        RUN curl -fsSL https://download.docker.com/linux/ubuntu/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/ubuntu \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "Test User" \
            user1
        RUN pip3 install edgedb
        RUN npm install edgedb
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
        ADD ./edbconnect.py /usr/bin/edbconnect.py
        ADD ./edbconnect.js /usr/bin/edbconnect.js
    "###,
        codename = codename
    )
}

pub fn dock_centos(codename: u32) -> String {
    format!(
        r###"
        FROM centos:{codename}
        RUN sed -i 's/mirrorlist/#mirrorlist/g' /etc/yum.repos.d/CentOS-*
        RUN sed -i 's|#baseurl=http://mirror.centos.org|baseurl=http://vault.centos.org|g' \
                     /etc/yum.repos.d/CentOS-*
        RUN yum -y install sudo yum-utils systemd
        RUN yum-config-manager \
            --add-repo \
            https://download.docker.com/linux/centos/docker-ce.repo
        RUN yum -y install docker-ce-cli
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --group users \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###,
        codename = codename
    )
}

pub fn dock_debian(codename: &str) -> String {
    format!(
        r###"
        FROM debian:{codename}
        ENV DEBIAN_FRONTEND=noninteractive
        RUN apt-get update && apt-get install -y \
            ca-certificates sudo gnupg2 apt-transport-https curl \
            software-properties-common dbus-user-session
        RUN curl -fsSL https://download.docker.com/linux/debian/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/debian \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "Test User" \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###,
        codename = codename
    )
}
