use chrono::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Duration;
use woddle::{async_trait, Job, JobConfig, JobRunner, RunnerConfig};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
static OTHER_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug)]
struct MyJobContext {
    pub name: String,
}

#[derive(Clone)]
struct MyJob {
    ctx: MyJobContext,
    config: JobConfig,
}

#[async_trait]
impl Job for MyJob {
    async fn run(&self) {
        log::info!(
            "starting {} with context: {:?}..",
            self.config.name,
            self.ctx
        );
        log::info!(
            "job done: time: {}, counter: {}!",
            Utc::now(),
            COUNTER.fetch_add(1, SeqCst)
        );
    }

    fn get_config(&self) -> &JobConfig {
        &self.config
    }
}

#[derive(Clone)]
struct OtherJob {
    config: JobConfig,
}

#[async_trait]
impl Job for OtherJob {
    async fn run(&self) {
        log::info!("starting {}..", self.config.name,);
        log::info!(
            "job done: time: {}, othercounter: {}!",
            Utc::now(),
            OTHER_COUNTER.fetch_add(1, SeqCst)
        );
    }

    fn get_config(&self) -> &JobConfig {
        &self.config
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let my_job = MyJob {
        ctx: MyJobContext {
            name: "heyo".to_string(),
        },
        config: JobConfig::new("my_job", "someSyncKey").cron("* * * * * * *"),
    };

    let other_job = OtherJob {
        config: JobConfig::new("other_job", "otherSyncKey").interval(Duration::from_millis(500)),
    };

    for _ in 0..10 {
        let config = RunnerConfig::default().check_interval(Duration::from_millis(100));
        let job_runner = JobRunner::new(config)
            .add_job(my_job.clone())
            .add_job(other_job.clone());

        tokio::time::delay_for(Duration::from_millis(50)).await;
        tokio::spawn(async move {
            if let Err(e) = job_runner.start().await {
                log::error!("error: {}", e);
            }
        });
    }

    tokio::time::delay_for(Duration::from_secs(11)).await;

    assert!(COUNTER.load(SeqCst) >= 10);
    assert!(OTHER_COUNTER.load(SeqCst) >= 20);
    log::info!("Success!");
}
