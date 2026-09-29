//! Runnable example, level 7 (most operationally complex): pointing this SDK at a
//! self-hosted System One-compatible backend instead of the hosted API — no API
//! key, a caller-chosen model name, and (optionally) the stable `systemOne`
//! surface instead of the alpha `decisions` one.
//!
//! Needs a backend actually listening at `SELF_HOSTED_URL` (default
//! `http://127.0.0.1:8099`) that speaks the System One wire format; point it at
//! Featherless' Simple Jev, `LocalJev`, or your own server. There is no live network
//! call in CI for this example.
//!
//! ```text
//! SELF_HOSTED_URL=http://127.0.0.1:8099 cargo run --example 08_self_hosted_backend --features derive
//! ```

use jev::{Client, Decision, Model, Noul, Surface};

/// A minimal one-question decision, kept small since this example is about client
/// configuration, not the decision shape (see example 3 for multi-question decisions).
#[derive(Debug, Decision)]
struct DoorCheck {
    /// Is the door open?
    is_open: Noul,
}

#[tokio::main]
async fn main() -> Result<(), jev::Error> {
    tracing_subscriber::fmt::init();

    let base_url =
        std::env::var("SELF_HOSTED_URL").unwrap_or_else(|_| "http://127.0.0.1:8099".to_string());

    // No `Authorization` header at all — most self-hosted backends neither expect
    // nor accept one. `Client::new`/`from_env` always send a bearer token; this is
    // the constructor for a backend that has no key to check.
    let client = Client::without_api_key(&base_url)?
        // A self-hosted backend commonly serves one model under a name of its own
        // choosing; `Model::Custom` is never rewritten on either surface (unlike
        // `Model::Jev113`/`Model::Latest`, which are namespaced differently per
        // surface — see `docs/primitives.md` and the crate-level docs on `Model`).
        .with_model(Model::Custom("laya-typed-decisions".to_string()))
        // The stable `v1/systemone` path (bare model IDs, no `session_id`/`trace`),
        // as opposed to the default alpha `decisions` surface. Self-hosted backends
        // commonly speak this one.
        .with_surface(Surface::SystemOne);

    let door: DoorCheck = client.decide("The door creaks open in the wind.").await?;
    println!("{}", door.is_open); // e.g. "yes, 62% sure"

    Ok(())
}
