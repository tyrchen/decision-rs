build:
	@cargo build --workspace --all-features

test:
	@cargo nextest run --all-features
	@cargo test --workspace --all-features

test-no-retry:
	@cargo test -p jev-typed --no-default-features --features derive --test wire

test-concurrency:
	@cargo nextest run -p jev-typed --all-features --test wire -E 'test(test_should_include_semaphore_wait_in_deadline_across_clones)' --stress-count 100

test-live:
	@cargo nextest run --all-features -- --ignored

lint:
	@cargo +nightly fmt --all -- --check
	@cargo clippy --workspace --all-targets --all-features --tests --benches -- -D warnings
	@$(MAKE) lint-boundaries

lint-boundaries:
	@cargo clippy --workspace --lib --all-features -- -D warnings -W clippy::pedantic -W clippy::unwrap_used -W clippy::expect_used -W clippy::indexing_slicing -W clippy::panic

feature-check:
	@cargo hack --feature-powerset --no-dev-deps check -p jev-typed

audit:
	@cargo audit
	@cargo deny check

secret-check:
	@gitleaks git --pre-commit --staged --redact --no-banner .

publish-check:
	@cargo publish --workspace --all-features --dry-run

publish:
	@cargo publish --workspace --all-features

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test test-no-retry test-concurrency test-live lint lint-boundaries feature-check audit secret-check publish-check publish release update-submodule
