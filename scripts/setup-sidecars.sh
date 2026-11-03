#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]]; then
  echo 'Only Linux x86_64 has a build recipe. Windows/macOS require separate builds.' >&2; exit 1
fi
target=x86_64-unknown-linux-gnu
mkdir -p .build/downloads .build/whisper .build/ffmpeg src-tauri/binaries src-tauri/resources/sources src-tauri/resources/licenses
fetch() {
  local url="$1" archive="$2" checksum="$3"
  if [[ ! -f "$archive" ]]; then curl --fail --location --proto '=https' --proto-redir '=https' "$url" -o "$archive.part"; mv "$archive.part" "$archive"; fi
  printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
}
fetch 'https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v1.8.3.tar.gz' .build/downloads/whisper-v1.8.3.tar.gz 870ba21409cdf66697dc4db15ebdb13bc67037d76c7cc63756c81471d8f1731a
fetch 'https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz' .build/downloads/ffmpeg-7.1.1.tar.xz 733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1
tar -xf .build/downloads/whisper-v1.8.3.tar.gz -C .build/whisper --strip-components=1
tar -xf .build/downloads/ffmpeg-7.1.1.tar.xz -C .build/ffmpeg --strip-components=1
# Separate caches prevent flags from leaking between CPU variants.
for variant in baseline avx2; do
  simd=OFF
  [[ "$variant" == avx2 ]] && simd=ON
  cmake -S .build/whisper -B ".build/whisper/build-$variant" -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF \
    -DGGML_AVX="$simd" -DGGML_AVX2="$simd" -DGGML_FMA="$simd" \
    -DGGML_F16C="$simd" -DGGML_BMI2="$simd" -DGGML_SSE42="$simd" \
    -DGGML_AVX512=OFF -DGGML_AVX512_VBMI=OFF -DGGML_AVX512_VNNI=OFF \
    -DGGML_AVX512_BF16=OFF -DGGML_AVX_VNNI=OFF \
    -DGGML_AMX_TILE=OFF -DGGML_AMX_INT8=OFF -DGGML_AMX_BF16=OFF \
    -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_METAL=OFF -DGGML_HIP=OFF \
    -DGGML_SYCL=OFF -DGGML_OPENCL=OFF -DGGML_MUSA=OFF -DGGML_WEBGPU=OFF \
    -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_SERVER=OFF
  cmake --build ".build/whisper/build-$variant" --target whisper-cli -j "${LOCALSCRIBE_BUILD_JOBS:-4}"
done
(
  cd .build/ffmpeg
  ./configure --disable-gpl --disable-nonfree --disable-network --disable-autodetect \
    --disable-doc --disable-debug --disable-shared --enable-static --disable-x86asm \
    --disable-ffplay --disable-everything --enable-ffmpeg --enable-ffprobe \
    --enable-protocol=file,pipe --enable-demuxer=mov,matroska,mp3,wav,aac,ogg \
    --enable-decoder=aac,mp3,mp3float,pcm_s16le,pcm_s24le,pcm_s32le,pcm_f32le,flac,alac,opus,vorbis \
    --enable-encoder=pcm_s16le --enable-muxer=wav \
    --enable-filter=aresample,aformat,anull --enable-swresample
  make -j "${LOCALSCRIBE_BUILD_JOBS:-4}"
)
install -m 0755 .build/whisper/build-baseline/bin/whisper-cli "src-tauri/binaries/localscribe-whisper-cli-$target"
install -m 0755 .build/whisper/build-avx2/bin/whisper-cli "src-tauri/binaries/localscribe-whisper-avx2-$target"
install -m 0755 .build/ffmpeg/ffmpeg "src-tauri/binaries/localscribe-ffmpeg-$target"
install -m 0755 .build/ffmpeg/ffprobe "src-tauri/binaries/localscribe-ffprobe-$target"
cp .build/downloads/whisper-v1.8.3.tar.gz .build/downloads/ffmpeg-7.1.1.tar.xz src-tauri/resources/sources/
cp scripts/setup-sidecars.sh src-tauri/resources/sources/build-sidecars.sh
cp .build/whisper/LICENSE src-tauri/resources/licenses/whisper.cpp-MIT.txt
cp .build/ffmpeg/COPYING.LGPLv2.1 src-tauri/resources/licenses/FFmpeg-LGPL-2.1.txt
cp docs/THIRD_PARTY.md src-tauri/resources/licenses/THIRD_PARTY.md
{
  echo "whisper.cpp v1.8.3; baseline and avx2; GPU disabled; GGML_NATIVE=OFF"
  for variant in baseline avx2; do
    echo "variant=$variant"
    rg '^(GGML_|CMAKE_(C|CXX)_FLAGS).*:(BOOL|STRING)=' ".build/whisper/build-$variant/CMakeCache.txt"
  done
  gcc --version
  cmake --version
  .build/ffmpeg/ffmpeg -version
  sha256sum src-tauri/binaries/localscribe-*-"$target"
} > src-tauri/resources/sources/build-manifest.txt
echo 'Sidecars ready. Run pnpm desktop or pnpm bundle.'
