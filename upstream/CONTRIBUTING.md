# Contributing to Meeting Minutes Updates

Thank you for your interest in contributing to Meetily! This document provides guidelines and instructions for contributing to this project.

## Table of Contents

- [Development Workflow](#development-workflow)
- [Issue Creation](#issue-creation)
- [Pull Request Process](#pull-request-process)
- [Code Style](#code-style)
- [Commit Message Format](#commit-message-format)
- [Testing, Linting, and Formatting](#testing-linting-and-formatting)
- [Review Process](#review-process)
- [Getting Help](#getting-help)
- [License](#license)

## Development Workflow

### Branch Strategy

- `main` - Production branch
- `devtest` - Development and testing branch
- Feature branches should be created from `devtest`

### Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/meeting-minutes.git
   ```
3. Add the original repository as upstream:
   ```bash
   git remote add upstream https://github.com/Zackriya-Solutions/meeting-minutes.git
   ```
4. Create a new branch from `devtest`:
   ```bash
   git checkout devtest
   git pull upstream devtest
   git checkout -b feature/your-feature-name
   ```

### Development Process

1. Always start your work from the `devtest` branch
2. Create a new branch for each feature/fix
3. Make your changes
4. Write or update tests as needed
5. Ensure all checks pass (see [Testing, Linting, and Formatting](#testing-linting-and-formatting))
6. Update documentation if necessary

### Issue Creation

Before starting work on a new feature or bug fix:

1. Check if an issue already exists
2. If not, create a new issue with:
   - Clear title
   - Detailed description
   - Steps to reproduce (for bugs)
   - Expected behavior
   - Screenshots (if applicable)
   - Labels (bug, enhancement, etc.)

### Pull Request Process

1. Create a PR from your feature branch to `devtest`
2. Link the PR to the related issue using the issue number (e.g., "Fixes #123")
3. Fill out the PR template completely
4. Ensure CI checks pass
5. Request review from at least one maintainer
6. Address any review comments
7. Once approved, the PR will be merged into `devtest`

### PR Template

```markdown
## Description
[Describe your changes here]

## Related Issue
[Link to the issue this PR addresses]

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Documentation update
- [ ] Performance improvement
- [ ] Code refactoring
- [ ] Other (please describe)

## Testing
- [ ] Unit tests added/updated
- [ ] Manual testing performed
- [ ] All tests pass

## Documentation
- [ ] Documentation updated
- [ ] No documentation needed

## Checklist
- [ ] Code follows project style
- [ ] Self-reviewed the code
- [ ] Added comments for complex code
- [ ] Updated README if needed
```

## Code Style

- Follow the existing code style
- Use meaningful variable and function names
- Add comments for complex logic
- Keep functions small and focused
- Write clear commit messages

## Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

Types:
- feat: New feature
- fix: Bug fix
- docs: Documentation changes
- style: Code style changes
- refactor: Code refactoring
- test: Adding/updating tests
- chore: Maintenance tasks

## Testing

- Write unit tests for new features
- Update existing tests when modifying code
- Ensure all tests pass before submitting PR
- Include integration tests for complex features

## Testing, Linting, and Formatting

Every PR must pass the checks in `.github/workflows/ci.yml` before it can
be merged. The relevant local commands live in the `frontend/` directory
and the `frontend/src-tauri/` subdirectory.

### Frontend (TypeScript / Vitest / ESLint / Prettier)

All commands run from the `frontend/` directory.

| Command              | What it does                                               |
| -------------------- | ---------------------------------------------------------- |
| `pnpm install`       | Install JS dependencies (use `--frozen-lockfile` in CI)    |
| `pnpm run typecheck` | `tsc --noEmit` — verify TypeScript types compile           |
| `pnpm test`          | Run the Vitest unit-test suite (4 test files, 17 tests)    |
| `pnpm run lint`      | Run Next.js / ESLint (warnings, non-blocking)              |
| `pnpm run lint:fix`  | Auto-fix ESLint issues where possible                      |
| `pnpm run format`    | Run Prettier to reformat all source files in place         |
| `pnpm run format:check` | Verify all source files are Prettier-formatted           |

### Backend (Rust / cargo test / rustfmt / clippy)

The Rust crate lives at `frontend/src-tauri/`. The workspace also
includes `llama-helper/` and is governed by the root `upstream/Cargo.toml`.

Before running cargo commands, you typically need:

```bash
export LIBCLANG_PATH="/path/to/llvm/lib"      # Windows: C:/Program Files/LLVM/bin
export WHISPER_DONT_GENERATE_BINDINGS=1      # skip bindgen, use vendored bindings
export CARGO_TARGET_DIR="$HOME/cargo-target" # optional, speeds rebuilds
```

If your cargo registry cache has been wiped, restore the vendored
whisper-rs-sys bindings first:

```bash
node frontend/scripts/restore-whisper-bindings.mjs
```

| Command                                    | What it does                                          |
| ------------------------------------------ | ----------------------------------------------------- |
| `cargo fmt --all`                          | Format the whole workspace                            |
| `cargo fmt --all -- --check`               | Verify formatting (CI gate)                           |
| `cargo test --lib --no-default-features --features platform-default --frozen` | Run all 204 unit tests (CI gate)        |
| `cargo clippy --lib --no-default-features --features platform-default` | Lint with the workspace policy in effect (CI gate) |

The workspace lints are defined in `upstream/Cargo.toml` under
`[workspace.lints.clippy]`. `correctness` is `deny`, `complexity`,
`suspicious`, and `style` are `warn` (currently ~100 pre-existing
warnings — see the "Known Lint Warnings" section below).

### Known Lint Warnings

As of this writing, `cargo clippy -- -D warnings` reports ~100
pre-existing `complexity` / `style` lints. They are intentionally NOT
failing the build, but new warnings introduced by a PR will be flagged
in review and should be fixed or explicitly allowed with a comment.

## Documentation

- Update documentation for new features
- Keep README up to date
- Document API changes
- Add comments for complex code

## Review Process

1. PRs require at least one review
2. Address all review comments
3. Keep the PR up to date with `devtest`
4. Squash commits if requested

## Getting Help

- Create an issue for questions
- Join our community chat
- Contact maintainers

## License

By contributing, you agree that your contributions will be licensed under the project's MIT License. 