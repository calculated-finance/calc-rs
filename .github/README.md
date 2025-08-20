# CI/CD Setup for calc-rs

This document explains the GitHub Actions CI/CD setup for the calc-rs repository.

## Overview

We have implemented automated testing that runs on every pull request and push to the main branch. The CI pipeline ensures code quality and catches issues early.

## Workflows

### 1. Main CI Workflow (`.github/workflows/ci.yml`)

This is the primary workflow that runs comprehensive checks:

- **Format Check**: Ensures all code follows Rust formatting standards
- **Clippy and Build**: Runs the Rust linter and builds all targets
- **Tests**: Runs the complete test suite

**Jobs run in parallel where possible for faster feedback.**

### 2. Basic Checks Workflow (`.github/workflows/basic.yml`)

This serves as a backup workflow that:

- Runs basic code quality checks
- Validates Cargo.toml files
- Checks for common issues (TODOs, println! statements, etc.)
- Continues even if external dependencies fail

## Key Features

### Network Resilience
- Configured to handle the external GitLab dependency (`rujira-rs`)
- Implements retry logic and optimized Git/Cargo settings
- Uses HTTPS instead of SSH for public repository access

### Performance Optimization
- Comprehensive dependency caching
- Parallel job execution
- Job dependencies to fail fast when appropriate

### Developer Experience
- Clear job names and separation of concerns
- Verbose output for debugging
- Integration with existing development workflow

## For Developers

### Local Development
Before pushing code, run these commands locally:

```bash
# Check formatting
cargo fmt --all -- --check

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Build everything
cargo build --all-targets

# Run tests
cargo test --all
```

### CI Status
- All PRs must pass CI checks before merging
- Green checkmarks indicate passing tests
- Failed checks will show detailed logs for debugging

## Troubleshooting

### Common Issues

1. **Formatting failures**: Run `cargo fmt --all` to fix
2. **Clippy warnings**: Fix the specific warnings shown in the logs
3. **Network failures**: The CI includes retry logic, but severe GitLab outages may cause failures

### External Dependencies
The project depends on `rujira-rs` from GitLab. The CI is configured to handle this dependency robustly, but in case of persistent issues, the basic checks workflow provides a fallback.

## Files Structure

```
.github/
├── workflows/
│   ├── ci.yml          # Main CI workflow
│   └── basic.yml       # Backup/basic checks
```

## Maintenance

- Workflows use stable Rust toolchain
- Dependencies are cached to improve performance
- Cache keys include Cargo.lock hash for proper invalidation
- Actions versions are pinned to specific major versions for stability