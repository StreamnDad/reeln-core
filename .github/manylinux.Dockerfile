FROM quay.io/pypa/manylinux_2_28_x86_64:latest

ARG FFMPEG_VERSION=7.0.2
ARG X264_VERSION=stable

RUN dnf install -y nasm yasm diffutils pkgconfig curl xz clang-devel && \
    dnf clean all

# Build libx264 (GPL, required for h264 encoding)
RUN curl -sL https://code.videolan.org/videolan/x264/-/archive/${X264_VERSION}/x264-${X264_VERSION}.tar.bz2 -o /tmp/x264.tar.bz2 && \
    tar xf /tmp/x264.tar.bz2 -C /tmp && \
    cd /tmp/x264-${X264_VERSION} && \
    ./configure \
      --prefix=/usr/local \
      --enable-shared \
      --disable-static \
      --enable-pic && \
    make -j$(nproc) && \
    make install && \
    ldconfig && \
    rm -rf /tmp/x264*

# Build FFmpeg with libx264 and built-in aac
RUN curl -sL https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz -o /tmp/ffmpeg.tar.xz && \
    tar xf /tmp/ffmpeg.tar.xz -C /tmp && \
    cd /tmp/ffmpeg-${FFMPEG_VERSION} && \
    ./configure \
      --prefix=/usr/local \
      --enable-shared \
      --disable-static \
      --disable-doc \
      --disable-programs \
      --enable-gpl \
      --enable-libx264 && \
    make -j$(nproc) && \
    make install && \
    ldconfig && \
    rm -rf /tmp/ffmpeg*

ENV PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:${PKG_CONFIG_PATH:-}
