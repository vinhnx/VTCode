# Cross-Compilation Configuration for VTCode

This document explains the cross-compilation setup for the VTCode project using the `cross` tool.

## Overview

VTCode uses [cross](https://github.com/cross-rs/cross) for reproducible cross-compilation builds across multiple platforms. The configuration is defined in `Cross.toml` to ensure consistent builds for all target platforms.

## Configuration Details

### Supported Target Platforms

The cross configuration supports building for:

- `x86_64-apple-darwin` (Intel Macs)
- `aarch64-apple-darwin` (Apple Silicon Macs) 
- `x86_64-unknown-linux-gnu` (64-bit Linux)
- `aarch64-unknown-linux-gnu` (ARM64 Linux)
- `x86_64-pc-windows-msvc` (64-bit Windows)
- `aarch64-pc-windows-msvc` (ARM64 Windows)

### Key Configuration Elements

#### Docker Images
- Uses official cross-rs Docker images for each target platform
- Ensures reproducible builds with consistent environments
- Includes necessary system dependencies for each platform

#### Pre-build Scripts
- Configures environment variables for cross-compilation
- Sets up platform-specific dependencies like OpenSSL and pkg-config
- Handles macOS SDKROOT configuration when building for Apple platforms

#### Environment Variables
- `PKG_CONFIG_ALLOW_CROSS=1` - Allows pkg-config to work during cross-compilation
- `OPENSSL_*` variables - Configure OpenSSL paths for different platforms
- `SDKROOT` - Sets the macOS SDK path when available

## Usage

### Building for Different Targets

```bash
# Build for a specific target
cross build --target x86_64-apple-darwin --release

# Build for multiple targets
cross build --target x86_64-unknown-linux-gnu --release
cross build --target aarch64-apple-darwin --release
```

### Using with Cargo Commands

With the cross configuration in place, standard cargo commands work with cross:

```bash
# Test on a specific platform
cross test --target aarch64-unknown-linux-gnu

# Run on a specific platform
cross run --target x86_64-pc-windows-msvc
```

## Platform-Specific Notes

### macOS Targets
- Requires Xcode command line tools for SDK access
- Uses macOS SDK path from xcrun when available
- Handles both Intel and Apple Silicon architectures

### Linux Targets
- Installs necessary build dependencies (pkg-config, libssl-dev)
- Uses standard Linux glibc environments

### Windows Targets
- Uses MSVC toolchain for native Windows builds
- Configures vcpkg for system library management

## Integration with Release Process

The cross configuration integrates with the release process:
- Used by `scripts/build-and-upload-binaries.sh` for building release binaries
- Automatically detected by release scripts in `scripts/release.sh`
- Falls back to native cargo if cross is not available (with warning)

## Troubleshooting

### Common Issues

1. **Docker Permission Denied**: Ensure Docker is running and your user has Docker permissions
2. **Missing SDKROOT**: On macOS, ensure Xcode command line tools are installed
3. **Slow Builds**: Cross-compilation can be slower than native builds due to emulation

### Updating Configuration

- New target platforms can be added to `Cross.toml`
- Docker images can be updated to newer versions as needed
- Pre-build scripts can be adjusted for new dependencies

## Maintenance

- Review and update Docker image tags periodically for security updates
- Test cross-compilation regularly to ensure configuration remains valid
- Adjust environment variables as project dependencies change