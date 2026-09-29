build:
	@cargo build --workspace --all-features

test:
	@cargo nextest run --all-features
	@cargo test --doc --all-features

test-live:
	@cargo nextest run --all-features -- --ignored

lint:
	@cargo +nightly fmt --all -- --check
	@cargo clippy --workspace --all-targets --all-features --tests --benches -- -D warnings

feature-check:
	@cargo hack --feature-powerset check -p jev

audit:
	@cargo audit
	@cargo deny check

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test test-live lint feature-check audit release update-submodule
