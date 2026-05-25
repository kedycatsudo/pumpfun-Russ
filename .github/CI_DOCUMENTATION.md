 # CI/CD Pipeline Documentation

This project uses GitHub Actions for continuous integration and deployment.

## Workflows

### CI Workflow (`ci.yml`)
Runs on every push to `main` and `develop` branches, and on all pull requests.

**Jobs:**
- **format**: Checks that code follows Rust formatting standards (rustfmt)
- **clippy**: Runs Clippy linter to catch common mistakes and suggest improvements
- **test**: Executes all unit and integration tests
- **build**: Builds the project on Linux, macOS, and Windows to ensure cross-platform compatibility
- **security-audit**: Checks dependencies for known security vulnerabilities

### Release Workflow (`release.yml`)
Automatically triggered when you push a git tag matching `v*` (e.g., `v1.0.0`).

**Jobs:**
- **create-release**: Creates a GitHub release
- **build-and-upload**: Builds binaries for multiple platforms and uploads them to the release:
  - Linux (x86_64)
  - macOS (Intel x86_64 and Apple Silicon aarch64)
  - Windows (x86_64)

## Dependabot Configuration

Automatically checks for:
- **Cargo dependencies**: Weekly updates (Mondays)
- **GitHub Actions**: Monthly updates

Pull requests are created automatically for available updates.

## Getting Started

1. Push code to trigger the CI pipeline:
   ```bash
   git push origin main
   ```

2. Create a release:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```

## Local Pre-commit Checks

Before pushing, you can run these locally:

```bash
# Check formatting
cargo fmt --check

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test

# Build release
cargo build --release
```

Or create a pre-commit hook to automate these checks.
