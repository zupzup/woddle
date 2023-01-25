[![CI](https://travis-ci.org/zupzup/woddle.svg?branch=main)](https://travis-ci.org/zupzup/woddle)
[![crates.io](https://meritbadge.herokuapp.com/woddle)](https://crates.io/crates/woddle)
[![docs](https://docs.rs/woddle/badge.svg)](https://docs.rs/woddle)

# woddle

An async, synchronized, database-backed Rust job scheduler

**Note: woddle requires at least Rust 1.60.**

## Usage

```toml
[dependencies]
woddle = "0.4"

# For connection pooling
# woddle = { version = "0.4", features = ["pool-mobc"] }
```

## Features

* Scheduling by interval
* Scheduling via cron (quartz)
* Synchronization between multiple instances via [PostgreSQL](https://www.postgresql.org/)
* Fully asynchronous (currently only [Tokio](https://github.com/tokio-rs/tokio))
* Optional database connection pooling using [mobc](https://github.com/importcjj/mobc)

## Setup

```rust
use woddle::{async_trait, JobConfig, RunnerConfig, Job, JobRunner};
use std::time::Duration;

#[derive(Clone)]
struct MyJob {
    config: JobConfig,
}

#[async_trait]
impl Job for MyJob {
    async fn run(&self) {
        println!("starting  my job!");
    }

    fn get_config(&self) -> &JobConfig {
        &self.config
    }
}

#[tokio::main]
async fn main() {
    let job_cfg = JobConfig::new("my_job", "someSyncKey").interval(Duration::from_secs(120));
    let my_job = MyJob { config: job_cfg };

    let config = RunnerConfig::default().check_interval(Duration::from_secs(60));
    let job_runner = JobRunner::new(config).add_job(my_job);

    tokio::spawn(async move {
        if let Err(e) = job_runner.start().await {
            eprintln!("error: {}", e);
        }
    });
    ...
}

```

## Setup with Connection Pool

```rust
use mobc::Pool;
use mobc_postgres::{tokio_postgres, PgConnectionManager};
use tokio_postgres::{Config, NoTls};
use woddle::{async_trait, JobConfig, RunnerConfig, Job, JobRunner};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let config = Config::from_str("host=localhost user=postgres port=5432")
        .expect("default config is valid");
    let manager = PgConnectionManager::new(config, NoTls);
    let pool = Pool::builder().build(manager);

    let job_cfg = JobConfig::new("my_job", "someSyncKey").cron("0/15 * * * * * *");
    let my_job = MyJob { config: job_cfg };

    let config = RunnerConfig::default()
        .check_interval(Duration::from_secs(1))
        .pool(pool.clone());
    let job_runner = JobRunner::new(config).add_job(my_job);

    tokio::spawn(async move {
        if let Err(e) = job_runner.start().await {
            eprintln!("error: {}", e);
        }
    });
    ...
}
```

### More Examples

Can be found in the [examples](https://github.com/zupzup/woddle/tree/main/examples) folder.

And ran with:

```bash
 RUST_LOG=info cargo run --example basic
 ```

## On intervals and cron

You can configure several intervals, delays and cron scheduling using this library.

#### RunnerConfig

* `initial_delay` - delays the start of the job runner, when it is initially started (defaults to None)
- `check_interval` - defines in which time interval the job runner checks, if any jobs should be run (defaults to 60s)

#### JobConfig

* `interval` - the time interval at which the job should be run
* `cron` - the cron schedule the job should be run at (using quartz cron expressions)

Either `interval`, or `cron` need to be set on every job, otherwise the job runner will exit with an error. One thing to keep in mind when choosing these values is, to give your jobs enough time to run and to set the `check_interval` accordingly.

For example, the minimum run time with a `cron` expression is every 1 second. If you choose this with a very low `check_interval` such as every 10 ms, you might run into trouble. In such a case, it's possible that the job is run several times within the same second due to rounding issues.

So any run intervals in the low seconds, with sub-second checking intervals, might lead to inconsistencies and are out of scope of this library.
