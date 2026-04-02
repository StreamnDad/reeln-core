FROM quay.io/pypa/manylinux_2_28_x86_64:latest

ARG FFMPEG_VERSION=7.0.2

RUN dnf install -y nasm yasm diffutils pkgconfig curl xz clang-devel && \
    dnf clean all

RUN curl -sL https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz -o /tmp/ffmpeg.tar.xz && \
    tar xf /tmp/ffmpeg.tar.xz -C /tmp && \
    cd /tmp/ffmpeg-${FFMPEG_VERSION} && \
    ./configure \
      --prefix=/usr/local \
      --enable-shared \
      --disable-static \
      --disable-doc \
      --disable-programs && \
    make -j$(nproc) && \
    make install && \
    ldconfig && \
    rm -rf /tmp/ffmpeg*

ENV PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:${PKG_CONFIG_PATH:-}
