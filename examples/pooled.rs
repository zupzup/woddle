use chrono::prelude::*;
use mobc::Pool;
use mobc_postgres::{tokio_postgres, PgConnectionManager};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Duration;
use tokio_postgres::{Config, NoTls};
use woddle::{async_trait, Job, JobConfig, JobRunner, RunnerConfig};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
struct MyJob {
    config: JobConfig,
}

#[async_trait]
impl Job for MyJob {
    async fn run(&self) {
        log::info!("starting  my job!");
        let val = COUNTER.fetch_add(1, SeqCst);
        log::info!("job done: time: {}, counter: {}", Utc::now(), val);
    }

    fn get_config(&self) -> &JobConfig {
        &self.config
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let config = Config::from_str("host=localhost user=postgres port=5432")
        .expect("default config is valid");
    let manager = PgConnectionManager::new(config, NoTls);
    let pool = Pool::builder().build(manager);

    log::info!("pool set up");
    let job_cfg = JobConfig::new("my_job", "someSyncKey").interval(Duration::from_secs(1));
    let my_job = MyJob { config: job_cfg };

    for _ in 0..10 {
        let config = RunnerConfig::default()
            .check_interval(Duration::from_millis(100))
            .pool(pool.clone());
        let job_runner = JobRunner::new(config).add_job(my_job.clone());

        tokio::time::sleep(Duration::from_millis(50)).await;
        tokio::spawn(async move {
            if let Err(e) = job_runner.start().await {
                log::error!("error: {}", e);
            }
        });
    }

    tokio::time::sleep(Duration::from_secs(6)).await;

    assert!(COUNTER.load(SeqCst) >= 5);
    log::info!("Success!");
}
