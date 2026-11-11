FROM ubuntu:22.04 AS builder

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl file build-essential pkg-config \
        libgtk-3-dev libayatana-appindicator3-dev \
        libgirepository1.0-dev librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH

WORKDIR /build
COPY . /build/source
WORKDIR /build/source
RUN cargo build --release --locked

RUN mkdir -p \
        /build/AppDir/usr/bin \
        /build/AppDir/usr/share/applications \
        /build/AppDir/usr/share/icons/hicolor/scalable/apps \
    && install -m 755 target/release/fanspeed /build/AppDir/usr/bin/fanspeed \
    && install -m 755 packaging/appimage/AppRun /build/AppDir/AppRun \
    && install -m 644 io.github.fanspeed.FanSpeed.desktop \
        /build/AppDir/usr/share/applications/io.github.fanspeed.FanSpeed.desktop \
    && install -m 644 io.github.fanspeed.FanSpeed.svg \
        /build/AppDir/usr/share/icons/hicolor/scalable/apps/io.github.fanspeed.FanSpeed.svg \
    && ln -s usr/share/applications/io.github.fanspeed.FanSpeed.desktop \
        /build/AppDir/io.github.fanspeed.FanSpeed.desktop \
    && ln -s usr/share/icons/hicolor/scalable/apps/io.github.fanspeed.FanSpeed.svg \
        /build/AppDir/io.github.fanspeed.FanSpeed.svg \
    && ln -s io.github.fanspeed.FanSpeed.svg /build/AppDir/.DirIcon

RUN curl --location --fail --show-error --retry 3 --retry-all-errors \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage \
        -o /usr/local/bin/linuxdeploy \
    && file /usr/local/bin/linuxdeploy | grep -q 'ELF.*executable' \
    && curl --location --fail --show-error --retry 3 --retry-all-errors \
        https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh \
        -o /usr/local/bin/linuxdeploy-plugin-gtk.sh \
    && head -n 1 /usr/local/bin/linuxdeploy-plugin-gtk.sh | grep -q 'bash' \
    && chmod 755 /usr/local/bin/linuxdeploy /usr/local/bin/linuxdeploy-plugin-gtk.sh

ARG APP_VERSION=1.0.0
RUN APPIMAGE_EXTRACT_AND_RUN=1 DEPLOY_GTK_VERSION=3 VERSION="$APP_VERSION" \
    /usr/local/bin/linuxdeploy \
        --appdir /build/AppDir \
        --executable /build/AppDir/usr/bin/fanspeed \
        --desktop-file /build/AppDir/usr/share/applications/io.github.fanspeed.FanSpeed.desktop \
        --icon-file /build/AppDir/usr/share/icons/hicolor/scalable/apps/io.github.fanspeed.FanSpeed.svg \
        --library /usr/lib/x86_64-linux-gnu/libayatana-appindicator3.so.1 \
        --plugin gtk \
        --output appimage \
    && mv ThinkPad_Fan_Control-*.AppImage /build/ThinkPad_Fan_Control-x86_64.AppImage \
    && chmod 755 /build/ThinkPad_Fan_Control-x86_64.AppImage

CMD ["cp", "/build/ThinkPad_Fan_Control-x86_64.AppImage", "/output/"]
