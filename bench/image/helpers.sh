# Sourced by the Dockerfile's RUN steps.
# pin NAME: value of NAME_ARM64 or NAME_AMD64 for the target architecture.
pin() {
  case "$TARGETARCH" in
    arm64) eval "echo \"\${${1}_ARM64}\"" ;;
    amd64) eval "echo \"\${${1}_AMD64}\"" ;;
    *) echo "unsupported architecture: $TARGETARCH" >&2; return 1 ;;
  esac
}
# sri_check FILE SHA512_BASE64: npm registry integrity of a downloaded tarball.
sri_check() {
  python3 -I -c 'import base64,hashlib,sys; sys.exit(base64.b64encode(hashlib.sha512(open(sys.argv[1],"rb").read()).digest()).decode()!=sys.argv[2])' "$1" "$2" \
    || { echo "sha512 mismatch: $1" >&2; return 1; }
}
